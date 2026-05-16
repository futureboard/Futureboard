import { normalizeEquz8Params } from "../../../../../../plugins/Equz8/Core";
import { dbToGain, smoothParam } from "../audioMath";
import type { InsertAudioNode, InsertNodeFactory, InsertUpdateContext } from "../types";

const BIQUAD_TYPE: Record<string, BiquadFilterType> = {
  highpass:  "highpass",
  lowshelf:  "lowshelf",
  bell:      "peaking",
  notch:     "notch",
  highshelf: "highshelf",
  lowpass:   "lowpass",
};

// Neutral filter settings when a band is disabled
const NEUTRAL: Record<string, { freq?: number; gain: number; q?: number }> = {
  highpass:  { freq: 20,    gain: 0, q: 0.707 },
  lowshelf:  { freq: 20,    gain: 0, q: 0.707 },
  peaking:   { freq: 1000,  gain: 0, q: 1 },
  notch:     { freq: 1000,  gain: 0, q: 1 },
  highshelf: { freq: 20000, gain: 0, q: 0.707 },
  lowpass:   { freq: 20000, gain: 0, q: 0.707 },
};

export const createEquz8Node: InsertNodeFactory = (audioCtx, device, updateCtx) => {
  const inputGain  = audioCtx.createGain();
  const dryGain    = audioCtx.createGain();
  const wetGain    = audioCtx.createGain();
  const outputGain = audioCtx.createGain();

  const filters: BiquadFilterNode[] = Array.from({ length: 8 }, () => {
    const f = audioCtx.createBiquadFilter();
    f.type = "peaking";
    f.gain.value = 0;
    return f;
  });

  // Internal routing
  inputGain.connect(dryGain);
  inputGain.connect(filters[0]);
  for (let i = 0; i < 7; i++) filters[i].connect(filters[i + 1]);
  filters[7].connect(wetGain);
  dryGain.connect(outputGain);
  wetGain.connect(outputGain);

  // Enabled state: wet=1, dry=0
  dryGain.gain.value = 0;
  wetGain.gain.value = 1;

  function applyParams(params: Record<string, number | string | boolean>, ctx: InsertUpdateContext): void {
    const model = normalizeEquz8Params(params);
    const now = ctx.now;

    smoothParam(outputGain.gain, dbToGain(model.outputDb), now);

    model.bands.forEach((band, i) => {
      const f = filters[i];
      const btype = BIQUAD_TYPE[band.type] ?? "peaking";
      if (f.type !== btype) f.type = btype;

      if (band.active) {
        smoothParam(f.frequency, Math.max(20, Math.min(20000, band.freq)), now);
        smoothParam(f.Q, Math.max(0.1, Math.min(100, band.q)), now);
        if (btype === "peaking" || btype === "lowshelf" || btype === "highshelf") {
          smoothParam(f.gain, Math.max(-24, Math.min(24, band.gain)), now);
        }
      } else {
        const n = NEUTRAL[btype] ?? NEUTRAL.peaking;
        if (n.freq !== undefined) smoothParam(f.frequency, n.freq, now);
        if (n.q !== undefined) smoothParam(f.Q, n.q, now);
        smoothParam(f.gain, 0, now);
      }
    });
  }

  // Apply initial params
  applyParams(device.params, updateCtx);

  const node: InsertAudioNode = {
    id: device.id,
    input: inputGain,
    output: outputGain,

    update(params, ctx) {
      applyParams(params, ctx);
    },

    setEnabled(enabled, now) {
      dryGain.gain.setTargetAtTime(enabled ? 0 : 1, now, 0.015);
      wetGain.gain.setTargetAtTime(enabled ? 1 : 0, now, 0.015);
    },

    dispose() {
      inputGain.disconnect();
      dryGain.disconnect();
      filters.forEach((f) => f.disconnect());
      wetGain.disconnect();
      outputGain.disconnect();
    },
  };

  return node;
};
