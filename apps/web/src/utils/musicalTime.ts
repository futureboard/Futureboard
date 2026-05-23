export type TimeSignature = {
  numerator: number;
  denominator: number;
};

export const DEFAULT_TIME_SIGNATURE: TimeSignature = { numerator: 4, denominator: 4 };

export const TICKS_PER_BEAT = 100;

/** Snap grid divisions. "off" = no snap. "auto" = adapts to zoom level. */
export type SnapDivision =
  | "auto"
  | "off"
  | "1bar"
  | "1/1"
  | "1/2"
  | "1/4"
  | "1/8"
  | "1/16"
  | "1/32"
  | "1/64"
  | "1/1T"
  | "1/2T"
  | "1/4T"
  | "1/8T"
  | "1/16T"
  | "1/32T"
  | "1/64T";

export const ARRANGEMENT_GRID_DIVISIONS: SnapDivision[] = [
  "1/1",
  "1/2",
  "1/4",
  "1/8",
  "1/16",
  "1/32",
  "1/64",
  "1/1T",
  "1/2T",
  "1/4T",
  "1/8T",
  "1/16T",
  "1/32T",
  "1/64T",
];

/** How many quarter-note beats one snap step covers for a given division. */
export function getGridStepBeats(div: SnapDivision): number {
  switch (div) {
    case "auto":  return 0; // dynamic — caller must resolve via getGridSubBeats
    case "off":   return 0;
    case "1bar":  return 4; // caller should multiply by beatsPerBar if needed
    case "1/1":   return 4;
    case "1/2":   return 2;
    case "1/4":   return 1;
    case "1/8":   return 0.5;
    case "1/16":  return 0.25;
    case "1/32":  return 0.125;
    case "1/64":  return 0.0625;
    case "1/1T":  return 4 / 3;
    case "1/2T":  return 2 / 3;
    case "1/4T":  return 1 / 3;
    case "1/8T":  return 1 / 6;
    case "1/16T": return 1 / 12;
    case "1/32T": return 1 / 24;
    case "1/64T": return 1 / 48;
  }
}

/** Snap a beat value to the nearest grid step. */
export function snapBeat(beat: number, div: SnapDivision, timeSig?: TimeSignature): number {
  const step =
    div === "1bar"
      ? (timeSig ? beatsPerBar(timeSig) : 4)
      : getGridStepBeats(div);
  if (step <= 0) return beat;
  return Math.max(0, Math.round(beat / step) * step);
}

/** Convert a quarter-note beat to a PPQN tick. */
export function beatToTick(beat: number, ppq = TICKS_PER_BEAT): number {
  return Math.round(beat * ppq);
}

/** Convert a PPQN tick back to a quarter-note beat. */
export function tickToBeat(tick: number, ppq = TICKS_PER_BEAT): number {
  return tick / ppq;
}

/** Number of whole bars from beat zero to `beats`. */
export function barsFromBeats(beats: number, timeSig: TimeSignature): number {
  return Math.floor(beats / beatsPerBar(timeSig));
}

/** Convert whole bars to quarter-note beats. */
export function barsToBeats(bars: number, timeSig: TimeSignature): number {
  return bars * beatsPerBar(timeSig);
}

/** Decompose a beat position into { bar (1-based), beat (1-based), tick }. */
export function beatsToBarsBeats(
  beats: number,
  timeSig: TimeSignature,
  ppq = TICKS_PER_BEAT,
): { bar: number; beat: number; tick: number } {
  const bpb = beatsPerBar(timeSig);
  const bar = Math.floor(beats / bpb) + 1;
  const beatInBar = Math.floor(beats % bpb) + 1;
  const tick = Math.round((beats - Math.floor(beats)) * ppq);
  return { bar, beat: beatInBar, tick };
}

/**
 * Parse a "bar.beat" or "bar.beat.tick" string back to an absolute beat count.
 * Returns null if the string cannot be parsed.
 */
export function parseBarBeat(
  text: string,
  timeSig: TimeSignature = DEFAULT_TIME_SIGNATURE,
  ppq = TICKS_PER_BEAT,
): number | null {
  const parts = text.trim().split(".");
  const bar  = parseInt(parts[0] ?? "", 10);
  const beat = parseInt(parts[1] ?? "1", 10);
  const tick = parseInt(parts[2] ?? "0", 10);
  if (isNaN(bar) || isNaN(beat) || isNaN(tick)) return null;
  const bpb = beatsPerBar(timeSig);
  return Math.max(0, (bar - 1) * bpb + (beat - 1) + tick / ppq);
}

// ── Timeline view helpers ────────────────────────────────────────────────────

/** Describes the current timeline viewport in pixel/beat coordinates. */
export type TimelineView = {
  pxPerBeat: number;
  scrollLeft: number;
  viewportWidth: number;
};

