// Meter telemetry store.
//
// Meters update at ~30 Hz. Threading that through React state would re-render
// the header, preset browser and module editor on every frame, so this store is
// deliberately *outside* React: components subscribe and write to their own DOM
// nodes. Nothing here triggers a render.
//
// The store is fed by exactly one of two sources:
//   * the native host, via `subscribeTelemetry` (the real signal), or
//   * the browser Test-DI preview, via `pushSimulatedFrame` (design/preview
//     only, and clearly labelled as such in the UI).
// It never invents levels on its own.

import {
  subscribeTelemetry,
  type HostStatus,
  type MeterFrame,
} from "../bridge";
import { SILENCE_DBFS, linearToDbfs } from "../globals";

export type { MeterFrame, HostStatus };

export const SILENT_FRAME: MeterFrame = {
  inPeak: 0,
  inRms: 0,
  outPeak: 0,
  outRms: 0,
  inClip: false,
  outClip: false,
};

/** Where the current meter reading comes from. Surfaced so the UI can say so. */
export type MeterSource = "none" | "host" | "preview";

type Listener = (frame: MeterFrame) => void;
type StatusListener = (status: HostStatus) => void;
type SourceListener = (source: MeterSource) => void;

let frame: MeterFrame = SILENT_FRAME;
let status: HostStatus = {};
let source: MeterSource = "none";

const listeners = new Set<Listener>();
const statusListeners = new Set<StatusListener>();
const sourceListeners = new Set<SourceListener>();

/** Peak-hold state, decayed by the render loop rather than the producer. */
let holdIn = 0;
let holdOut = 0;
let holdTimestamp = 0;

/** Peak hold time before the indicator starts falling back, in ms. */
const HOLD_MS = 900;
/** Fallback rate once the hold expires, in linear amplitude per second. */
const HOLD_DECAY_PER_S = 0.6;

function emit() {
  for (const fn of listeners) fn(frame);
}

function setSource(next: MeterSource) {
  if (source === next) return;
  source = next;
  for (const fn of sourceListeners) fn(next);
}

function acceptFrame(next: MeterFrame, from: MeterSource) {
  frame = next;
  setSource(from);

  const now = performance.now();
  if (next.inPeak >= holdIn || next.outPeak >= holdOut) holdTimestamp = now;
  const elapsed = (now - holdTimestamp) / 1000;
  const decay = elapsed > HOLD_MS / 1000 ? HOLD_DECAY_PER_S * elapsed : 0;
  holdIn = Math.max(next.inPeak, holdIn - decay);
  holdOut = Math.max(next.outPeak, holdOut - decay);

  emit();
}

/** Latest frame. Safe to read during render for a first paint. */
export function getFrame(): MeterFrame {
  return frame;
}

/** Peak-hold levels (linear), for the meter's hold tick. */
export function getHold(): { in: number; out: number } {
  return { in: holdIn, out: holdOut };
}

export function getStatus(): HostStatus {
  return status;
}

export function getSource(): MeterSource {
  return source;
}

export function subscribeMeters(fn: Listener): () => void {
  listeners.add(fn);
  fn(frame);
  return () => listeners.delete(fn);
}

export function subscribeStatus(fn: StatusListener): () => void {
  statusListeners.add(fn);
  fn(status);
  return () => statusListeners.delete(fn);
}

export function subscribeSource(fn: SourceListener): () => void {
  sourceListeners.add(fn);
  fn(source);
  return () => sourceListeners.delete(fn);
}

/**
 * Feed a frame from the browser Test-DI preview. Ignored while a native host is
 * supplying real telemetry, so the preview can never mask the real signal.
 */
export function pushSimulatedFrame(next: MeterFrame): void {
  if (source === "host") return;
  acceptFrame(next, "preview");
}

/** Reset the local view of the clip indicators (the DSP is told separately). */
export function clearLocalClip(): void {
  frame = { ...frame, inClip: false, outClip: false };
  emit();
}

