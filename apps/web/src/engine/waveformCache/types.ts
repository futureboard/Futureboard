export const WAVEFORM_CACHE_VERSION = 2;
export const WAVEFORM_PEAK_LEVELS = [256, 512, 1024, 2048, 4096, 8192, 16384, 32768] as const;
export const SAMPLES_PER_PEAK = 8192;

export type WaveformCacheEntry = {
  version: number;
  fileId: string;
  fileName?: string;
  fileSize?: number;
  fileLastModified?: number;
  sampleRate: number;
  channelCount: number;
  duration: number;
  samplesPerPeak: number;
  peakCount: number;
  createdAt: number;
  /** Int16Array in memory; serialized as number[] in JSON / stored via structured clone in IDB. */
  peaks: Int16Array | Float32Array | number[];
};

export interface WaveformCacheAdapter {
  get(key: string): Promise<WaveformCacheEntry | null>;
  set(key: string, entry: WaveformCacheEntry): Promise<void>;
  delete(key: string): Promise<void>;
  clear(): Promise<void>;
}

export function buildCacheKey(fileId: string, samplesPerPeak: number = SAMPLES_PER_PEAK): string {
  return `waveform:v${WAVEFORM_CACHE_VERSION}:${fileId}:${samplesPerPeak}`;
}

export function entryPeaksAsFloat32(entry: WaveformCacheEntry): Float32Array {
  if (entry.peaks instanceof Float32Array) return entry.peaks;
  if (entry.peaks instanceof Int16Array) {
    const out = new Float32Array(entry.peaks.length);
    for (let i = 0; i < entry.peaks.length; i++) out[i] = entry.peaks[i] / 32767;
    return out;
  }
  return new Float32Array(entry.peaks);
}

export function entryPeaksAsInt16(entry: WaveformCacheEntry): Int16Array {
  if (entry.peaks instanceof Int16Array) return entry.peaks;
  if (entry.peaks instanceof Float32Array) {
    const out = new Int16Array(entry.peaks.length);
    for (let i = 0; i < entry.peaks.length; i++) {
      out[i] = Math.max(-32768, Math.min(32767, Math.round(entry.peaks[i] * 32767)));
    }
    return out;
  }
  return new Int16Array(entry.peaks);
}
