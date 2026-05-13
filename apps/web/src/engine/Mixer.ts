import type { TrackId } from "../types/daw";
import { audioEngine } from "./AudioEngine";

const ANALYSER_FFT  = 256;    // time-domain window size
const ANALYSER_SMOOTH = 0.75; // exponential smoothing (0=instant, 1=frozen)

function analyserSampleBuffer(fftSize: number): Float32Array {
  return new Float32Array(new ArrayBuffer(fftSize * Float32Array.BYTES_PER_ELEMENT));
}

/** DOM lib expects `Float32Array<ArrayBuffer>`; runtime buffer is a normal `ArrayBuffer`. */
function readAnalyserTimeDomain(analyser: AnalyserNode, dest: Float32Array): void {
  analyser.getFloatTimeDomainData(dest as Float32Array<ArrayBuffer>);
}

export type StereoLevel = { l: number; r: number };

type TrackNodes = {
  gain:      GainNode;
  panner:    StereoPannerNode;
  splitter:  ChannelSplitterNode;
  merger:    ChannelMergerNode;
  analyserL: AnalyserNode;
  analyserR: AnalyserNode;
  bufL:      Float32Array;
  bufR:      Float32Array;
  muted:     boolean;
  solo:      boolean;
  volume:    number;
};

class Mixer {
  private tracks     = new Map<TrackId, TrackNodes>();
  private masterGain:      GainNode            | null = null;
  private masterSplitter:  ChannelSplitterNode | null = null;
  private masterMerger:    ChannelMergerNode   | null = null;
  private masterAnalyserL: AnalyserNode        | null = null;
  private masterAnalyserR: AnalyserNode        | null = null;
  private masterBufL:      Float32Array        | null = null;
  private masterBufR:      Float32Array        | null = null;

  // ── master chain ─────────────────────────────────────────────────────────────

  private get master(): GainNode {
    if (!this.masterGain) {
      const ctx = audioEngine.ctx;

      this.masterGain      = ctx.createGain();
      this.masterSplitter  = ctx.createChannelSplitter(2);
      this.masterMerger    = ctx.createChannelMerger(2);
      this.masterAnalyserL = ctx.createAnalyser();
      this.masterAnalyserR = ctx.createAnalyser();

      for (const a of [this.masterAnalyserL, this.masterAnalyserR]) {
        a.fftSize = ANALYSER_FFT;
        a.smoothingTimeConstant = ANALYSER_SMOOTH;
      }

      this.masterBufL = analyserSampleBuffer(ANALYSER_FFT);
      this.masterBufR = analyserSampleBuffer(ANALYSER_FFT);

      // master: gain → split → (tap L/R meters + rebuild stereo) → destination
      this.masterGain.connect(this.masterSplitter);
      this.masterSplitter.connect(this.masterAnalyserL, 0);
      this.masterSplitter.connect(this.masterAnalyserR, 1);
      this.masterSplitter.connect(this.masterMerger, 0, 0);
      this.masterSplitter.connect(this.masterMerger, 1, 1);
      this.masterMerger.connect(audioEngine.destination);
    }
    return this.masterGain;
  }

  // ── track chain ──────────────────────────────────────────────────────────────

  getOrCreateTrack(trackId: TrackId, volume = 1, pan = 0): TrackNodes {
    if (!this.tracks.has(trackId)) {
      const ctx = audioEngine.ctx;

      const gain      = ctx.createGain();
      const panner    = ctx.createStereoPanner();
      const splitter  = ctx.createChannelSplitter(2);
      const merger    = ctx.createChannelMerger(2);
      const analyserL = ctx.createAnalyser();
      const analyserR = ctx.createAnalyser();

      for (const a of [analyserL, analyserR]) {
        a.fftSize = ANALYSER_FFT;
        a.smoothingTimeConstant = ANALYSER_SMOOTH;
      }

      gain.gain.value  = volume;
      panner.pan.value = pan;

      // gain → panner → split → (L/R analysers + merger) → master
      gain.connect(panner);
      panner.connect(splitter);
      splitter.connect(analyserL, 0);
      splitter.connect(analyserR, 1);
      splitter.connect(merger, 0, 0);
      splitter.connect(merger, 1, 1);
      merger.connect(this.master);

      this.tracks.set(trackId, {
        gain,
        panner,
        splitter,
        merger,
        analyserL,
        analyserR,
        bufL: analyserSampleBuffer(ANALYSER_FFT),
        bufR: analyserSampleBuffer(ANALYSER_FFT),
        muted: false,
        solo: false,
        volume,
      });
    }
    return this.tracks.get(trackId)!;
  }

