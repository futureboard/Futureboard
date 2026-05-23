import { normalizeUltraVerbParams, type UltraVerbParams } from "../../../../../../plugins/UltraVerb/Core";
import { dbToGain, smoothParam, equalPowerMix, clamp } from "../audioMath";
import type { InsertAudioNode, InsertNodeFactory } from "../types";

// Generate an impulse response buffer matching reverb params
function buildIR(audioCtx: AudioContext, p: UltraVerbParams): AudioBuffer {
  const sr     = audioCtx.sampleRate;
  const decay  = clamp(p.decay, 0.1, 20);
  const length = Math.ceil(sr * (decay + 0.5));
  const buf    = audioCtx.createBuffer(2, length, sr);

  // Mode-dependent diffusion density
  const densityMap: Record<string, number> = { room: 0.4, plate: 0.85, hall: 0.65, space: 0.3 };
  const density = densityMap[p.mode] ?? 0.65;
  const diffusion = p.diffusion / 100;
  const damping   = p.damping   / 100;

  for (let ch = 0; ch < 2; ch++) {
    const data = buf.getChannelData(ch);
    for (let i = 0; i < length; i++) {
      const t    = i / sr;
      const env  = Math.exp(-t * (3.0 / decay));
      const damp = Math.pow(1 - damping * 0.9, t * 10);
      const r    = (Math.random() * 2 - 1) * (1 - diffusion * (1 - density));
      data[i]    = r * env * damp;
    }
    // Add a brief early-reflection spike at t=0
    data[0] = ch === 0 ? 0.8 : 0.7;
    if (length > 10) {
      const preSamples = Math.round((p.preDelayMs / 1000) * sr);
      if (preSamples > 0 && preSamples < length) {
        data[preSamples] += ch === 0 ? 0.4 : 0.35;
      }
    }
  }

  return buf;
}

export const createUltraVerbNode: InsertNodeFactory = (audioCtx, device, _updateCtx) => {
  const input    = audioCtx.createGain();
  const output   = audioCtx.createGain();
  const dryGain  = audioCtx.createGain();
  const wetGain  = audioCtx.createGain();
  const convolver = audioCtx.createConvolver();
  convolver.normalize = true;

  // Pre-delay
  const preDelay = audioCtx.createDelay(0.5);

  // Output filters
  const hiPass  = audioCtx.createBiquadFilter();
  const loPass  = audioCtx.createBiquadFilter();
  hiPass.type   = "highpass";
  loPass.type   = "lowpass";

  // Makeup
  const makeup  = audioCtx.createGain();
  makeup.gain.value = 1;

  // Routing: input → dryGain → output
  //          input → preDelay → convolver → hiPass → loPass → makeup → wetGain → output
  input.connect(dryGain);
  dryGain.connect(output);

  input.connect(preDelay);
  preDelay.connect(convolver);
  convolver.connect(hiPass);
  hiPass.connect(loPass);
  loPass.connect(makeup);
  makeup.connect(wetGain);
  wetGain.connect(output);

  dryGain.gain.value = 1;
  wetGain.gain.value = 0;

  let lastDry = 1;
  let lastWet = 0;

  let debounceTimer: ReturnType<typeof setTimeout> | null = null;
  let lastIrKey = "";

  function irKey(p: UltraVerbParams): string {
    return `${p.mode}|${p.size.toFixed(0)}|${p.decay.toFixed(2)}|${p.damping.toFixed(0)}|${p.diffusion.toFixed(0)}|${p.preDelayMs.toFixed(0)}`;
  }

  function scheduleIRRebuild(p: UltraVerbParams): void {
    const key = irKey(p);
    if (key === lastIrKey) return;
    lastIrKey = key;
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      convolver.buffer = buildIR(audioCtx, p);
    }, 80);
  }

  function applyParams(params: Record<string, number | string | boolean>): void {
    const m   = normalizeUltraVerbParams(params);
    const now = audioCtx.currentTime;

    scheduleIRRebuild(m);

    preDelay.delayTime.setTargetAtTime(clamp(m.preDelayMs / 1000, 0, 0.5), now, 0.02);

    smoothParam(hiPass.frequency, m.lowCutHz,  now);
    smoothParam(loPass.frequency, m.highCutHz, now);

    smoothParam(makeup.gain, dbToGain(m.outputDb), now);

    const { dry, wet } = equalPowerMix(m.mix);
    lastDry = dry;
    lastWet = wet;
    smoothParam(dryGain.gain, dry, now);
    smoothParam(wetGain.gain, wet, now);
  }

  // Initial IR and params
  const initialModel = normalizeUltraVerbParams(device.params);
  convolver.buffer = buildIR(audioCtx, initialModel);
  lastIrKey = irKey(initialModel);
  applyParams(device.params);

  const node: InsertAudioNode = {
    id: device.id,
    input,
    output,

    update(params) {
      applyParams(params);
    },

    setEnabled(enabled, now) {
      dryGain.gain.setTargetAtTime(enabled ? lastDry : 1, now, 0.015);
      wetGain.gain.setTargetAtTime(enabled ? lastWet : 0, now, 0.015);
    },

    dispose() {
      if (debounceTimer) clearTimeout(debounceTimer);
      input.disconnect();
      dryGain.disconnect();
      preDelay.disconnect();
      convolver.disconnect();
      hiPass.disconnect();
      loPass.disconnect();
      makeup.disconnect();
      wetGain.disconnect();
      output.disconnect();
    },
  };

  return node;
};
