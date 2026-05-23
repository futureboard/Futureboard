/**
 * Shared grid-line math for the Arrangement timeline.
 *
 * Both TimelineGrid (canvas lines) and TimelineRuler (tick marks + labels)
 * call getArrangementGridLines() so their visual hierarchy stays in sync at
 * every zoom level, matching the Piano Roll's bar/beat/sub model.
 */
import {
  beatsPerBar,
  getGridIntervalBeats,
  getGridStepBeats,
  getGridSubBeats,
  secondsPerBeat,
} from "./musicalTime";
import type { SnapDivision, TimeSignature } from "./musicalTime";

export type GridLineLevel = "bar" | "beat" | "sub";

export interface GridLine {
  /** CSS-pixel x, already scroll-adjusted — pass straight to canvas/DOM */
  x: number;
  /** Absolute beat number from the start of the project */
  beat: number;
  /** Visual importance — drives stroke color and ruler tick height */
  level: GridLineLevel;
  /** True when a ruler label should appear at this beat position */
  showLabel: boolean;
}

/** pixels per beat = pixelsPerSecond × secondsPerBeat(bpm) */
export function pxPerBeat(pixelsPerSecond: number, bpm: number): number {
  return pixelsPerSecond * secondsPerBeat(bpm);
}

/**
 * Returns every visible grid line for the Arrangement viewport, tagged by
 * visual level.  Callers iterate once and branch on `line.level`.
 *
 * @param pixelsPerSecond  UI-store zoom value
 * @param bpm              Project BPM
 * @param timeSig          Project time signature
 * @param scrollX          Current horizontal scroll offset in CSS pixels
 * @param viewportWidth    Visible width of the scroll area in CSS pixels
 */
export function getArrangementGridLines(
  pixelsPerSecond: number,
  bpm: number,
  timeSig: TimeSignature,
  scrollX: number,
  viewportWidth: number,
  gridDivision?: SnapDivision,
): GridLine[] {
  const ppb      = pxPerBeat(pixelsPerSecond, bpm);
  const bpb      = beatsPerBar(timeSig);
  const isFixed  = gridDivision && gridDivision !== "off" && gridDivision !== "auto";
  const baseSub  = isFixed
    ? (gridDivision === "1bar" ? bpb : getGridStepBeats(gridDivision))
    : getGridSubBeats(ppb, timeSig);
  let sub = baseSub;
  while (sub * ppb < 4) sub *= 2;
  const interval = getGridIntervalBeats(ppb, timeSig);
  // tolerance proportional to the finest subdivision to handle float drift
  const eps      = sub * 0.01;

  const startBeat = scrollX / ppb;
  const endBeat   = (scrollX + viewportWidth) / ppb;
  const first     = Math.floor(startBeat / sub) * sub;

  const lines: GridLine[] = [];

  for (let beat = first; beat <= endBeat + sub; beat += sub) {
    // 5 decimal places prevents float drift across long sessions
    const rb = Math.round(beat * 100000) / 100000;
    const x  = Math.round(rb * ppb - scrollX);

    // Bar boundary — beat is a multiple of bpb
    const modBar  = ((rb % bpb) + bpb) % bpb;
    const isBar   = modBar < eps || modBar > bpb - eps;

    // Quarter-note beat boundary — whole number, but not a bar
    const modQn   = ((rb % 1) + 1) % 1;
    const isBeat  = !isBar && (modQn < eps || modQn > 1 - eps);

    // Ruler label — aligns with the adaptive major interval so labels never overlap
    const modLbl  = ((rb % interval) + interval) % interval;
    const isLabel = modLbl < eps || modLbl > interval - eps;

    lines.push({
      x,
      beat: rb,
      level: isBar ? "bar" : isBeat ? "beat" : "sub",
      showLabel: isLabel,
    });
  }

  return lines;
}
