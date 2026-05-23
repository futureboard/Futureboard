import type { DawFile, FileId, WaveformPeaks } from "../types/daw";
import { audioStorage } from "./AudioStorage";
import { generatePeaks } from "./WaveformGenerator";
import { waveformCache, buildCacheKey, entryPeaksAsInt16, SAMPLES_PER_PEAK, WAVEFORM_CACHE_VERSION, WAVEFORM_PEAK_LEVELS } from "./waveformCache";
import { platform } from "../platform";
import { SoundTouchNode } from "@soundtouchjs/audio-worklet";
import soundTouchProcessorUrl from "@soundtouchjs/audio-worklet/processor?url";
import { audioImportQueue } from "./AudioImportQueue";

type OnPeaks = (fileId: FileId, peaks: WaveformPeaks) => void;
type OnWaveformProgress = (fileId: FileId, progress: number) => void;
type OnWaveformError = (fileId: FileId, message: string) => void;

type LoadedBuffer = {
  audioBuffer: AudioBuffer;
  peaks: WaveformPeaks;
};

class AudioEngine {
  private _ctx: AudioContext | null = null;
  private bufferCache = new Map<FileId, LoadedBuffer>();
  private soundTouchWorkletPromise: Promise<void> | null = null;

  get ctx(): AudioContext {
    if (!this._ctx) {
      this._ctx = new AudioContext();
      console.log(`[WebAudio] AudioContext state: ${this._ctx.state}`);
    }
    return this._ctx;
  }

  async resume() {
    console.log(`[WebAudio] AudioContext state: ${this.ctx.state}`);
    if (this.ctx.state === "suspended") {
      await this.ctx.resume();
      console.log(`[WebAudio] resumed: ${this.ctx.state}`);
    }
  }

  async ensureSoundTouchWorklet(): Promise<void> {
    if (!this.soundTouchWorkletPromise) {
      this.soundTouchWorkletPromise = SoundTouchNode.register(this.ctx, soundTouchProcessorUrl);
    }
    await this.soundTouchWorkletPromise;
  }

  createSoundTouchNode(): SoundTouchNode {
    return new SoundTouchNode({ context: this.ctx, outputChannelCount: 2 });
  }

  // ── core decode + cache ──────────────────────────────────────────────────────

  private async decodeAndCache(
    file: DawFile,
    arrayBuffer: ArrayBuffer,
    onPeaks: OnPeaks,
    onProgress?: OnWaveformProgress,
    onError?: OnWaveformError
  ): Promise<AudioBuffer> {
    const audioBuffer = await this.ctx.decodeAudioData(arrayBuffer);

    // Placeholder entry so callers can check existence immediately
    this.bufferCache.set(file.id, {
      audioBuffer,
      peaks: { samplesPerPeak: SAMPLES_PER_PEAK, channelCount: audioBuffer.numberOfChannels, peaks: new Float32Array(0) },
    });

    // Check waveform cache before spawning the worker
    const cacheKeys = [...WAVEFORM_PEAK_LEVELS].reverse().map((samplesPerPeak) => buildCacheKey(file.id, samplesPerPeak));
    const cachedEntries = await Promise.all(cacheKeys.map((key) => waveformCache.get(key).catch(() => null)));
    const cached = cachedEntries.find(Boolean);

    if (cached) {
      const peaks: WaveformPeaks = {
        fileId: file.id,
        samplesPerPeak: cached.samplesPerPeak,
        channelCount: cached.channelCount,
        peakCount: cached.peakCount,
        peaks: entryPeaksAsInt16(cached),
        sampleRate: cached.sampleRate,
        duration: cached.duration,
        version: cached.version,
      };
      const entry = this.bufferCache.get(file.id);
      if (entry) entry.peaks = peaks;
      onPeaks(file.id, peaks);
    } else {
      generatePeaks(file.id, audioBuffer, (fileId, peaks) => {
        const entry = this.bufferCache.get(fileId);
        if (entry) entry.peaks = peaks;
        onPeaks(fileId, peaks);
        const cacheKey = buildCacheKey(fileId, peaks.samplesPerPeak);
        waveformCache.set(cacheKey, {
          version: WAVEFORM_CACHE_VERSION,
          fileId,
          sampleRate: audioBuffer.sampleRate,
          channelCount: audioBuffer.numberOfChannels,
          duration: audioBuffer.duration,
          samplesPerPeak: peaks.samplesPerPeak,
          peakCount: peaks.peakCount ?? Math.ceil((peaks.peaks.length / audioBuffer.numberOfChannels) / 2),
          createdAt: Date.now(),
          peaks: peaks.peaks,
        }).catch((e) => console.warn("[WaveformCache] set failed:", e));
      }, onProgress, onError);
    }

    return audioBuffer;
  }

  // ── public API ───────────────────────────────────────────────────────────────

  /** Import: decode, cache in memory, persist to IndexedDB. */
  async loadBuffer(file: DawFile, arrayBuffer: ArrayBuffer, onPeaks: OnPeaks, onProgress?: OnWaveformProgress, onError?: OnWaveformError): Promise<AudioBuffer> {
    const buf = await this.decodeAndCache(file, arrayBuffer, onPeaks, onProgress, onError);
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
      if (stored) return await this.decodeAndCache(file, stored, onPeaks);
      if (file.storageProvider === "file-handle" && file.cacheKey) {
        const source = await platform.fileSystem.readAudioFile(file.cacheKey);
        if (!source) return null;
        return await this.decodeAndCache(file, await source.arrayBuffer(), onPeaks);
      }
      return null;
    } catch (e) {
      console.warn("[AudioEngine] restoreBuffer failed for", file.name, e);
      return null;
    }
  }

  getBuffer(fileId: FileId): LoadedBuffer | undefined {
    return this.bufferCache.get(fileId);
  }

  adoptDecodedBuffer(file: DawFile, audioBuffer: AudioBuffer): void {
    if (this.bufferCache.has(file.id)) return;
    this.bufferCache.set(file.id, {
      audioBuffer,
      peaks: {
        fileId: file.id,
        samplesPerPeak: SAMPLES_PER_PEAK,
        channelCount: audioBuffer.numberOfChannels,
        peaks: new Int16Array(0),
        sampleRate: audioBuffer.sampleRate,
        duration: audioBuffer.duration,
      },
    });
  }

  async ensureBuffer(file: DawFile): Promise<AudioBuffer | null> {
    const existing = this.bufferCache.get(file.id)?.audioBuffer;
    if (existing) return existing;
    const decoded = await audioImportQueue.ensureDecodedBuffer(file);
    if (decoded) this.adoptDecodedBuffer(file, decoded);
    return decoded;
  }

  get destination(): AudioDestinationNode {
    return this.ctx.destination;
  }

  get currentTime(): number {
    return this.ctx.currentTime;
  }
}

export const audioEngine = new AudioEngine();
