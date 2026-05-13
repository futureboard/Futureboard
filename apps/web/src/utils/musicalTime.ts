export type TimeSignature = {
  numerator: number;
  denominator: number;
};

export const DEFAULT_TIME_SIGNATURE: TimeSignature = { numerator: 4, denominator: 4 };

const TICKS_PER_BEAT = 100;

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
  for (const sub of [1 / 16, 1 / 8, 1 / 4, 1 / 2, 1, 2]) {
    if (sub < bpb) result.push(sub);
  }
  for (const mult of [1, 2, 4, 8, 16, 32]) {
    result.push(bpb * mult);
  }
  return result;
}

// Minimum pixel gap between major grid lines before stepping up to next interval
const MIN_LABEL_GAP_PX = 72;

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
): number {
  const subDiv = getGridSubBeats(pixelsPerBeat, timeSig);
  const spb = secondsPerBeat(bpm);
  const totalBeats = seconds / spb;
  const snapped = Math.round(totalBeats / subDiv) * subDiv;
  return Math.max(0, snapped * spb);
}
