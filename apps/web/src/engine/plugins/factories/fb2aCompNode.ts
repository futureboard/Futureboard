import {
  colorToDrive,
  normalizeFB2AParams,
  opticalModelFromParams,
  type FB2AParams,
} from "../../../../../../plugins/FB2AComp/Core";
import { dbToGain, smoothParam, equalPowerMix, clamp } from "../audioMath";
import type { InsertAudioNode, InsertNodeFactory } from "../types";

export const createFB2ACompNode: InsertNodeFactory = (audioCtx, device, _updateCtx) => {
  const input = audioCtx.createGain();
  const output = audioCtx.createGain();
  const dryGain = audioCtx.createGain();
  const wetBus = audioCtx.createGain();
  const wetGain = audioCtx.createGain();
  const trim = audioCtx.createGain();

  const linkedComp = audioCtx.createDynamicsCompressor();
  const linkedMakeup = audioCtx.createGain();
  const linkedColor = audioCtx.createWaveShaper();
  const linkedBlend = audioCtx.createGain();

  const splitter = audioCtx.createChannelSplitter(2);
  const compL = audioCtx.createDynamicsCompressor();
  const compR = audioCtx.createDynamicsCompressor();
  const merger = audioCtx.createChannelMerger(2);
  const unlinkedMakeup = audioCtx.createGain();
  const unlinkedColor = audioCtx.createWaveShaper();
  const unlinkedBlend = audioCtx.createGain();

  input.connect(dryGain);
  dryGain.connect(output);

  input.connect(linkedComp);
  linkedComp.connect(linkedMakeup);
  linkedMakeup.connect(linkedColor);
  linkedColor.connect(linkedBlend);
  linkedBlend.connect(wetBus);

  input.connect(splitter);
  splitter.connect(compL, 0);
  splitter.connect(compR, 1);
  compL.connect(merger, 0, 0);
  compR.connect(merger, 0, 1);
  merger.connect(unlinkedMakeup);
  unlinkedMakeup.connect(unlinkedColor);
  unlinkedColor.connect(unlinkedBlend);
  unlinkedBlend.connect(wetBus);

  wetBus.connect(trim);
  trim.connect(wetGain);
  wetGain.connect(output);

  dryGain.gain.value = 0;
  wetGain.gain.value = 1;
  linkedBlend.gain.value = 1;
  unlinkedBlend.gain.value = 0;

  let lastDry = 0;
  let lastWet = 1;
  let lastDrive = -1;

  function setCompressorParams(comp: DynamicsCompressorNode, p: FB2AParams, now: number): void {
    const model = opticalModelFromParams(p);
    smoothParam(comp.threshold, model.thresholdDb, now, 0.025);
    smoothParam(comp.ratio, model.ratio, now, 0.05);
    smoothParam(comp.knee, model.kneeDb, now, 0.05);
    smoothParam(comp.attack, model.attackSec, now, 0.05);
    smoothParam(comp.release, model.releaseSec, now, 0.08);
  }

  function applyParams(params: Record<string, number | string | boolean>): void {
    const m = normalizeFB2AParams(params);
    const now = audioCtx.currentTime;

    setCompressorParams(linkedComp, m, now);
    setCompressorParams(compL, m, now);
    setCompressorParams(compR, m, now);

    const makeup = dbToGain(m.gainDb);
    smoothParam(linkedMakeup.gain, makeup, now);
    smoothParam(unlinkedMakeup.gain, makeup, now);
    smoothParam(trim.gain, dbToGain(m.outputTrimDb), now);

    const drive = colorToDrive(m.color);
    if (Math.abs(drive - lastDrive) > 0.01) {
      const curve = makeSaturationCurve(drive);
      linkedColor.curve = curve;
      unlinkedColor.curve = curve;
      linkedColor.oversample = drive > 0.45 ? "4x" : "2x";
      unlinkedColor.oversample = linkedColor.oversample;
      lastDrive = drive;
    }

    const link = clamp(m.stereoLink / 100, 0, 1);
    smoothParam(linkedBlend.gain, link, now, 0.03);
    smoothParam(unlinkedBlend.gain, 1 - link, now, 0.03);

    const { dry, wet } = equalPowerMix(m.mix);
    lastDry = dry;
    lastWet = wet;
    smoothParam(dryGain.gain, dry, now);
    smoothParam(wetGain.gain, wet, now);
  }

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
      input.disconnect();
      dryGain.disconnect();
      linkedComp.disconnect();
      linkedMakeup.disconnect();
      linkedColor.disconnect();
      linkedBlend.disconnect();
      splitter.disconnect();
      compL.disconnect();
      compR.disconnect();
      merger.disconnect();
      unlinkedMakeup.disconnect();
      unlinkedColor.disconnect();
      unlinkedBlend.disconnect();
      wetBus.disconnect();
      trim.disconnect();
      wetGain.disconnect();
      output.disconnect();
    },
  };

  return node;
};

function makeSaturationCurve(drive: number): Float32Array<ArrayBuffer> {
  const n = 1024;
  const curve = new Float32Array(new ArrayBuffer(n * Float32Array.BYTES_PER_ELEMENT));
  const k = drive * 14;
  for (let i = 0; i < n; i++) {
    const x = (i / (n - 1)) * 2 - 1;
    curve[i] = drive <= 0.001 ? x : Math.tanh(x * (1 + k)) / Math.tanh(1 + k);
  }
  return curve;
}
