// Global (non-per-model) plugin parameters and level formatting helpers.
//
// Every entry here maps 1:1 onto a real field in the Rust `Params` struct and a
// real arm of `Dsp::apply_ui_param` (`rodharerist/src/dsp/mod.rs`). Nothing in
// this file is decorative: if a control does not have a DSP parameter behind
// it, it does not belong here.

import type { Param } from "./data";

/** A DSP-facing parameter description. `id` is the stable automation id. */
export type GlobalParamSpec = Param & {
  /** Longer label for tooltips/menus, where the compact `name` is ambiguous. */
  displayName: string;
  /** Discrete increment for keyboard/wheel nudging, in parameter units. */
  step: number;
};

export const INPUT_TRIM: GlobalParamSpec = {
  id: "input_trim",
  name: "In Trim",
  displayName: "Input Trim",
  min: -24,
  max: 24,
  val: 0,
  unit: "dB",
  step: 0.1,
};

export const OUTPUT_TRIM: GlobalParamSpec = {
  id: "output_trim",
  name: "Out Trim",
  displayName: "Output Trim",
  min: -24,
  max: 24,
  val: 0,
  unit: "dB",
  step: 0.1,
};

/** Global bypass. Mapped to the DSP's `power` param (inverted). */
export const POWER_PARAM_ID = "power";

export const globalParams: GlobalParamSpec[] = [INPUT_TRIM, OUTPUT_TRIM];

/** Floor below which a level is reported as silence rather than a huge negative. */
export const SILENCE_DBFS = -60;

/** Linear 0..1 amplitude → dBFS, floored at {@link SILENCE_DBFS}. */
export function linearToDbfs(linear: number): number {
  if (!(linear > 0)) return SILENCE_DBFS;
  const db = 20 * Math.log10(linear);
  return db < SILENCE_DBFS ? SILENCE_DBFS : db;
}

/** Format a dBFS level for a meter readout: `-18.2` / `-inf`. */
export function formatDbfs(db: number): string {
  if (db <= SILENCE_DBFS) return "-∞";
  return db.toFixed(1);
}

/** Format a trim value with an explicit sign, as trims are bipolar. */
export function formatTrim(db: number): string {
  const rounded = Math.abs(db) < 0.05 ? 0 : db;
  return `${rounded > 0 ? "+" : ""}${rounded.toFixed(1)}`;
}

/**
 * Map a dBFS level onto a 0..1 meter position. Linear-in-dB over the visible
 * range so the meter has usable resolution where guitar signals actually sit,
 * instead of squashing everything below -20 dBFS into the first pixel.
 */
export function meterPosition(db: number): number {
  const p = (db - SILENCE_DBFS) / (0 - SILENCE_DBFS);
  return p < 0 ? 0 : p > 1 ? 1 : p;
}

/**
 * NAM input calibration bands. A NAM capture is trained at a specific input
 * level; feeding it hotter or colder changes its gain *and* its voicing, so the
 * editor states plainly where the current signal sits rather than hiding this
 * in an advanced dialog.
 *
 * The bands describe the measured input level only — they are not a claim that
 * any particular capture was trained at a particular level, which the `.nam`
 * metadata may or may not record.
 */
export type CalibrationState = "silent" | "low" | "calibrated" | "hot" | "clipping";

export function calibrationFor(peakDb: number, clipped: boolean): CalibrationState {
  if (clipped || peakDb >= -0.1) return "clipping";
  if (peakDb <= SILENCE_DBFS) return "silent";
  if (peakDb < -24) return "low";
  if (peakDb > -6) return "hot";
  return "calibrated";
}

export const calibrationLabel: Record<CalibrationState, string> = {
  silent: "No signal",
  low: "Low",
  calibrated: "Calibrated",
  hot: "Hot",
  clipping: "Clipping",
};

/**
 * Physical range the `cab_dist` 0..100 % parameter spans, for display only.
 * The DSP treats distance as a normalized roll-off amount; showing it in cm
 * gives the control a meaningful scale without claiming a measured model.
 */
const DISTANCE_CM_MIN = 0;
const DISTANCE_CM_MAX = 30;

export function distanceCm(pct: number): number {
  return DISTANCE_CM_MIN + (pct / 100) * (DISTANCE_CM_MAX - DISTANCE_CM_MIN);
}

/** `cab_mic` 0 % = dead centre (on-axis), 100 % = speaker edge (off-axis). */
export function positionLabel(pct: number): string {
  if (pct < 12) return "Centre";
  if (pct > 78) return "Edge";
  return "Off-centre";
}

/** `cab_mic_type` append-only enum: Dynamic=0, Ribbon=1, Condenser=2. */
export function micTypeLabel(value: number): string {
  return ["Dynamic", "Ribbon", "Condenser"][Math.round(value)] ?? "Dynamic";
}
