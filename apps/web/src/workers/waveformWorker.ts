import type { FileId, WaveformPeaks } from "../types/daw";

type WorkerInput = {
  fileId: FileId;
  channelData: Float32Array[];
  sampleRate: number;
  duration: number;
  samplesPerPeakList: number[];
};

type WorkerOutput =
  | { type: "progress"; fileId: FileId; progress: number; samplesPerPeak: number }
  | { type: "peaks"; fileId: FileId; peaks: WaveformPeaks }
  | { type: "completed"; fileId: FileId }
  | { type: "error"; fileId: FileId; message: string };

function post(message: WorkerOutput, transfer?: Transferable[]) {
  self.postMessage(message, transfer ? { transfer } : undefined);
}

self.onmessage = (e: MessageEvent<WorkerInput>) => {
  const { fileId, channelData, sampleRate, duration, samplesPerPeakList } = e.data;
  const channelCount = channelData.length;
  const length = channelData[0]?.length ?? 0;
  const totalLevels = Math.max(1, samplesPerPeakList.length);

  try {
    for (let levelIndex = 0; levelIndex < samplesPerPeakList.length; levelIndex++) {
      const samplesPerPeak = samplesPerPeakList[levelIndex];
      const peakCount = Math.ceil(length / samplesPerPeak);
      const peaks = new Int16Array(peakCount * channelCount * 2);

      for (let ch = 0; ch < channelCount; ch++) {
        const data = channelData[ch];
        for (let i = 0; i < peakCount; i++) {
          let min = 0;
          let max = 0;
          const start = i * samplesPerPeak;
          const end = Math.min(start + samplesPerPeak, length);
          for (let s = start; s < end; s++) {
            const v = data[s];
            if (v < min) min = v;
            if (v > max) max = v;
          }
          const base = (i * channelCount + ch) * 2;
          peaks[base] = Math.max(-32768, Math.min(32767, Math.round(min * 32767)));
          peaks[base + 1] = Math.max(-32768, Math.min(32767, Math.round(max * 32767)));
        }

        post({
          type: "progress",
          fileId,
          samplesPerPeak,
          progress: (levelIndex + ((ch + 1) / channelCount)) / totalLevels,
        });
      }

      post({
        type: "peaks",
        fileId,
        peaks: {
          fileId,
          samplesPerPeak,
          channelCount,
          peakCount,
          peaks,
          sampleRate,
          duration,
          version: 2,
        },
      }, [peaks.buffer]);
    }

    post({ type: "completed", fileId });
  } catch (error) {
    post({ type: "error", fileId, message: error instanceof Error ? error.message : "Waveform worker failed" });
  }
};
