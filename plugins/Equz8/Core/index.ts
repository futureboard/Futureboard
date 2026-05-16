export type Equz8BandType = "highpass" | "lowshelf" | "bell" | "notch" | "highshelf" | "lowpass";

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
  analyzer: boolean;
  outputDb: number;
  selectedBand: number;
  bands: Equz8Band[];
};

export const EQUZ8_DB_RANGE = 18;
export const EQUZ8_FREQ_MIN = 20;
export const EQUZ8_FREQ_MAX = 20_000;
export const EQUZ8_OUTPUT_DB_MIN = -24;
export const EQUZ8_OUTPUT_DB_MAX = 12;
export const EQUZ8_UI_SAMPLE_RATE = 48_000;

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
  analyzer: true,
  outputDb: 0,
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
    analyzer: asBool(raw?.analyzer, true),
    outputDb: clamp(asNum(raw?.outputDb, 0), EQUZ8_OUTPUT_DB_MIN, EQUZ8_OUTPUT_DB_MAX),
    selectedBand: clamp(Math.round(asNum(raw?.selectedBand, 2)), 0, 7),
    bands,
  };
}

export function serializeEquz8Params(params: Equz8Params): Record<string, number | string | boolean> {
  const out: Record<string, number | string | boolean> = {
    power: params.power,
    analyzer: params.analyzer,
    outputDb: clamp(params.outputDb, EQUZ8_OUTPUT_DB_MIN, EQUZ8_OUTPUT_DB_MAX),
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
    case "bell":
      return computeBellResponse(freq, band.freq, band.gain, band.q);
    case "lowshelf":
    case "highshelf":
      return computeShelfResponse(freq, band.freq, band.gain, band.q, band.type);
    case "highpass":
    case "lowpass":
    case "notch":
      return computePassResponse(freq, band.freq, band.q, band.type);
    default:
      return 0;
  }
}

export function totalEqGainDb(bands: Equz8Band[], freq: number): number {
  return sumBandResponses(bands, freq);
}

export function sumBandResponses(bands: Equz8Band[], freq: number): number {
  return clamp(
    bands.reduce((sum, band) => sum + (band.active ? bandContributionDb(band, freq) : 0), 0),
    -EQUZ8_DB_RANGE,
    EQUZ8_DB_RANGE,
  );
}

export function computeBellResponse(freq: number, centerFreq: number, gainDb: number, q: number): number {
  return biquadMagnitudeDb(freq, makeBiquad("bell", centerFreq, q, gainDb));
}

export function computeShelfResponse(
  freq: number,
  shelfFreq: number,
  gainDb: number,
  q: number,
  type: "lowshelf" | "highshelf",
): number {
  return biquadMagnitudeDb(freq, makeBiquad(type, shelfFreq, q, gainDb));
}

export function computePassResponse(
  freq: number,
  cutoffFreq: number,
  q: number,
  type: "highpass" | "lowpass" | "notch",
): number {
  return biquadMagnitudeDb(freq, makeBiquad(type, cutoffFreq, q, 0));
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
  return value === "highpass" || value === "lowshelf" || value === "bell" || value === "notch" || value === "highshelf" || value === "lowpass";
}

type Coefficients = {
  b0: number;
  b1: number;
  b2: number;
  a0: number;
  a1: number;
  a2: number;
};

function makeBiquad(type: Equz8BandType, freq: number, q: number, gainDb: number): Coefficients {
  const f0 = clamp(freq, EQUZ8_FREQ_MIN, EQUZ8_FREQ_MAX);
  const safeQ = clamp(q, 0.1, 12);
  const w0 = (2 * Math.PI * Math.min(f0, EQUZ8_UI_SAMPLE_RATE * 0.48)) / EQUZ8_UI_SAMPLE_RATE;
  const cos = Math.cos(w0);
  const sin = Math.sin(w0);
  const alpha = sin / (2 * safeQ);
  const a = Math.pow(10, gainDb / 40);

  switch (type) {
    case "bell":
      return {
        b0: 1 + alpha * a,
        b1: -2 * cos,
        b2: 1 - alpha * a,
        a0: 1 + alpha / a,
        a1: -2 * cos,
        a2: 1 - alpha / a,
      };
    case "notch":
      return {
        b0: 1,
        b1: -2 * cos,
        b2: 1,
        a0: 1 + alpha,
        a1: -2 * cos,
        a2: 1 - alpha,
      };
    case "lowpass":
      return {
        b0: (1 - cos) / 2,
        b1: 1 - cos,
        b2: (1 - cos) / 2,
        a0: 1 + alpha,
        a1: -2 * cos,
        a2: 1 - alpha,
      };
    case "highpass":
      return {
        b0: (1 + cos) / 2,
        b1: -(1 + cos),
        b2: (1 + cos) / 2,
        a0: 1 + alpha,
        a1: -2 * cos,
        a2: 1 - alpha,
      };
    case "lowshelf":
    case "highshelf":
      return makeShelf(type, w0, cos, sin, a, safeQ);
    default:
      return { b0: 1, b1: 0, b2: 0, a0: 1, a1: 0, a2: 0 };
  }
}

function makeShelf(
  type: "lowshelf" | "highshelf",
  _w0: number,
  cos: number,
  sin: number,
  a: number,
  q: number,
): Coefficients {
  const slope = clamp(q, 0.1, 1);
  const alpha = (sin / 2) * Math.sqrt(Math.max(0.0001, (a + 1 / a) * (1 / slope - 1) + 2));
  const beta = 2 * Math.sqrt(a) * alpha;

  if (type === "lowshelf") {
    return {
      b0: a * ((a + 1) - (a - 1) * cos + beta),
      b1: 2 * a * ((a - 1) - (a + 1) * cos),
      b2: a * ((a + 1) - (a - 1) * cos - beta),
      a0: (a + 1) + (a - 1) * cos + beta,
      a1: -2 * ((a - 1) + (a + 1) * cos),
      a2: (a + 1) + (a - 1) * cos - beta,
    };
  }

  return {
    b0: a * ((a + 1) + (a - 1) * cos + beta),
    b1: -2 * a * ((a - 1) + (a + 1) * cos),
    b2: a * ((a + 1) + (a - 1) * cos - beta),
    a0: (a + 1) - (a - 1) * cos + beta,
    a1: 2 * ((a - 1) - (a + 1) * cos),
    a2: (a + 1) - (a - 1) * cos - beta,
  };
}

function biquadMagnitudeDb(freq: number, c: Coefficients): number {
  const f = clamp(freq, EQUZ8_FREQ_MIN, EQUZ8_FREQ_MAX);
  const w = (2 * Math.PI * Math.min(f, EQUZ8_UI_SAMPLE_RATE * 0.48)) / EQUZ8_UI_SAMPLE_RATE;
  const cos1 = Math.cos(w);
  const sin1 = Math.sin(w);
  const cos2 = Math.cos(2 * w);
  const sin2 = Math.sin(2 * w);

  const nr = c.b0 + c.b1 * cos1 + c.b2 * cos2;
  const ni = -c.b1 * sin1 - c.b2 * sin2;
  const dr = c.a0 + c.a1 * cos1 + c.a2 * cos2;
  const di = -c.a1 * sin1 - c.a2 * sin2;
  const num = nr * nr + ni * ni;
  const den = Math.max(1e-20, dr * dr + di * di);
  return clamp(10 * Math.log10(Math.max(1e-20, num / den)), -96, 48);
}
