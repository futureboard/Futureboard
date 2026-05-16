export type FB2AMode = "compress" | "limit";
export type FB2AMeterMode = "gr" | "output" | "input";

export type FB2AParams = {
  power: boolean;
  peakReduction: number;     // 0..100
  gainDb: number;            // -12..24
  mode: FB2AMode;
  emphasis: number;          // 0..100
  mix: number;               // 0..100
  noise: boolean;
  color: number;             // 0..100
  stereoLink: number;        // 0..100
  meter: FB2AMeterMode;
  sidechainLowCutHz: number; // 20..500
  outputTrimDb: number;      // -12..12
};

export type FB2AOpticalModel = {
  thresholdDb: number;
  ratio: number;
  kneeDb: number;
  attackSec: number;
  releaseSec: number;
  estimatedReductionDb: number;
};

export const FB2A_DEFAULT_PARAMS: FB2AParams = {
  power: true,
  peakReduction: 35,
  gainDb: 0,
  mode: "compress",
  emphasis: 45,
  mix: 100,
  noise: false,
  color: 12,
  stereoLink: 100,
  meter: "gr",
  sidechainLowCutHz: 90,
  outputTrimDb: 0,
};

const VALID_MODES: FB2AMode[] = ["compress", "limit"];
const VALID_METERS: FB2AMeterMode[] = ["gr", "output", "input"];

export function normalizeFB2AParams(
  raw: Record<string, number | string | boolean> | undefined
): FB2AParams {
  const d = FB2A_DEFAULT_PARAMS;
  const mode  = raw?.mode;
  const meter = raw?.meter;
  return {
    power:             asBool(raw?.power, d.power),
    peakReduction:     clamp(asNum(raw?.peakReduction, d.peakReduction), 0, 100),
    gainDb:            clamp(asNum(raw?.gainDb ?? raw?.gain, d.gainDb), -12, 24),
    mode:              VALID_MODES.includes(mode as FB2AMode) ? (mode as FB2AMode) : d.mode,
    emphasis:          clamp(asNum(raw?.emphasis, d.emphasis), 0, 100),
    mix:               clamp(asNum(raw?.mix, d.mix), 0, 100),
    noise:             asBool(raw?.noise, d.noise),
    color:             clamp(asNum(raw?.color, d.color), 0, 100),
    stereoLink:        clamp(asNum(raw?.stereoLink, d.stereoLink), 0, 100),
    meter:             VALID_METERS.includes(meter as FB2AMeterMode) ? (meter as FB2AMeterMode) : d.meter,
    sidechainLowCutHz: clamp(asNum(raw?.sidechainLowCutHz ?? raw?.scCut, d.sidechainLowCutHz), 20, 500),
    outputTrimDb:      clamp(asNum(raw?.outputTrimDb ?? raw?.trim, d.outputTrimDb), -12, 12),
  };
}

export function serializeFB2AParams(
  p: FB2AParams
): Record<string, number | string | boolean> {
  return {
    power: p.power, peakReduction: p.peakReduction, gainDb: p.gainDb,
    mode: p.mode, emphasis: p.emphasis, mix: p.mix, noise: p.noise,
    color: p.color, stereoLink: p.stereoLink, meter: p.meter,
    sidechainLowCutHz: p.sidechainLowCutHz, outputTrimDb: p.outputTrimDb,
  };
}

export function peakReductionToThresholdDb(peakReduction: number): number {
  return -8 - Math.pow(clamp(peakReduction, 0, 100) / 100, 1.18) * 38;
}

export function opticalModelFromParams(p: FB2AParams): FB2AOpticalModel {
  const amount = clamp(p.peakReduction, 0, 100) / 100;
  const emphasis = clamp(p.emphasis, 0, 100) / 100;
  const scCut = clamp(p.sidechainLowCutHz, 20, 500);
  const scRelief = ((scCut - 20) / 480) * 5.5;
  const emphasisPush = (emphasis - 0.5) * 7;
  const limit = p.mode === "limit";
  const thresholdDb = clamp(peakReductionToThresholdDb(p.peakReduction) - emphasisPush + scRelief, -54, -3);
  const ratio = limit ? 12 + amount * 8 : 2.2 + amount * 1.6;
  const kneeDb = limit ? 2.5 + (1 - amount) * 2 : 8 + (1 - amount) * 8;
  const attackSec = clamp((limit ? 0.004 : 0.008) + (1 - amount) * 0.032 - emphasis * 0.003, 0.002, 0.07);
  const releaseSec = clamp(0.12 + amount * 0.68 + emphasis * 0.12, 0.08, 1.1);
  const estimatedReductionDb = estimateGainReductionDb(p);
  return { thresholdDb, ratio, kneeDb, attackSec, releaseSec, estimatedReductionDb };
}

export function estimateGainReductionDb(p: FB2AParams): number {
  const amount = clamp(p.peakReduction, 0, 100) / 100;
  const modeBoost = p.mode === "limit" ? 1.28 : 1;
  const emphasisBoost = 0.9 + (clamp(p.emphasis, 0, 100) / 100) * 0.22;
  const scRelief = 1 - ((clamp(p.sidechainLowCutHz, 20, 500) - 20) / 480) * 0.16;
  return clamp(Math.pow(amount, 0.86) * 18 * modeBoost * emphasisBoost * scRelief, 0, 24);
}

export function colorToDrive(color: number): number {
  return clamp(color, 0, 100) / 100;
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
