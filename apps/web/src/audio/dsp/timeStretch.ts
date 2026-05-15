import { resampleLinear } from "./resample";

type F32 = Float32Array<ArrayBufferLike>;

export type TimeStretchQuality = "draft" | "balanced" | "high";

const GRAIN_SIZE: Record<TimeStretchQuality, number> = {
  draft:    1024,
  balanced: 2048,
  high:     4096,
};

/**
 * Overlap-add (OLA) granular time stretcher.
 * All channels are processed using the SAME grain positions (stereo safe).
 *
 * stretchRatio = outputDuration / inputDuration
 *   2.0 → output is twice as long (slower playback)
 *   0.5 → output is half as long (faster playback)
 *
 * Does NOT preserve pitch by itself — combine with pitchShiftDraft for that.
 * Quality: draft=1024 grains (fastest), balanced=2048 (default), high=4096.
 */
export function timeStretchGranular(
  channels: F32[],
  stretchRatio: number,
  quality: TimeStretchQuality = "balanced",
): Float32Array[] {
  const ratio = Math.max(0.25, Math.min(4.0, stretchRatio));
  if (channels.length === 0) return [];

  const grainSize = GRAIN_SIZE[quality] ?? 2048;
  return stretchChannelsConsistent(channels, ratio, grainSize);
}

function stretchChannelsConsistent(
  channels: F32[],
  stretchRatio: number,
  grainSize: number,
): Float32Array[] {
  const inLen = channels[0].length;
  if (inLen === 0) return channels.map(() => new Float32Array(0));

  // Very short input: fall back to resampling per channel (same result for all)
  if (inLen < grainSize) {
    return channels.map((ch) => resampleLinear(ch, 1 / stretchRatio));
  }

  const hopIn  = Math.max(1, grainSize >> 2);              // grainSize / 4
  const hopOut = Math.max(1, Math.round(hopIn * stretchRatio));
  const outLen = Math.max(1, Math.ceil(inLen * stretchRatio));
  const win    = hannWindow(grainSize);

  // Shared window accumulator (same grain positions for every channel)
  const windowSum = new Float32Array(outLen);
  const outputs   = channels.map(() => new Float32Array(outLen));

  let inPos  = 0;
  let outPos = 0;

  while (inPos + grainSize <= inLen && outPos < outLen) {
    const copyLen = Math.min(grainSize, outLen - outPos);

    // Accumulate window weights once — shared across channels
    for (let i = 0; i < copyLen; i++) {
      windowSum[outPos + i] += win[i];
    }

    // Apply windowed grain to every channel at the same positions
    for (let ch = 0; ch < channels.length; ch++) {
      const src = channels[ch];
      const dst = outputs[ch];
      for (let i = 0; i < copyLen; i++) {
        dst[outPos + i] += src[inPos + i] * win[i];
      }
    }

    inPos  += hopIn;
    outPos += hopOut;
  }

  // Normalize by accumulated window weight — same denominator for all channels
  for (let i = 0; i < outLen; i++) {
    const w = windowSum[i];
    if (w > 1e-6) {
      const inv = 1 / w;
      for (const dst of outputs) {
        dst[i] *= inv;
      }
    }
  }

  return outputs;
}

function hannWindow(size: number): Float32Array {
  const win = new Float32Array(size);
  const n1  = size - 1;
  for (let i = 0; i < size; i++) {
    win[i] = 0.5 * (1 - Math.cos((2 * Math.PI * i) / n1));
  }
  return win;
}
