import type { DawFile, FileId, WaveformPeaks } from "../types/daw";
import { audioStorage } from "./AudioStorage";
import { generatePeaks } from "./WaveformGenerator";
import { waveformCache, buildCacheKey, entryPeaksAsFloat32, SAMPLES_PER_PEAK, WAVEFORM_CACHE_VERSION } from "./waveformCache";

type OnPeaks = (fileId: FileId, peaks: WaveformPeaks) => void;

type LoadedBuffer = {
  audioBuffer: AudioBuffer;
  peaks: WaveformPeaks;
};

class AudioEngine {
  private _ctx: AudioContext | null = null;
  private bufferCache = new Map<FileId, LoadedBuffer>();

  get ctx(): AudioContext {
    if (!this._ctx) this._ctx = new AudioContext();
    return this._ctx;
  }

  async resume() {
    if (this.ctx.state === "suspended") await this.ctx.resume();
  }

  // ── core decode + cache ──────────────────────────────────────────────────────

  private async decodeAndCache(
    file: DawFile,
    arrayBuffer: ArrayBuffer,
    onPeaks: OnPeaks
  ): Promise<AudioBuffer> {
    const audioBuffer = await this.ctx.decodeAudioData(arrayBuffer.slice(0));

    // Placeholder entry so callers can check existence immediately
    this.bufferCache.set(file.id, {
      audioBuffer,
      peaks: { samplesPerPeak: SAMPLES_PER_PEAK, channelCount: audioBuffer.numberOfChannels, peaks: new Float32Array(0) },
    });

    // Check waveform cache before spawning the worker
    const cacheKey = buildCacheKey(file.id, SAMPLES_PER_PEAK);
    const cached = await waveformCache.get(cacheKey).catch(() => null);

    if (cached) {
      const peaks: WaveformPeaks = {
        samplesPerPeak: cached.samplesPerPeak,
        channelCount: cached.channelCount,
        peaks: entryPeaksAsFloat32(cached),
        sampleRate: cached.sampleRate,
        duration: cached.duration,
      };
      const entry = this.bufferCache.get(file.id);
      if (entry) entry.peaks = peaks;
      onPeaks(file.id, peaks);
    } else {
      generatePeaks(file.id, audioBuffer, (fileId, peaks) => {
        const entry = this.bufferCache.get(fileId);
        if (entry) entry.peaks = peaks;
        onPeaks(fileId, peaks);
        // Persist peaks to cache non-blocking
        waveformCache.set(cacheKey, {
          version: WAVEFORM_CACHE_VERSION,
          fileId,
          sampleRate: audioBuffer.sampleRate,
          channelCount: audioBuffer.numberOfChannels,
          duration: audioBuffer.duration,
          samplesPerPeak: SAMPLES_PER_PEAK,
          peakCount: Math.ceil((peaks.peaks.length / audioBuffer.numberOfChannels) / 2),
          createdAt: Date.now(),
          peaks: peaks.peaks,
        }).catch((e) => console.warn("[WaveformCache] set failed:", e));
      });
    }

    return audioBuffer;
  }

  // ── public API ───────────────────────────────────────────────────────────────

  /** Import: decode, cache in memory, persist to IndexedDB. */
  async loadBuffer(file: DawFile, arrayBuffer: ArrayBuffer, onPeaks: OnPeaks): Promise<AudioBuffer> {
    const buf = await this.decodeAndCache(file, arrayBuffer, onPeaks);
    // Persist raw bytes to IndexedDB (non-blocking)
    audioStorage.save(file.id, arrayBuffer).catch((e) =>
      console.warn("[AudioStorage] save failed:", e)
    );
    return buf;
  }

  /**
   * Restore: try to reload from IndexedDB after a page refresh.
   * Returns the AudioBuffer on success, null if the file is not in storage.
   */
  async restoreBuffer(file: DawFile, onPeaks: OnPeaks): Promise<AudioBuffer | null> {
    if (this.bufferCache.has(file.id)) return this.bufferCache.get(file.id)!.audioBuffer;

    try {
      const stored = await audioStorage.load(file.id);
      if (!stored) return null;
      return await this.decodeAndCache(file, stored, onPeaks);
    } catch (e) {
      console.warn("[AudioEngine] restoreBuffer failed for", file.name, e);
      return null;
    }
  }

  getBuffer(fileId: FileId): LoadedBuffer | undefined {
    return this.bufferCache.get(fileId);
  }

  get destination(): AudioDestinationNode {
    return this.ctx.destination;
  }

  get currentTime(): number {
    return this.ctx.currentTime;
  }
}

export const audioEngine = new AudioEngine();