/** Total scrollable pixel width needed to show `projectLengthBeats`. */
export function getContentWidth(
  projectLengthBeats: number,
  pxPerBeat: number,
  viewportWidth: number,
): number {
  return Math.max(viewportWidth, projectLengthBeats * pxPerBeat + viewportWidth * 0.5);
}

/**
 * After a zoom change, compute the new scrollLeft that keeps the beat at the
 * horizontal center of the viewport fixed in place.
 */
export function preserveCenterBeatOnZoom(
  oldPxPerBeat: number,
  newPxPerBeat: number,
  scrollLeft: number,
  viewportWidth: number,
): number {
  const centerBeat = (scrollLeft + viewportWidth / 2) / oldPxPerBeat;
  return Math.max(0, centerBeat * newPxPerBeat - viewportWidth / 2);
}

/**
 * After a zoom change, compute the new scrollLeft that keeps an arbitrary
 * anchor beat fixed at `anchorOffsetPx` pixels from the left of the viewport.
 *
 * Use this to anchor zoom to the playhead (or any other time-position):
 *   anchorPx      = anchorTime * oldPxPerSecond - scrollLeft  (position within viewport)
 *   anchorOffsetPx = anchorPx clamped to [0, viewportWidth]
 */
export function preserveAnchorBeatOnZoom(
  anchorTimePx: number,      // absolute content-x of the anchor at the old zoom
  anchorOffsetPx: number,    // screen-x offset within viewport where anchor should stay
  newPxPerSecond: number,
  oldPxPerSecond: number,
): number {
  const anchorTimeSec = anchorTimePx / oldPxPerSecond;
  return Math.max(0, anchorTimeSec * newPxPerSecond - anchorOffsetPx);
}

/** Returns the [startBeat, endBeat] range that is currently visible. */
export function getVisibleBeatRange(
  scrollLeft: number,
  viewportWidth: number,
  pxPerBeat: number,
): [number, number] {
  const start = Math.max(0, scrollLeft / pxPerBeat);
  const end = (scrollLeft + viewportWidth) / pxPerBeat;
  return [start, end];
}

/** Clamp a beat value to ≥ 0. */
export function clampBeat(beat: number): number {
  return Math.max(0, beat);
}

export function secondsPerBeat(bpm: number): number {
  return 60 / Math.max(1, bpm);
}

export function secondsToBeats(seconds: number, bpm: number): number {
  return seconds / secondsPerBeat(bpm);
}

export function beatsToSeconds(beats: number, bpm: number): number {
  return beats * secondsPerBeat(bpm);
}

// Quarter-note beats per bar (BPM always counts quarter notes)
export function beatsPerBar(timeSig: TimeSignature): number {
  return timeSig.numerator * (4 / timeSig.denominator);
}

// Build ascending list of valid musical grid intervals (in quarter-note beats)
function buildIntervalList(bpb: number): number[] {
  const result: number[] = [];
  // Sub-beat subdivisions: 1/32, 1/16, 1/8, 1/4, 1/2 (include those finer than one bar)
  for (const sub of [1 / 32, 1 / 16, 1 / 8, 1 / 4, 1 / 2, 1, 2]) {
    if (sub < bpb) result.push(sub);
  }
  // Bar-level multiples
  for (const mult of [1, 2, 4, 8, 16, 32, 64]) {
    result.push(bpb * mult);
  }
  return result;
}

// Minimum pixel gap between ruler labels before stepping up to the next interval.
// Larger value = fewer, more spaced-out labels = cleaner ruler at every zoom level.
const MIN_LABEL_GAP_PX = 100;

export function getGridIntervalBeats(pixelsPerBeat: number, timeSig: TimeSignature): number {
  const bpb = beatsPerBar(timeSig);
  const minBeats = MIN_LABEL_GAP_PX / pixelsPerBeat;
  const intervals = buildIntervalList(bpb);
  return intervals.find((n) => n >= minBeats) ?? intervals[intervals.length - 1];
}

export function getGridSubBeats(pixelsPerBeat: number, timeSig: TimeSignature): number {
  const bpb = beatsPerBar(timeSig);
  const interval = getGridIntervalBeats(pixelsPerBeat, timeSig);
  const intervals = buildIntervalList(bpb);
  const idx = intervals.indexOf(interval);
  return idx > 0 ? intervals[idx - 1] : interval;
}

export function formatBarBeat(seconds: number, bpm: number, timeSig: TimeSignature): string {
  const totalBeats = Math.max(0, secondsToBeats(seconds, bpm));
  const bpb = beatsPerBar(timeSig);
  const bar = Math.floor(totalBeats / bpb) + 1;
  const beat = Math.floor(totalBeats % bpb) + 1;
  return `${bar}.${beat}`;
}

