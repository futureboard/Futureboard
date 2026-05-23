export type UltraVerbMode = "room" | "plate" | "hall" | "space";

export type UltraVerbParams = {
  power: boolean;
  mode: UltraVerbMode;
  mix: number;          // 0..100 %
  size: number;         // 0..100 %
  decay: number;        // 0.1..20 s
  preDelayMs: number;   // 0..250 ms
  diffusion: number;    // 0..100 %
  damping: number;      // 0..100 %
  lowCutHz: number;     // 20..1000 Hz
  highCutHz: number;    // 1000..20000 Hz
  width: number;        // 0..150 %
  modulation: number;   // 0..100 %
  modRateHz: number;    // 0.05..5 Hz
  earlyLevel: number;   // -60..6 dB
  lateLevel: number;    // -60..6 dB
  outputDb: number;     // -24..12 dB
  freeze: boolean;
};

export const ULTRAVERB_DEFAULT_PARAMS: UltraVerbParams = {
  power: true,
  mode: "hall",
  mix: 22,
  size: 55,
  decay: 2.4,
  preDelayMs: 18,
  diffusion: 72,
  damping: 48,
  lowCutHz: 140,
  highCutHz: 12000,
  width: 105,
  modulation: 12,
  modRateHz: 0.35,
  earlyLevel: -8,
  lateLevel: 0,
  outputDb: 0,
  freeze: false,
};

const VALID_MODES: UltraVerbMode[] = ["room", "plate", "hall", "space"];

export function normalizeUltraVerbParams(
  raw: Record<string, number | string | boolean> | undefined
): UltraVerbParams {
  const d = ULTRAVERB_DEFAULT_PARAMS;
  const mode = raw?.mode;
  return {
    power:       asBool(raw?.power, d.power),
    mode:        VALID_MODES.includes(mode as UltraVerbMode) ? (mode as UltraVerbMode) : d.mode,
    mix:         clamp(asNum(raw?.mix, d.mix), 0, 100),
    size:        clamp(asNum(raw?.size, d.size), 0, 100),
    decay:       clamp(asNum(raw?.decay, d.decay), 0.1, 20),
    preDelayMs:  clamp(asNum(raw?.preDelayMs, d.preDelayMs), 0, 250),
    diffusion:   clamp(asNum(raw?.diffusion, d.diffusion), 0, 100),
    damping:     clamp(asNum(raw?.damping, d.damping), 0, 100),
    lowCutHz:    clamp(asNum(raw?.lowCutHz, d.lowCutHz), 20, 1000),
    highCutHz:   clamp(asNum(raw?.highCutHz, d.highCutHz), 1000, 20000),
    width:       clamp(asNum(raw?.width, d.width), 0, 150),
    modulation:  clamp(asNum(raw?.modulation, d.modulation), 0, 100),
    modRateHz:   clamp(asNum(raw?.modRateHz, d.modRateHz), 0.05, 5),
    earlyLevel:  clamp(asNum(raw?.earlyLevel, d.earlyLevel), -60, 6),
    lateLevel:   clamp(asNum(raw?.lateLevel, d.lateLevel), -60, 6),
    outputDb:    clamp(asNum(raw?.outputDb, d.outputDb), -24, 12),
    freeze:      asBool(raw?.freeze, d.freeze),
  };
}

export function serializeUltraVerbParams(
  p: UltraVerbParams
): Record<string, number | string | boolean> {
  return {
    power: p.power, mode: p.mode, mix: p.mix, size: p.size,
    decay: p.decay, preDelayMs: p.preDelayMs, diffusion: p.diffusion,
    damping: p.damping, lowCutHz: p.lowCutHz, highCutHz: p.highCutHz,
    width: p.width, modulation: p.modulation, modRateHz: p.modRateHz,
    earlyLevel: p.earlyLevel, lateLevel: p.lateLevel,
    outputDb: p.outputDb, freeze: p.freeze,
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
