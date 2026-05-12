import type { DawFile, FileId, WaveformPeaks } from "../types/daw";

type LoadedBuffer = {
  audioBuffer: AudioBuffer;
  peaks: WaveformPeaks;
};

class AudioEngine {
  private _ctx: AudioContext | null = null;
  private bufferCache = new Map<FileId, LoadedBuffer>();
  private worker: Worker | null = null;

  get ctx(): AudioContext {
    if (!this._ctx) {
      this._ctx = new AudioContext();
    }
    return this._ctx;
  }

  async resume() {
    if (this.ctx.state === "suspended") {
      await this.ctx.resume();
    }
  }

  async loadBuffer(
    file: DawFile,
    arrayBuffer: ArrayBuffer,
    onPeaks: (fileId: FileId, peaks: WaveformPeaks) => void
  ): Promise<AudioBuffer> {
    const audioBuffer = await this.ctx.decodeAudioData(arrayBuffer.slice(0));

    const channelData: Float32Array[] = [];
    for (let c = 0; c < audioBuffer.numberOfChannels; c++) {
      channelData.push(audioBuffer.getChannelData(c).slice());
    }

    const worker = new Worker(
      new URL("../workers/waveformWorker.ts", import.meta.url),
      { type: "module" }
    );

    worker.postMessage({ fileId: file.id, channelData, samplesPerPeak: 256 }, channelData.map((c) => c.buffer));

    worker.onmessage = (e: MessageEvent<{ fileId: FileId; peaks: WaveformPeaks }>) => {
      const { fileId, peaks } = e.data;
      const existing = this.bufferCache.get(fileId);
      if (existing) {
        existing.peaks = peaks;
      }
      onPeaks(fileId, peaks);
      worker.terminate();
    };

    this.bufferCache.set(file.id, { audioBuffer, peaks: { samplesPerPeak: 256, channelCount: audioBuffer.numberOfChannels, peaks: new Float32Array(0) } });
    return audioBuffer;
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
