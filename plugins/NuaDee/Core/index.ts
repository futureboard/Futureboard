export type NuaDeeParams = {
  power: boolean;
  gain: number;       // -24..24 dB  (pre-gain into saturation)
  boost: number;      // 0..100 %    (harmonic EQ boost: low shelf + high shelf)
  saturation: number; // 0..100 %    (tanh drive amount)
  mix: number;        // 0..100 %    (dry/wet blend)
  out: number;        // -24..12 dB  (output trim)
};

export const NUADEE_DEFAULT_PARAMS: NuaDeeParams = {
  power: true,
  gain: 0,
  boost: 0,
  saturation: 25,
  mix: 100,
  out: 0,
};

export function normalizeNuaDeeParams(
  raw: Record<string, number | string | boolean> | undefined,
): NuaDeeParams {
  const d = NUADEE_DEFAULT_PARAMS;
  return {
    power:      asBool(raw?.power, d.power),
    gain:       clamp(asNum(raw?.gain, d.gain), -24, 24),
    boost:      clamp(asNum(raw?.boost, d.boost), 0, 100),
    saturation: clamp(asNum(raw?.saturation, d.saturation), 0, 100),
    mix:        clamp(asNum(raw?.mix, d.mix), 0, 100),
    out:        clamp(asNum(raw?.out, d.out), -24, 12),
  };
}

export function serializeNuaDeeParams(
  p: NuaDeeParams,
): Record<string, number | string | boolean> {
  return {
    power: p.power,
    gain: p.gain,
    boost: p.boost,
    saturation: p.saturation,
    mix: p.mix,
    out: p.out,
  };
}

/** drive = 1 + (sat/100) * 4  →  range 1×..5× */
export function satDrive(sat: number): number {
  return 1 + (sat / 100) * 4;
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
