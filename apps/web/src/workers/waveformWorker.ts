import type { FileId, WaveformPeaks } from "../types/daw";

type WorkerInput = {
  fileId: FileId;
  channelData: Float32Array[];
  samplesPerPeak: number;
};

self.onmessage = (e: MessageEvent<WorkerInput>) => {
  const { fileId, channelData, samplesPerPeak } = e.data;
  const channelCount = channelData.length;
  const length = channelData[0]?.length ?? 0;
  const peakCount = Math.ceil(length / samplesPerPeak);

  // Interleaved min/max pairs per channel: [ch0_min, ch0_max, ch1_min, ch1_max, ...]
  const peaks = new Float32Array(peakCount * channelCount * 2);

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
      peaks[base] = min;
      peaks[base + 1] = max;
    }
  }

  const result: WaveformPeaks = { samplesPerPeak, channelCount, peaks };
  self.postMessage({ fileId, peaks: result }, [peaks.buffer]);
};