/** Drop back to the idle state, e.g. when the Test-DI preview is stopped. */
export function releasePreview(): void {
  if (source !== "preview") return;
  frame = SILENT_FRAME;
  holdIn = 0;
  holdOut = 0;
  setSource("none");
  emit();
}

/**
 * Connect the store to the native host. Call once at mount; the returned
 * function detaches. With no host bridge this is inert.
 */
export function attachHostTelemetry(): () => void {
  return subscribeTelemetry({
    onMeters: (next) => acceptFrame(next, "host"),
    onStatus: (next) => {
      status = next;
      for (const fn of statusListeners) fn(next);
    },
  });
}

// ---------------------------------------------------------------------------
// Smoothed (ballistic) meter view
//
// Telemetry frames arrive at whatever cadence the native pump manages —
// nominally ~30 Hz but with real jitter through the CEF bridge. Painting each
// raw frame directly makes the bars step and stutter. This view re-times the
// display on `requestAnimationFrame` with classic meter ballistics: peaks
// attack instantly and fall at a fixed dB/s, RMS glides through a short time
// constant. Purely a *display* transform in dB space — clip flags, calibration
// and the numeric peak source stay on the raw frames.
// ---------------------------------------------------------------------------

/** Display-smoothed levels, in dBFS (floored at {@link SILENCE_DBFS}). */
export type SmoothedLevels = {
  inRmsDb: number;
  inPeakDb: number;
  outRmsDb: number;
  outPeakDb: number;
};

type SmoothedListener = (levels: SmoothedLevels) => void;

/** Peak fallback rate once below the incoming level, in dB per second. */
const PEAK_FALL_DB_PER_S = 30;
/** RMS glide time constants, seconds (rise faster than fall). */
const RMS_RISE_S = 0.05;
const RMS_FALL_S = 0.18;

const smoothed: SmoothedLevels = {
  inRmsDb: SILENCE_DBFS,
  inPeakDb: SILENCE_DBFS,
  outRmsDb: SILENCE_DBFS,
  outPeakDb: SILENCE_DBFS,
};

const smoothedListeners = new Set<SmoothedListener>();
let rafId: number | null = null;
let lastTick = 0;

function ballistics(dtSeconds: number) {
  const dt = Math.min(Math.max(dtSeconds, 0), 0.1);

  const peakStep = (cur: number, target: number) =>
    target >= cur ? target : Math.max(target, cur - PEAK_FALL_DB_PER_S * dt);
  const rmsStep = (cur: number, target: number) => {
    const tau = target > cur ? RMS_RISE_S : RMS_FALL_S;
    return cur + (target - cur) * (1 - Math.exp(-dt / tau));
  };

  smoothed.inPeakDb = peakStep(smoothed.inPeakDb, linearToDbfs(frame.inPeak));
  smoothed.outPeakDb = peakStep(smoothed.outPeakDb, linearToDbfs(frame.outPeak));
  smoothed.inRmsDb = rmsStep(smoothed.inRmsDb, linearToDbfs(frame.inRms));
  smoothed.outRmsDb = rmsStep(smoothed.outRmsDb, linearToDbfs(frame.outRms));
}

function tick(now: number) {
  rafId = null;
  ballistics((now - lastTick) / 1000);
  lastTick = now;
  for (const fn of smoothedListeners) fn(smoothed);
  if (smoothedListeners.size > 0) {
    rafId = requestAnimationFrame(tick);
  }
}

/**
 * Subscribe to the rAF-timed, ballistic meter view. The loop runs only while
 * at least one listener is attached. Safe outside a browser (tests): with no
 * `requestAnimationFrame` the listener receives the resting state once and is
 * never driven.
 */
export function subscribeSmoothedMeters(fn: SmoothedListener): () => void {
  smoothedListeners.add(fn);
  fn(smoothed);
  if (rafId === null && typeof requestAnimationFrame === "function") {
    lastTick = performance.now();
    rafId = requestAnimationFrame(tick);
  }
  return () => {
    smoothedListeners.delete(fn);
    if (smoothedListeners.size === 0 && rafId !== null) {
      cancelAnimationFrame(rafId);
      rafId = null;
    }
  };
}
