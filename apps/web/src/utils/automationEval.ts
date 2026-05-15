import type { AutomationLane, AutomationPoint, AutomationTarget } from "../types/daw";

// ── Sorting ────────────────────────────────────────────────────────────────────

export function sortAutomationPoints(points: AutomationPoint[]): AutomationPoint[] {
  return points.slice().sort((a, b) => a.beat - b.beat);
}

// ── Interpolation ──────────────────────────────────────────────────────────────

function interpolateLinear(a: AutomationPoint, b: AutomationPoint, beat: number): number {
  const t = (beat - a.beat) / (b.beat - a.beat);
  return a.value + (b.value - a.value) * t;
}

/** Evaluate the automation value at a given beat position. Returns defaultValue for empty lanes. */
export function evaluateAutomationAtBeat(
  points: AutomationPoint[],
  beat: number,
  defaultValue: number,
): number {
  if (points.length === 0) return defaultValue;

  const sorted = sortAutomationPoints(points);

  if (beat <= sorted[0].beat) return sorted[0].value;
  if (beat >= sorted[sorted.length - 1].beat) return sorted[sorted.length - 1].value;

  for (let i = 0; i < sorted.length - 1; i++) {
    const a = sorted[i];
    const b = sorted[i + 1];
    if (beat >= a.beat && beat <= b.beat) {
      const curve = b.curve ?? "linear";
      if (curve === "hold") return a.value;
      return interpolateLinear(a, b, beat);
    }
  }

  return defaultValue;
}

export function evaluateAutomationLaneAtBeat(lane: AutomationLane, beat: number): number {
  return evaluateAutomationAtBeat(lane.points, beat, lane.target.defaultValue);
}

// ── Y ↔ Value mapping ──────────────────────────────────────────────────────────

export function automationValueToY(
  value: number,
  target: AutomationTarget,
  laneHeight: number,
): number {
  const range = target.max - target.min;
  if (range === 0) return laneHeight / 2;
  const normalized = (value - target.min) / range;
  return laneHeight - normalized * laneHeight;
}

export function yToAutomationValue(
  y: number,
  target: AutomationTarget,
  laneHeight: number,
): number {
  const normalized = 1 - y / laneHeight;
  return target.min + normalized * (target.max - target.min);
}

export function clampAutomationValue(value: number, target: AutomationTarget): number {
  return Math.max(target.min, Math.min(target.max, value));
}
