export type Equz8BandType = "highpass" | "lowshelf" | "bell" | "highshelf" | "lowpass";

export type Equz8Band = {
  id: number;
  active: boolean;
  type: Equz8BandType;
  freq: number;
  gain: number;
  q: number;
};

export type Equz8Params = {
  power: boolean;
  selectedBand: number;
  bands: Equz8Band[];
};

export const EQUZ8_DB_RANGE = 18;
export const EQUZ8_FREQ_MIN = 20;
export const EQUZ8_FREQ_MAX = 20_000;

export const EQUZ8_DEFAULT_BANDS: Equz8Band[] = [
  { id: 1, active: true, type: "highpass",  freq: 50,    gain: 0,    q: 0.7 },
  { id: 2, active: true, type: "lowshelf",  freq: 120,   gain: 0,    q: 0.8 },
  { id: 3, active: true, type: "bell",      freq: 250,   gain: 2.5,  q: 1.2 },
  { id: 4, active: true, type: "bell",      freq: 750,   gain: -1.5, q: 1.4 },
  { id: 5, active: true, type: "bell",      freq: 1500,  gain: 1,    q: 1 },
  { id: 6, active: true, type: "bell",      freq: 3500,  gain: 0,    q: 1.1 },
  { id: 7, active: true, type: "highshelf", freq: 8000,  gain: 1.5,  q: 0.8 },
  { id: 8, active: true, type: "lowpass",   freq: 16000, gain: 0,    q: 0.7 },
];

export const EQUZ8_DEFAULT_PARAMS: Equz8Params = {
  power: true,
  selectedBand: 2,
  bands: EQUZ8_DEFAULT_BANDS,
};

export function normalizeEquz8Params(raw: Record<string, number | string | boolean> | undefined): Equz8Params {
  const bands = EQUZ8_DEFAULT_BANDS.map((fallback, i) => {
    const prefix = `band${i + 1}`;
    const type = raw?.[`${prefix}Type`];
    return {
      id: i + 1,
      active: asBool(raw?.[`${prefix}Active`], fallback.active),
      type: isBandType(type) ? type : fallback.type,
      freq: clamp(asNum(raw?.[`${prefix}Freq`], fallback.freq), EQUZ8_FREQ_MIN, EQUZ8_FREQ_MAX),
      gain: clamp(asNum(raw?.[`${prefix}Gain`], fallback.gain), -EQUZ8_DB_RANGE, EQUZ8_DB_RANGE),
      q: clamp(asNum(raw?.[`${prefix}Q`], fallback.q), 0.1, 12),
    };
  });

  return {
    power: asBool(raw?.power, true),
    selectedBand: clamp(Math.round(asNum(raw?.selectedBand, 2)), 0, 7),
    bands,
  };
}

export function serializeEquz8Params(params: Equz8Params): Record<string, number | string | boolean> {
  const out: Record<string, number | string | boolean> = {
    power: params.power,
    selectedBand: params.selectedBand,
  };
  params.bands.forEach((band, i) => {
    const prefix = `band${i + 1}`;
    out[`${prefix}Active`] = band.active;
    out[`${prefix}Type`] = band.type;
    out[`${prefix}Freq`] = band.freq;
    out[`${prefix}Gain`] = band.gain;
    out[`${prefix}Q`] = band.q;
  });
  return out;
}

export function bandContributionDb(band: Equz8Band, freq: number): number {
  switch (band.type) {
    case "bell": {
      const d = Math.log2(freq / band.freq);
      const w = Math.max(0.06, 1 / band.q);
      return band.gain * Math.exp(-(d * d) / (2 * w * w));
    }
    case "lowshelf": {
      const logR = Math.log10(freq / band.freq);
      const hw = clamp(0.55 / Math.sqrt(band.q), 0.07, 1.1);
      return band.gain * (1 - smoothstep(-hw, hw, logR));
    }
    case "highshelf": {
      const logR = Math.log10(freq / band.freq);
      const hw = clamp(0.55 / Math.sqrt(band.q), 0.07, 1.1);
      return band.gain * smoothstep(-hw, hw, logR);
    }
    case "highpass": {
      const ratio = freq / band.freq;
      const r2n = Math.pow(ratio, 4);
      return 20 * Math.log10(Math.sqrt(r2n / (1 + r2n)) + 1e-10);
    }
    case "lowpass": {
      const ratio = freq / band.freq;
      const r2n = Math.pow(ratio, 4);
      return 20 * Math.log10(Math.sqrt(1 / (1 + r2n)) + 1e-10);
    }
    default:
      return 0;
  }
}

export function totalEqGainDb(bands: Equz8Band[], freq: number): number {
  return clamp(
    bands.reduce((sum, band) => sum + (band.active ? bandContributionDb(band, freq) : 0), 0),
    -EQUZ8_DB_RANGE,
    EQUZ8_DB_RANGE,
  );
}

export function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

function asNum(value: number | string | boolean | undefined, fallback: number): number {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function asBool(value: number | string | boolean | undefined, fallback: boolean): boolean {
  return typeof value === "boolean" ? value : fallback;
}

function isBandType(value: number | string | boolean | undefined): value is Equz8BandType {
  return value === "highpass" || value === "lowshelf" || value === "bell" || value === "highshelf" || value === "lowpass";
}

function smoothstep(edge0: number, edge1: number, x: number): number {
  const t = clamp((x - edge0) / (edge1 - edge0), 0, 1);
  return t * t * (3 - 2 * t);
}
