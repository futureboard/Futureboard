import type { DawFile, FileId, WaveformPeaks } from "../types/daw";
import { audioStorage } from "./AudioStorage";

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

    // Placeholder cache entry so callers can check existence immediately
    this.bufferCache.set(file.id, {
      audioBuffer,
      peaks: { samplesPerPeak: 256, channelCount: audioBuffer.numberOfChannels, peaks: new Float32Array(0) },
    });

    // Generate waveform peaks in a worker
    const channelData: Float32Array[] = [];
    for (let c = 0; c < audioBuffer.numberOfChannels; c++) {
      channelData.push(audioBuffer.getChannelData(c).slice());
    }

    const worker = new Worker(
      new URL("../workers/waveformWorker.ts", import.meta.url),
      { type: "module" }
    );
    worker.postMessage(
      { fileId: file.id, channelData, samplesPerPeak: 256 },
      channelData.map((c) => c.buffer)
    );
    worker.onmessage = (e: MessageEvent<{ fileId: FileId; peaks: WaveformPeaks }>) => {
      const { fileId, peaks } = e.data;
      const entry = this.bufferCache.get(fileId);
      if (entry) entry.peaks = peaks;
      onPeaks(fileId, peaks);
      worker.terminate();
    };

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