export function formatBarBeatTick(
  seconds: number,
  bpm: number,
  timeSig: TimeSignature = DEFAULT_TIME_SIGNATURE,
): string {
  const totalBeats = Math.max(0, secondsToBeats(seconds, bpm));
  const bpb = beatsPerBar(timeSig);
  const wholeBeats = Math.floor(totalBeats);
  const bar = Math.floor(wholeBeats / bpb) + 1;
  const beat = Math.floor(wholeBeats % bpb) + 1;
  const tick = Math.floor((totalBeats - wholeBeats) * TICKS_PER_BEAT);
  return `${String(bar).padStart(3, "0")}.${beat}.${String(tick).padStart(2, "0")}`;
}

export function formatBeatLength(
  seconds: number,
  bpm: number,
  timeSig: TimeSignature = DEFAULT_TIME_SIGNATURE,
): string {
  const totalBeats = secondsToBeats(seconds, bpm);
  const bpb = beatsPerBar(timeSig);
  if (totalBeats < bpb) {
    return `${Math.round(totalBeats * 10) / 10} bt`;
  }
  const bars = totalBeats / bpb;
  return `${Math.round(bars * 10) / 10} bar`;
}

export function snapTime(
  seconds: number,
  bpm: number,
  timeSig: TimeSignature,
  pixelsPerBeat: number,
  division?: SnapDivision,
): number {
  const isAuto = !division || division === "auto";
  const subDiv = isAuto
    ? getGridSubBeats(pixelsPerBeat, timeSig)
    : division === "1bar"
      ? beatsPerBar(timeSig)
      : getGridStepBeats(division);
  if (subDiv <= 0) return seconds;
  const spb = secondsPerBeat(bpm);
  const totalBeats = seconds / spb;
  const snapped = Math.round(totalBeats / subDiv) * subDiv;
  return Math.max(0, snapped * spb);
}

// ── Shared timeline coordinate helpers ───────────────────────────────────────
// Single source of truth for ruler, grid, playhead, loop region, and clips.
//
// Two coordinate spaces:
//   • CONTENT x — pixels inside the ruler-wrap / grid-wrap (origin = right of
//     the track-header lane).  Used by canvas drawing and loop overlays.
//   • TIMELINE x — pixels inside the outer Timeline container (origin =
//     left of the track-header lane).  Used by the playhead overlay which
//     spans ruler + body and must straddle the header boundary.
//
// TIMELINE x = CONTENT x + TIMELINE_CONTENT_LEFT
//
// Importing HEADER_WIDTH from theme keeps the origin in one place.
import { HEADER_WIDTH } from "../theme";

/** Left edge of the timeline content area within the outer Timeline div. */
export const TIMELINE_CONTENT_LEFT = HEADER_WIDTH;

/** Time → CONTENT x.  Integer-rounded so layers land on the same pixel column. */
export function timeToContentX(
  timeSec: number,
  pixelsPerSecond: number,
  scrollX: number,
): number {
  return Math.round(timeSec * pixelsPerSecond - scrollX);
}

/** CONTENT x → time. */
export function contentXToTime(
  x: number,
  pixelsPerSecond: number,
  scrollX: number,
): number {
  return Math.max(0, (x + scrollX) / Math.max(1, pixelsPerSecond));
}

/** Time → TIMELINE x (for absolute overlays placed in the outer Timeline div). */
export function timeToTimelineX(
  timeSec: number,
  pixelsPerSecond: number,
  scrollX: number,
): number {
  return TIMELINE_CONTENT_LEFT + timeToContentX(timeSec, pixelsPerSecond, scrollX);
}

/** TIMELINE x → time.  Inverse of timeToTimelineX. */
export function timelineXToTime(
  x: number,
  pixelsPerSecond: number,
  scrollX: number,
): number {
  return contentXToTime(x - TIMELINE_CONTENT_LEFT, pixelsPerSecond, scrollX);
}

/** Beat → CONTENT x. */
export function beatsToX(
  beats: number,
  pixelsPerSecond: number,
  bpm: number,
  scrollX: number,
): number {
  return timeToContentX(beats * secondsPerBeat(bpm), pixelsPerSecond, scrollX);
}

/** CONTENT x → beat.  Inverse of beatsToX. */
export function xToBeats(
  x: number,
  pixelsPerSecond: number,
  bpm: number,
  scrollX: number,
): number {
  return contentXToTime(x, pixelsPerSecond, scrollX) / secondsPerBeat(bpm);
}

/** Snap a raw beat value to the nearest grid subdivision. */
export function snapBeats(
  beats: number,
  pixelsPerBeat: number,
  timeSig: TimeSignature,
): number {
  const subDiv = getGridSubBeats(pixelsPerBeat, timeSig);
  return Math.max(0, Math.round(beats / subDiv) * subDiv);
}
