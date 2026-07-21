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
