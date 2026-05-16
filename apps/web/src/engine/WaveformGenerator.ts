import type { FileId, WaveformPeaks } from "../types/daw";
import { WAVEFORM_PEAK_LEVELS } from "./waveformCache";

type WorkerMessage =
  | { type: "progress"; fileId: FileId; progress: number }
  | { type: "peaks"; fileId: FileId; peaks: WaveformPeaks }
  | { type: "completed"; fileId: FileId }
  | { type: "error"; fileId: FileId; message: string };

export function generatePeaks(
  fileId: FileId,
  audioBuffer: AudioBuffer,
  onPeaks: (fileId: FileId, peaks: WaveformPeaks) => void,
  onProgress?: (fileId: FileId, progress: number) => void,
  onError?: (fileId: FileId, message: string) => void,
  samplesPerPeakList: number[] = [...WAVEFORM_PEAK_LEVELS].reverse()
): void {
  const channelData: Float32Array[] = [];
  for (let c = 0; c < audioBuffer.numberOfChannels; c++) {
    channelData.push(audioBuffer.getChannelData(c).slice());
  }

  const worker = new Worker(
    new URL("../workers/waveformWorker.ts", import.meta.url),
    { type: "module" }
  );

  worker.postMessage(
    {
      fileId,
      channelData,
      sampleRate: audioBuffer.sampleRate,
      duration: audioBuffer.duration,
      samplesPerPeakList,
    },
    channelData.map((c) => c.buffer)
  );

  worker.onmessage = (e: MessageEvent<WorkerMessage>) => {
    if (e.data.type === "progress") {
      onProgress?.(e.data.fileId, e.data.progress);
      return;
    }
    if (e.data.type === "peaks") {
      onPeaks(e.data.fileId, e.data.peaks);
      return;
    }
    if (e.data.type === "error") {
      onError?.(e.data.fileId, e.data.message);
    }
    worker.terminate();
  };

  worker.onerror = () => {
    onError?.(fileId, "Waveform worker failed");
    worker.terminate();
  };
}
