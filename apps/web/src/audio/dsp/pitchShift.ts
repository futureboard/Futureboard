import { resampleLinear } from "./resample";
import { timeStretchGranular, type TimeStretchQuality } from "./timeStretch";

type F32 = Float32Array<ArrayBufferLike>;

/**
 * Draft-quality pitch shift that preserves approximate duration.
 *
 * Algorithm:
 *   1. Resample by pitchRatio (changes pitch + duration).
 *   2. Time-stretch back to original duration via OLA granular (stereo-consistent).
 *   3. Trim/pad output to exactly the original length.
 *
 * Not artifact-free. Suitable for preview; replace with phase-vocoder later.
 *
 * semitones: -24 to +24
 * quality:   controls OLA grain size (draft=1024, balanced=2048, high=4096)
 */
export function pitchShiftDraft(
  channels: F32[],
  semitones: number,
  quality: TimeStretchQuality = "balanced",
): Float32Array[] {
  const clamped = Math.max(-24, Math.min(24, semitones));
  if (clamped === 0 || channels.length === 0) {
    return channels.map((ch) => new Float32Array(ch));
  }

  const pitchRatio    = Math.pow(2, clamped / 12);
  const originalLength = channels[0].length;

  // Step 1: resample to change pitch (also changes duration)
  // pitchRatio > 1 (up)  → shorter buffer
  // pitchRatio < 1 (down) → longer buffer
  const resampled = channels.map((ch) => resampleLinear(ch, pitchRatio));

  // Step 2: time-stretch back to original duration (stereo-consistent grain positions)
  const stretched = timeStretchGranular(resampled, pitchRatio, quality);

  // Step 3: trim or zero-pad to match original length exactly
  return stretched.map((ch) => {
    if (ch.length === originalLength) return ch;
    const out = new Float32Array(originalLength);
    out.set(ch.subarray(0, Math.min(ch.length, originalLength)));
    return out;
  });
}