  getTrackInput(trackId: TrackId): GainNode {
    return this.getOrCreateTrack(trackId).gain;
  }

  // ── level metering ───────────────────────────────────────────────────────────

  /** Per-channel RMS (0–1) after panner. */
  getLevel(trackId: TrackId): StereoLevel {
    const nodes = this.tracks.get(trackId);
    if (!nodes) return { l: 0, r: 0 };
    readAnalyserTimeDomain(nodes.analyserL, nodes.bufL);
    readAnalyserTimeDomain(nodes.analyserR, nodes.bufR);
    return { l: rms(nodes.bufL), r: rms(nodes.bufR) };
  }

  getMasterLevel(): StereoLevel {
    if (!this.masterAnalyserL || !this.masterAnalyserR || !this.masterBufL || !this.masterBufR) {
      return { l: 0, r: 0 };
    }
    readAnalyserTimeDomain(this.masterAnalyserL, this.masterBufL);
    readAnalyserTimeDomain(this.masterAnalyserR, this.masterBufR);
    return { l: rms(this.masterBufL), r: rms(this.masterBufR) };
  }

  // ── control ──────────────────────────────────────────────────────────────────

  setVolume(trackId: TrackId, value: number) {
    const nodes = this.tracks.get(trackId);
    if (!nodes) return;
    nodes.volume = value;
    if (!nodes.muted) nodes.gain.gain.setTargetAtTime(value, audioEngine.currentTime, 0.01);
  }

  setPan(trackId: TrackId, value: number) {
    const nodes = this.tracks.get(trackId);
    if (nodes) nodes.panner.pan.setTargetAtTime(value, audioEngine.currentTime, 0.01);
  }

  setMute(trackId: TrackId, muted: boolean) {
    const nodes = this.tracks.get(trackId);
    if (!nodes) return;
    nodes.muted = muted;
    this.recalcGain(nodes);
  }

  setSolo(trackId: TrackId, solo: boolean) {
    const nodes = this.tracks.get(trackId);
    if (!nodes) return;
    nodes.solo = solo;
    this.applyAllSolo();
  }

  setMasterVolume(value: number) {
    this.master.gain.setTargetAtTime(value, audioEngine.currentTime, 0.01);
  }

  removeTrack(trackId: TrackId) {
    const nodes = this.tracks.get(trackId);
    if (nodes) {
      nodes.gain.disconnect();
      nodes.panner.disconnect();
      nodes.splitter.disconnect();
      nodes.analyserL.disconnect();
      nodes.analyserR.disconnect();
      nodes.merger.disconnect();
      this.tracks.delete(trackId);
    }
  }

  // ── private ──────────────────────────────────────────────────────────────────

  private applyAllSolo() {
    const anySolo = [...this.tracks.values()].some((n) => n.solo);
    for (const nodes of this.tracks.values()) {
      nodes.muted = anySolo && !nodes.solo;
      this.recalcGain(nodes);
    }
  }

  private recalcGain(nodes: TrackNodes) {
    const target = nodes.muted ? 0 : nodes.volume;
    nodes.gain.gain.setTargetAtTime(target, audioEngine.currentTime, 0.01);
  }
}

function rms(buf: ArrayLike<number>): number {
  const n = buf.length;
  let sum = 0;
  for (let i = 0; i < n; i++) {
    const x = buf[i]!;
    sum += x * x;
  }
  return Math.sqrt(sum / n);
}

export const mixer = new Mixer();
