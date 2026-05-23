// Use the wide typed-array form so inputs from both AudioBuffer and new Float32Array() are accepted.
type F32 = Float32Array<ArrayBufferLike>;

/**
 * Linear interpolation resampler.
 *
 * Convention:
 *   speedRatio 2.0 → output is half the length → audio plays twice as fast → higher pitch
 *   speedRatio 0.5 → output is double the length → audio plays at half speed → lower pitch
 *
 * outLength = ceil(inLength / speedRatio)
 */
export function resampleLinear(input: F32, speedRatio: number): Float32Array {
  const ratio = Math.max(0.25, Math.min(4.0, speedRatio));

  if (input.length === 0) return new Float32Array(0);
  if (ratio === 1) return new Float32Array(input);

  const outLen = Math.max(1, Math.ceil(input.length / ratio));
  const output = new Float32Array(outLen);
  const lastIdx = input.length - 1;

  for (let i = 0; i < outLen; i++) {
    const srcPos = i * ratio;
    const lo = Math.floor(srcPos) | 0;
    const hi = lo < lastIdx ? lo + 1 : lastIdx;
    const frac = srcPos - lo;
    output[i] = input[lo] + (input[hi] - input[lo]) * frac;
  }

  return output;
}

/** Apply resampleLinear to each channel. */
export function resampleChannels(channels: F32[], speedRatio: number): Float32Array[] {
  return channels.map((ch) => resampleLinear(ch, speedRatio));
}
