import { resampleLinear } from "./resample";

type F32 = Float32Array<ArrayBufferLike>;

export type TimeStretchQuality = "draft" | "balanced" | "high";

const GRAIN_SIZE: Record<TimeStretchQuality, number> = {
  draft: 1024,
  balanced: 2048,
  high: 4096,
};

const CORR_LEN = 128;

/**
 * Similarity-aligned granular time stretcher.
 * All channels use the same grain positions, preserving the stereo image.
 *
 * stretchRatio = outputDuration / inputDuration
 *   2.0 -> output is twice as long
 *   0.5 -> output is half as long
 */
export function timeStretchGranular(
  channels: F32[],
  stretchRatio: number,
  quality: TimeStretchQuality = "balanced",
): Float32Array[] {
  const ratio = Math.max(0.25, Math.min(4.0, stretchRatio));
  if (channels.length === 0) return [];

  const grainSize = GRAIN_SIZE[quality] ?? GRAIN_SIZE.balanced;
  return stretchChannelsConsistent(channels, ratio, grainSize);
}

function stretchChannelsConsistent(
  channels: F32[],
  stretchRatio: number,
  grainSize: number,
): Float32Array[] {
  const inLen = channels[0].length;
  if (inLen === 0) return channels.map(() => new Float32Array(0));

  if (inLen < grainSize) {
    return channels.map((ch) => resampleLinear(ch, 1 / stretchRatio));
  }

  const hopIn = Math.max(1, grainSize >> 2);
  const hopOut = Math.max(1, Math.round(hopIn * stretchRatio));
  const outLen = Math.max(1, Math.ceil(inLen * stretchRatio));
  const searchRange = Math.max(1, Math.min(hopIn, grainSize >> 3));
  const searchStep = Math.max(1, searchRange >> 3);
  const corrLen = Math.max(1, Math.min(CORR_LEN, grainSize >> 2));
  const refOffset = Math.min(hopIn, Math.max(0, grainSize - corrLen));
  const win = hannWindow(grainSize);

  const windowSum = new Float32Array(outLen);
  const outputs = channels.map(() => new Float32Array(outLen));
  const ref0 = channels[0];

  let expectedInPos = 0;
  let outPos = 0;
  let prevGrain: Float32Array | null = null;

  while (outPos < outLen) {
    let bestPos = Math.max(0, Math.min(inLen - grainSize, expectedInPos));

    if (prevGrain !== null) {
      let bestScore = -Infinity;
      const lo = Math.max(0, expectedInPos - searchRange);
      const hi = Math.min(inLen - grainSize, expectedInPos + searchRange);

      for (let pos = lo; pos <= hi; pos += searchStep) {
        const score = normalizedXCorr(ref0, pos, prevGrain, refOffset, corrLen);
        if (score > bestScore) {
          bestScore = score;
          bestPos = pos;
        }
      }
    }

    const copyLen = Math.min(grainSize, outLen - outPos);

    for (let i = 0; i < copyLen; i++) {
      windowSum[outPos + i] += win[i];
    }

    for (let ch = 0; ch < channels.length; ch++) {
      const src = channels[ch];
      const dst = outputs[ch];
      for (let i = 0; i < copyLen; i++) {
        dst[outPos + i] += src[bestPos + i] * win[i];
      }
    }

    prevGrain = new Float32Array(ref0.buffer, ref0.byteOffset + bestPos * 4, grainSize);
    expectedInPos += hopIn;
    outPos += hopOut;
  }

  for (let i = 0; i < outLen; i++) {
    const w = windowSum[i];
    if (w > 1e-6) {
      const inv = 1 / w;
      for (const dst of outputs) {
        dst[i] *= inv;
      }
    } else {
      const srcPos = Math.min(inLen - 1, Math.floor(i / stretchRatio));
      for (let ch = 0; ch < channels.length; ch++) {
        outputs[ch][i] = channels[ch][srcPos];
      }
    }
  }

  return outputs;
}

function normalizedXCorr(signal: F32, pos: number, ref: F32, refOffset: number, len: number): number {
  let sum = 0;
  let eSig = 0;
  let eRef = 0;
  const n = Math.min(len, signal.length - pos, ref.length - refOffset);
  if (n <= 0) return 0;

  for (let i = 0; i < n; i++) {
    const s = signal[pos + i];
    const r = ref[refOffset + i];
    sum += s * r;
    eSig += s * s;
    eRef += r * r;
  }

  const denom = Math.sqrt(eSig * eRef);
  return denom > 1e-8 ? sum / denom : 0;
}

function hannWindow(size: number): Float32Array {
  const win = new Float32Array(size);
  const n1 = size - 1;
  for (let i = 0; i < size; i++) {
    win[i] = 0.5 * (1 - Math.cos((2 * Math.PI * i) / n1));
  }
  return win;
}
