import type { FileId } from "../types/daw";
import type { PeakLevelMeta } from "../store/projectStore";

/**
 * Given the per-file peak-level metadata map, return the PeakLevelMeta whose
 * samplesPerPeak best matches the current zoom (pixelsPerSecond).
 *
 * Strategy: aim for ~1 peak per rendered CSS pixel. Walks fine → coarse and
 * returns the coarsest level still fine enough (spp ≤ 2 × ideal). Falls back
 * to the finest available level if nothing qualifies (high zoom, coarse-only).
 */
export function pickBestLevel(
  peakMeta: Map<FileId, Map<number, PeakLevelMeta>>,
  fileId: FileId,
  pixelsPerSecond: number,
  sampleRate = 48000,
): PeakLevelMeta | undefined {
  const fileLevels = peakMeta.get(fileId);
  if (!fileLevels || fileLevels.size === 0) return undefined;

  const idealSpp = Math.max(1, Math.round(sampleRate / pixelsPerSecond));

  const sorted = [...fileLevels.keys()].sort((a, b) => a - b); // fine → coarse
  let best = sorted[0]; // finest as fallback
  for (const spp of sorted) {
    if (spp <= idealSpp * 2) best = spp;
  }
  return fileLevels.get(best);
}
