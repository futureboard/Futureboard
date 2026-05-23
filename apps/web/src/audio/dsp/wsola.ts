/**
 * WSOLA — Waveform Similarity Overlap-Add time-stretcher.
 *
 * Improvement over basic OLA: for each output frame, a short cross-correlation
 * search finds the best-matching grain in the input around the expected position.
 * This keeps successive grains aligned with the audio's natural periodicity,
 * dramatically reducing phasing/robotic artifacts for pitched/polyphonic material.
 *
 * stretchRatio = outputDuration / inputDuration
 *   2.0 → output is twice as long (slower)
 *   0.5 → output is half as long (faster)
 *
 * Stereo: similarity search uses channel 0 only; chosen grain position is
 * applied identically to all channels, preserving stereo image.
 */

type F32 = Float32Array<ArrayBufferLike>;
type Quality = "draft" | "balanced" | "high";

const GRAIN_SIZE: Record<Quality, number> = {
  draft:    1024,
  balanced: 2048,
  high:     4096,
};

// Cross-correlation window length — short enough to be fast, long enough to lock pitch.
const CORR_LEN = 128;

export function timeStretchWSOLA(
  channels: F32[],
  stretchRatio: number,
  quality: Quality = "balanced",
): Float32Array[] {
  const ratio    = Math.max(0.25, Math.min(4.0, stretchRatio));
  if (channels.length === 0) return [];

  const grainSize   = GRAIN_SIZE[quality] ?? 2048;
  const hopIn       = Math.max(1, grainSize >> 2);         // 25 % of grain
  const hopOut      = Math.max(1, Math.round(hopIn * ratio));
  const searchRange = Math.max(1, Math.min(hopIn, grainSize >> 3)); // ±searchRange around expected pos
  const searchStep  = Math.max(1, searchRange >> 3);       // ~8 candidates to evaluate
  const corrLen     = Math.max(1, Math.min(CORR_LEN, grainSize >> 2));
  const refOffset   = Math.min(hopIn, Math.max(0, grainSize - corrLen));

  const inLen  = channels[0].length;
  const outLen = Math.max(1, Math.ceil(inLen * ratio));

  if (inLen < grainSize) {
    // Input shorter than one grain — fall back to linear resample.
    return channels.map((ch) => resampleLinear(ch, 1 / ratio));
  }

  const win       = hannWindow(grainSize);
  const outputs   = channels.map(() => new Float32Array(outLen));
  const windowSum = new Float32Array(outLen);

  const ref0 = channels[0]; // reference channel for similarity search

  let expectedInPos = 0;
  let outPos        = 0;
  // Holds the last grain placed (ch0 only) for cross-correlation.
  let prevGrain: Float32Array | null = null;

  while (outPos < outLen) {
    let bestPos = expectedInPos;

    if (prevGrain !== null) {
      let bestScore = -Infinity;
      const lo = Math.max(0, expectedInPos - searchRange);
      const hi = Math.min(inLen - grainSize, expectedInPos + searchRange);

      for (let pos = lo; pos <= hi; pos += searchStep) {
        const score = normalizedXCorr(ref0, pos, prevGrain, refOffset, corrLen);
        if (score > bestScore) {
          bestScore = score;
          bestPos   = pos;
        }
      }
    }

    bestPos = Math.max(0, Math.min(inLen - grainSize, bestPos));

    const copyLen = Math.min(grainSize, outLen - outPos);

    // Accumulate window weights once (shared across channels)
    for (let i = 0; i < copyLen; i++) {
      windowSum[outPos + i] += win[i];
    }

    // Apply windowed grain to every channel at the same grain position
    for (let ch = 0; ch < channels.length; ch++) {
      const src = channels[ch];
      const dst = outputs[ch];
      for (let i = 0; i < copyLen; i++) {
        dst[outPos + i] += src[bestPos + i] * win[i];
      }
    }

    // Save current grain (ch0) as reference for next iteration
    prevGrain = new Float32Array(ref0.buffer, ref0.byteOffset + bestPos * 4, grainSize);

    expectedInPos += hopIn;
    outPos        += hopOut;
  }

  // Normalize by accumulated window weight (same denominator for all channels)
  for (let i = 0; i < outLen; i++) {
    const w = windowSum[i];
    if (w > 1e-6) {
      const inv = 1 / w;
      for (const dst of outputs) {
        dst[i] *= inv;
      }
    } else {
      const srcPos = Math.min(inLen - 1, Math.floor(i / ratio));
      for (let ch = 0; ch < channels.length; ch++) {
        outputs[ch][i] = channels[ch][srcPos];
      }
    }
  }

  return outputs;
}

// ── helpers ───────────────────────────────────────────────────────────────────

/** Normalized cross-correlation between signal[pos..] and ref[refOffset..]. */
function normalizedXCorr(signal: F32, pos: number, ref: F32, refOffset: number, len: number): number {
  let sum = 0, eSig = 0, eRef = 0;
  const n = Math.min(len, signal.length - pos, ref.length - refOffset);
  if (n <= 0) return 0;
  for (let i = 0; i < n; i++) {
    const s = signal[pos + i];
    const r = ref[refOffset + i];
    sum  += s * r;
    eSig += s * s;
    eRef += r * r;
  }
  const denom = Math.sqrt(eSig * eRef);
  return denom > 1e-8 ? sum / denom : 0;
}

function hannWindow(size: number): Float32Array {
  const win = new Float32Array(size);
  const n1  = size - 1;
  for (let i = 0; i < size; i++) {
    win[i] = 0.5 * (1 - Math.cos((2 * Math.PI * i) / n1));
  }
  return win;
}

/** Linear resampler used as fallback for very short inputs. */
function resampleLinear(input: F32, speedRatio: number): Float32Array {
  const ratio  = Math.max(0.25, Math.min(4.0, speedRatio));
  const outLen = Math.max(1, Math.ceil(input.length / ratio));
  const output = new Float32Array(outLen);
  const lastIdx = input.length - 1;
  for (let i = 0; i < outLen; i++) {
    const srcPos = i * ratio;
    const lo = Math.floor(srcPos) | 0;
    const hi = lo < lastIdx ? lo + 1 : lastIdx;
    output[i] = input[lo] + (input[hi] - input[lo]) * (srcPos - lo);
  }
  return output;
}
