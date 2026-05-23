import { normalizeUltraDelayParams, divisionToMs } from "../../../../../../plugins/UltraDelay/Core";
import { dbToGain, smoothParam, equalPowerMix, clamp } from "../audioMath";
import type { InsertAudioNode, InsertNodeFactory, InsertUpdateContext } from "../types";

export const createUltraDelayNode: InsertNodeFactory = (audioCtx, device, updateCtx) => {
  const input  = audioCtx.createGain();
  const output = audioCtx.createGain();
  const dryGain = audioCtx.createGain();
  const wetGain = audioCtx.createGain();

  // Stereo split
  const splitter = audioCtx.createChannelSplitter(2);
  const merger   = audioCtx.createChannelMerger(2);

  // Delay nodes (max 4s)
  const delayL = audioCtx.createDelay(4.0);
  const delayR = audioCtx.createDelay(4.0);

  // Feedback gains
  const fbL = audioCtx.createGain();
  const fbR = audioCtx.createGain();

  // Cross-feedback gains (for ping-pong)
  const xfbL = audioCtx.createGain(); // R -> L cross
  const xfbR = audioCtx.createGain(); // L -> R cross

  // Filters in feedback path
  const hpL = audioCtx.createBiquadFilter();
  const hpR = audioCtx.createBiquadFilter();
  const lpL = audioCtx.createBiquadFilter();
  const lpR = audioCtx.createBiquadFilter();

  hpL.type = hpR.type = "highpass";
  lpL.type = lpR.type = "lowpass";

  // Output gain (makeup)
  const makeupGain = audioCtx.createGain();
  makeupGain.gain.value = 1;

  // Routing:
  // input → splitter → delayL/R → filter chain → fbL/R (loop) + cross → merger → wetGain → output
  // input → dryGain → output

  input.connect(dryGain);
  dryGain.connect(output);

  input.connect(splitter);

  // L channel: splitter[0] → delayL → hpL → lpL → fbL (loop back to delayL) + merger[0]
  splitter.connect(delayL, 0);
  delayL.connect(hpL);
  hpL.connect(lpL);
  // lpL feeds back into delayL via fbL
  lpL.connect(fbL);
  fbL.connect(delayL);
  // lpL cross-feeds into delayR via xfbR
  lpL.connect(xfbR);
  xfbR.connect(delayR);
  // lpL goes to merger left channel
  lpL.connect(merger, 0, 0);

  // R channel: splitter[1] → delayR → hpR → lpR → fbR (loop back to delayR) + merger[1]
  splitter.connect(delayR, 1);
  delayR.connect(hpR);
  hpR.connect(lpR);
  lpR.connect(fbR);
  fbR.connect(delayR);
  // lpR cross-feeds into delayL via xfbL
  lpR.connect(xfbL);
  xfbL.connect(delayL);
  // lpR goes to merger right channel
  lpR.connect(merger, 0, 1);

  merger.connect(makeupGain);
  makeupGain.connect(wetGain);
  wetGain.connect(output);

  dryGain.gain.value = 1;
  wetGain.gain.value = 0;

  let lastDry = 1;
  let lastWet = 0;

  function applyParams(params: Record<string, number | string | boolean>, ctx: InsertUpdateContext): void {
    const m   = normalizeUltraDelayParams(params);
    const now = ctx.now;
    const bpm = ctx.bpm;

    const timeLMs = m.sync ? divisionToMs(m.timeL, bpm) : m.timeMsL;
    const timeRMs = (m.sync && !m.link) ? divisionToMs(m.timeR, bpm) : (m.link ? timeLMs : m.timeMsR);

    delayL.delayTime.setTargetAtTime(clamp(timeLMs / 1000, 0, 4), now, 0.02);
    delayR.delayTime.setTargetAtTime(clamp(timeRMs / 1000, 0, 4), now, 0.02);

    const fbGain = m.feedback / 100;

    if (m.mode === "pingpong") {
      // Direct feedback muted, cross-feedback carries the ping-pong
      smoothParam(fbL.gain, 0, now);
      smoothParam(fbR.gain, 0, now);
      smoothParam(xfbL.gain, fbGain, now);
      smoothParam(xfbR.gain, fbGain, now);
    } else if (m.mode === "dual") {
      // Independent L and R, no cross
      smoothParam(fbL.gain, fbGain, now);
      smoothParam(fbR.gain, fbGain, now);
      smoothParam(xfbL.gain, 0, now);
      smoothParam(xfbR.gain, 0, now);
    } else {
      // stereo / mono
      const xgain = (m.crossFeedback / 100) * fbGain * 0.5;
      smoothParam(fbL.gain, fbGain * 0.8, now);
      smoothParam(fbR.gain, fbGain * 0.8, now);
      smoothParam(xfbL.gain, xgain, now);
      smoothParam(xfbR.gain, xgain, now);
    }

    smoothParam(hpL.frequency, m.lowCutHz, now);
    smoothParam(hpR.frequency, m.lowCutHz, now);
    smoothParam(lpL.frequency, m.highCutHz, now);
    smoothParam(lpR.frequency, m.highCutHz, now);

    smoothParam(makeupGain.gain, dbToGain(m.outputDb), now);

    const { dry, wet } = equalPowerMix(m.mix);
    lastDry = dry;
    lastWet = wet;
    smoothParam(dryGain.gain, dry, now);
    smoothParam(wetGain.gain, wet, now);
  }

  applyParams(device.params, updateCtx);

  const node: InsertAudioNode = {
    id: device.id,
    input,
    output,

    update(params, ctx) {
      applyParams(params, ctx);
    },

    setEnabled(enabled, now) {
      dryGain.gain.setTargetAtTime(enabled ? lastDry : 1, now, 0.015);
      wetGain.gain.setTargetAtTime(enabled ? lastWet : 0, now, 0.015);
    },

    dispose() {
      input.disconnect();
      dryGain.disconnect();
      splitter.disconnect();
      delayL.disconnect();
      delayR.disconnect();
      hpL.disconnect();
      hpR.disconnect();
      lpL.disconnect();
      lpR.disconnect();
      fbL.disconnect();
      fbR.disconnect();
      xfbL.disconnect();
      xfbR.disconnect();
      merger.disconnect();
      makeupGain.disconnect();
      wetGain.disconnect();
      output.disconnect();
    },
  };

  return node;
};
