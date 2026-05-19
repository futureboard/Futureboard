import type { FileId, WaveformPeaks } from "../types/daw";

type WorkerInput = {
  fileId: FileId;
  source?: Blob;
  channelData?: Float32Array[];
  sampleRate?: number;
  duration?: number;
  samplesPerPeakList: number[];
};

type WorkerOutput =
  | { type: "progress"; fileId: FileId; progress: number; samplesPerPeak: number }
  | { type: "peaks"; fileId: FileId; peaks: WaveformPeaks }
  | { type: "completed"; fileId: FileId }
  | { type: "error"; fileId: FileId; message: string };

type WavInfo = {
  sampleRate: number;
  channels: number;
  bitsPerSample: number;
  audioFormat: number;
  dataOffset: number;
  dataBytes: number;
  duration: number;
};

const CHUNK_BYTES = 1024 * 1024;

// Coarser levels derived from the fine 256 spp scan, posted coarsest-first.
const DERIVED_SPP = [32768, 8192, 4096, 2048, 1024, 512] as const;

function post(message: WorkerOutput, transfer?: Transferable[]) {
  self.postMessage(message, transfer ? { transfer } : undefined);
}

self.onmessage = async (e: MessageEvent<WorkerInput>) => {
  const { fileId, source, channelData, sampleRate, duration } = e.data;

  try {
    if (source) {
      // Single WAV scan at finest resolution, then derive coarser levels (no extra I/O).
      const fine = await generateWavPeaks(fileId, source);

      // Post coarsest first → quick visual feedback, finest last → full detail.
      for (const targetSpp of DERIVED_SPP) {
        const derived = deriveCoarserLevel(fine, targetSpp);
        post({ type: "peaks", fileId, peaks: derived }, [derived.peaks.buffer]);
      }
      post({ type: "peaks", fileId, peaks: fine }, [(fine.peaks as Int16Array).buffer]);
      post({ type: "completed", fileId });
      return;
    }

    if (channelData) {
      const fine = generateFloatPeaks(fileId, channelData, sampleRate ?? 48000, duration ?? 0);
      for (const targetSpp of DERIVED_SPP) {
        const derived = deriveCoarserLevel(fine, targetSpp);
        post({ type: "peaks", fileId, peaks: derived }, [derived.peaks.buffer]);
      }
      post({ type: "peaks", fileId, peaks: fine }, [(fine.peaks as Int16Array).buffer]);
      post({ type: "completed", fileId });
      return;
    }

    throw new Error("No waveform source supplied");
  } catch (error) {
    post({ type: "error", fileId, message: error instanceof Error ? error.message : "Waveform worker failed" });
  }
};

// ── Derive a coarser peak level by downsampling ─────────────────────────────

function deriveCoarserLevel(fine: WaveformPeaks, targetSpp: number): WaveformPeaks {
  const ratio = Math.max(1, Math.round(targetSpp / fine.samplesPerPeak));
  const srcPeaks = fine.peaks as Int16Array;
  const srcPeakCount = fine.peakCount ?? Math.floor(srcPeaks.length / (fine.channelCount * 2));
  const peakCount = Math.ceil(srcPeakCount / ratio);
  const result = new Int16Array(peakCount * fine.channelCount * 2);

  for (let i = 0; i < peakCount; i++) {
    for (let ch = 0; ch < fine.channelCount; ch++) {
      let lo = 32767;
      let hi = -32768;
      for (let j = 0; j < ratio; j++) {
        const k = i * ratio + j;
        if (k >= srcPeakCount) break;
        const base = (k * fine.channelCount + ch) * 2;
        if (srcPeaks[base]     < lo) lo = srcPeaks[base];
        if (srcPeaks[base + 1] > hi) hi = srcPeaks[base + 1];
      }
      const out = (i * fine.channelCount + ch) * 2;
      result[out]     = lo === 32767  ? 0 : lo;
      result[out + 1] = hi === -32768 ? 0 : hi;
    }
  }

  return {
    fileId: fine.fileId,
    samplesPerPeak: targetSpp,
    channelCount: fine.channelCount,
    peakCount,
    peaks: result,
    sampleRate: fine.sampleRate,
    duration: fine.duration,
    version: fine.version,
  };
}

// ── WAV peak generation (returns, does not post) ────────────────────────────

async function generateWavPeaks(
  fileId: FileId,
  source: Blob,
): Promise<WaveformPeaks> {
  const samplesPerPeak = 256;
  const info = await readWavInfo(source);
  if (info.audioFormat !== 1 || ![16, 24, 32].includes(info.bitsPerSample)) {
    throw new Error("Only PCM WAV peak generation is supported without decode");
  }

  const bytesPerSample = info.bitsPerSample / 8;
  const bytesPerFrame = bytesPerSample * info.channels;
  const totalFrames = Math.floor(info.dataBytes / bytesPerFrame);
  const peakCount = Math.ceil(totalFrames / samplesPerPeak);
  const peaks = new Int16Array(peakCount * info.channels * 2);

  let frameIndex = 0;
  let currentPeak = 0;
  const min = new Float32Array(info.channels);
  const max = new Float32Array(info.channels);
  resetMinMax(min, max);

  let byteOffset = info.dataOffset;
  const dataEnd = info.dataOffset + info.dataBytes;
  while (byteOffset < dataEnd) {
    const remaining = dataEnd - byteOffset;
    const chunkBytes = Math.min(remaining, CHUNK_BYTES);
    const alignedChunkBytes = remaining <= CHUNK_BYTES
      ? remaining
      : Math.max(bytesPerFrame, Math.floor(chunkBytes / bytesPerFrame) * bytesPerFrame);
    const nextOffset = byteOffset + alignedChunkBytes;
    const buffer = await source.slice(byteOffset, nextOffset).arrayBuffer();
    const view = new DataView(buffer);
    const frameCount = Math.floor(buffer.byteLength / bytesPerFrame);

    for (let frame = 0; frame < frameCount; frame++) {
      const frameByte = frame * bytesPerFrame;
      for (let ch = 0; ch < info.channels; ch++) {
        const sampleByte = frameByte + ch * bytesPerSample;
        const value = readPcmSample(view, sampleByte, info.bitsPerSample);
        if (value < min[ch]) min[ch] = value;
        if (value > max[ch]) max[ch] = value;
      }

      frameIndex++;
      if (frameIndex % samplesPerPeak === 0) {
        writePeak(peaks, currentPeak, info.channels, min, max);
        currentPeak++;
        resetMinMax(min, max);
      }
    }

    byteOffset = nextOffset;
    const localProgress = Math.min(0.98, (byteOffset - info.dataOffset) / info.dataBytes);
    post({ type: "progress", fileId, samplesPerPeak, progress: localProgress });
  }

  if (currentPeak < peakCount) writePeak(peaks, currentPeak, info.channels, min, max);

  return {
    fileId,
    samplesPerPeak,
    channelCount: info.channels,
    peakCount,
    peaks,
    sampleRate: info.sampleRate,
    duration: info.duration,
    version: 2,
  };
}

