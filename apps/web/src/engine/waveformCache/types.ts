export const WAVEFORM_CACHE_VERSION = 1;
export const SAMPLES_PER_PEAK = 256;

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
  /** Float32Array in memory; serialized as number[] in JSON / stored as ArrayBuffer in IDB. */
  peaks: Float32Array | number[];
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
  return new Float32Array(entry.peaks);
}
