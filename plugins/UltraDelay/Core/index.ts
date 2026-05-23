export type DelayTimeDivision =
  | "1/64" | "1/32" | "1/16" | "1/8" | "1/4" | "1/2" | "1/1"
  | "1/16T" | "1/8T" | "1/4T" | "1/2T"
  | "1/16D" | "1/8D" | "1/4D" | "1/2D";

export type UltraDelayMode = "stereo" | "pingpong" | "dual" | "mono";

export type UltraDelayParams = {
  power: boolean;
  mode: UltraDelayMode;
  sync: boolean;
  timeL: DelayTimeDivision;
  timeR: DelayTimeDivision;
  timeMsL: number;       // 1..4000
  timeMsR: number;       // 1..4000
  link: boolean;
  feedback: number;      // 0..98
  crossFeedback: number; // 0..100
  width: number;         // 0..150
  lowCutHz: number;      // 20..2000
  highCutHz: number;     // 1000..20000
  saturation: number;    // 0..100
  modulation: number;    // 0..100
  modRateHz: number;     // 0.01..10
  ducking: number;       // 0..100
  mix: number;           // 0..100
  outputDb: number;      // -24..12
  freeze: boolean;
};

export const ULTRADELAY_DEFAULT_PARAMS: UltraDelayParams = {
  power: true,
  mode: "pingpong",
  sync: true,
  timeL: "1/4",
  timeR: "1/8D",
  timeMsL: 375,
  timeMsR: 563,
  link: false,
  feedback: 34,
  crossFeedback: 65,
  width: 100,
  lowCutHz: 180,
  highCutHz: 9000,
  saturation: 8,
  modulation: 8,
  modRateHz: 0.25,
  ducking: 0,
  mix: 20,
  outputDb: 0,
  freeze: false,
};

const VALID_MODES: UltraDelayMode[] = ["stereo", "pingpong", "dual", "mono"];

const VALID_DIVISIONS: DelayTimeDivision[] = [
  "1/64", "1/32", "1/16", "1/8", "1/4", "1/2", "1/1",
  "1/16T", "1/8T", "1/4T", "1/2T",
  "1/16D", "1/8D", "1/4D", "1/2D",
];

const DIVISION_MULTIPLIERS: Record<DelayTimeDivision, number> = {
  "1/64": 0.0625, "1/32": 0.125,  "1/16": 0.25,  "1/8": 0.5,
  "1/4":  1,      "1/2":  2,       "1/1":  4,
  "1/16T": 0.25 * 2/3, "1/8T": 0.5 * 2/3, "1/4T": 2/3, "1/2T": 4/3,
  "1/16D": 0.375, "1/8D": 0.75,   "1/4D": 1.5,   "1/2D": 3,
};

export function divisionToMs(division: DelayTimeDivision, bpm: number): number {
  const quarterMs = 60000 / Math.max(1, bpm);
  return clamp(quarterMs * DIVISION_MULTIPLIERS[division], 1, 4000);
}

export function normalizeUltraDelayParams(
  raw: Record<string, number | string | boolean> | undefined
): UltraDelayParams {
  const d = ULTRADELAY_DEFAULT_PARAMS;
  const mode = raw?.mode;
  const timeL = raw?.timeL;
  const timeR = raw?.timeR;
  return {
    power:         asBool(raw?.power, d.power),
    mode:          VALID_MODES.includes(mode as UltraDelayMode) ? (mode as UltraDelayMode) : d.mode,
    sync:          asBool(raw?.sync, d.sync),
    timeL:         VALID_DIVISIONS.includes(timeL as DelayTimeDivision) ? (timeL as DelayTimeDivision) : d.timeL,
    timeR:         VALID_DIVISIONS.includes(timeR as DelayTimeDivision) ? (timeR as DelayTimeDivision) : d.timeR,
    timeMsL:       clamp(asNum(raw?.timeMsL, d.timeMsL), 1, 4000),
    timeMsR:       clamp(asNum(raw?.timeMsR, d.timeMsR), 1, 4000),
    link:          asBool(raw?.link, d.link),
    feedback:      clamp(asNum(raw?.feedback, d.feedback), 0, 98),
    crossFeedback: clamp(asNum(raw?.crossFeedback, d.crossFeedback), 0, 100),
    width:         clamp(asNum(raw?.width, d.width), 0, 150),
    lowCutHz:      clamp(asNum(raw?.lowCutHz, d.lowCutHz), 20, 2000),
    highCutHz:     clamp(asNum(raw?.highCutHz, d.highCutHz), 1000, 20000),
    saturation:    clamp(asNum(raw?.saturation, d.saturation), 0, 100),
    modulation:    clamp(asNum(raw?.modulation, d.modulation), 0, 100),
    modRateHz:     clamp(asNum(raw?.modRateHz, d.modRateHz), 0.01, 10),
    ducking:       clamp(asNum(raw?.ducking, d.ducking), 0, 100),
    mix:           clamp(asNum(raw?.mix, d.mix), 0, 100),
    outputDb:      clamp(asNum(raw?.outputDb, d.outputDb), -24, 12),
    freeze:        asBool(raw?.freeze, d.freeze),
  };
}

export function serializeUltraDelayParams(
  p: UltraDelayParams
): Record<string, number | string | boolean> {
  return {
    power: p.power, mode: p.mode, sync: p.sync,
    timeL: p.timeL, timeR: p.timeR, timeMsL: p.timeMsL, timeMsR: p.timeMsR,
    link: p.link, feedback: p.feedback, crossFeedback: p.crossFeedback,
    width: p.width, lowCutHz: p.lowCutHz, highCutHz: p.highCutHz,
    saturation: p.saturation, modulation: p.modulation, modRateHz: p.modRateHz,
    ducking: p.ducking, mix: p.mix, outputDb: p.outputDb, freeze: p.freeze,
  };
}

export function clamp(v: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, v));
}

function asNum(v: number | string | boolean | undefined, fallback: number): number {
  return typeof v === "number" && Number.isFinite(v) ? v : fallback;
}

function asBool(v: number | string | boolean | undefined, fallback: boolean): boolean {
  return typeof v === "boolean" ? v : fallback;
}