async function readWavInfo(source: Blob): Promise<WavInfo> {
  const header = await source.slice(0, Math.min(source.size, 65536)).arrayBuffer();
  const view = new DataView(header);
  if (header.byteLength < 44 || fourCc(view, 0) !== "RIFF" || fourCc(view, 8) !== "WAVE") {
    throw new Error("Invalid WAV file");
  }

  let offset = 12;
  let sampleRate = 0;
  let channels = 0;
  let bitsPerSample = 0;
  let audioFormat = 0;
  let dataOffset = 0;
  let dataBytes = 0;

  while (offset + 8 <= header.byteLength) {
    const id = fourCc(view, offset);
    const size = view.getUint32(offset + 4, true);
    const chunk = offset + 8;
    if (id === "fmt " && chunk + 16 <= header.byteLength) {
      audioFormat = view.getUint16(chunk, true);
      channels = view.getUint16(chunk + 2, true);
      sampleRate = view.getUint32(chunk + 4, true);
      bitsPerSample = view.getUint16(chunk + 14, true);
    } else if (id === "data") {
      dataOffset = chunk;
      dataBytes = size;
      break;
    }
    offset = chunk + size + (size % 2);
  }

  if (!sampleRate || !channels || !bitsPerSample || !dataOffset || !dataBytes) {
    throw new Error("Incomplete WAV metadata");
  }

  const bytesPerFrame = channels * (bitsPerSample / 8);
  return {
    sampleRate,
    channels,
    bitsPerSample,
    audioFormat,
    dataOffset,
    dataBytes,
    duration: dataBytes / bytesPerFrame / sampleRate,
  };
}

function fourCc(view: DataView, offset: number): string {
  return String.fromCharCode(view.getUint8(offset), view.getUint8(offset + 1), view.getUint8(offset + 2), view.getUint8(offset + 3));
}

function readPcmSample(view: DataView, offset: number, bitsPerSample: number): number {
  if (bitsPerSample === 16) return view.getInt16(offset, true) / 32768;
  if (bitsPerSample === 24) {
    const b0 = view.getUint8(offset);
    const b1 = view.getUint8(offset + 1);
    const b2 = view.getUint8(offset + 2);
    let sample = b0 | (b1 << 8) | (b2 << 16);
    if (sample & 0x800000) sample |= 0xff000000;
    return sample / 8388608;
  }
  return view.getInt32(offset, true) / 2147483648;
}

function resetMinMax(min: Float32Array, max: Float32Array): void {
  for (let i = 0; i < min.length; i++) {
    min[i] = 1;
    max[i] = -1;
  }
}

function writePeak(peaks: Int16Array, peakIndex: number, channels: number, min: Float32Array, max: Float32Array): void {
  for (let ch = 0; ch < channels; ch++) {
    const base = (peakIndex * channels + ch) * 2;
    peaks[base] = clampInt16(min[ch]);
    peaks[base + 1] = clampInt16(max[ch]);
  }
}

function clampInt16(value: number): number {
  return Math.max(-32768, Math.min(32767, Math.round(value * 32767)));
}

// ── Float peak generation (pre-decoded channel data) ────────────────────────

function generateFloatPeaks(
  fileId: FileId,
  channelData: Float32Array[],
  sampleRate: number,
  duration: number,
): WaveformPeaks {
  const samplesPerPeak = 256;
  const channelCount = channelData.length;
  const length = channelData[0]?.length ?? 0;
  const peakCount = Math.ceil(length / samplesPerPeak);
  const peaks = new Int16Array(peakCount * channelCount * 2);

  for (let ch = 0; ch < channelCount; ch++) {
    const data = channelData[ch];
    for (let i = 0; i < peakCount; i++) {
      let lo = 0;
      let hi = 0;
      const start = i * samplesPerPeak;
      const end = Math.min(start + samplesPerPeak, length);
      for (let s = start; s < end; s++) {
        const v = data[s];
        if (v < lo) lo = v;
        if (v > hi) hi = v;
      }
      const base = (i * channelCount + ch) * 2;
      peaks[base] = clampInt16(lo);
      peaks[base + 1] = clampInt16(hi);
    }

    post({ type: "progress", fileId, samplesPerPeak, progress: (ch + 1) / channelCount });
  }

  return {
    fileId,
    samplesPerPeak,
    channelCount,
    peakCount,
    peaks,
    sampleRate,
    duration,
    version: 2,
  };
}
