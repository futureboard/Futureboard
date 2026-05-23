import type { InsertDevice, TrackId, TrackPreviewMode, TrackSend } from "../types/daw";
import { audioEngine } from "./AudioEngine";
import { getDspFactory } from "./plugins/dspRegistry";
import type { InsertAudioNode } from "./plugins/types";

const ANALYSER_FFT    = 256;
const ANALYSER_SMOOTH = 0.75;

function analyserSampleBuffer(fftSize: number): Float32Array {
  return new Float32Array(new ArrayBuffer(fftSize * Float32Array.BYTES_PER_ELEMENT));
}

function readAnalyserTimeDomain(analyser: AnalyserNode, dest: Float32Array): void {
  analyser.getFloatTimeDomainData(dest as Float32Array<ArrayBuffer>);
}

export type StereoLevel = { l: number; r: number };

type TrackNodes = {
  input:       GainNode;
  gain:        GainNode;
  insertInput:  GainNode;
  insertOutput: GainNode;
  phaseNode:   GainNode;      // gain = 1 (normal) or -1 (phase inverted)
  panner:      StereoPannerNode;
  previewSplitter: ChannelSplitterNode;
  previewMerger:   ChannelMergerNode;
  previewLToL: GainNode;
  previewLToR: GainNode;
  previewRToL: GainNode;
  previewRToR: GainNode;
  splitter:    ChannelSplitterNode;
  merger:      ChannelMergerNode;
  analyserL:   AnalyserNode;
  analyserR:   AnalyserNode;
  bufL:        Float32Array;
  bufR:        Float32Array;
  spectrum:    Float32Array;
  _userMuted:  boolean;
  muted:       boolean;
  solo:        boolean;
  volume:      number;
  previewMode: TrackPreviewMode;
  insertNodes: Map<string, InsertAudioNode>;
  insertChain: InsertAudioNode[];
  sendNodes:   Map<string, SendRoute>;
};

type SendRoute = {
  gain: GainNode;
  source: AudioNode;
  output: AudioNode;
  targetTrackId: TrackId;
  preFader: boolean;
};

class Mixer {
  private tracks      = new Map<TrackId, TrackNodes>();
  private _outputNodes = new Map<TrackId, AudioNode>();
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

      this.masterGain.connect(this.masterSplitter);
      this.masterSplitter.connect(this.masterAnalyserL, 0);
      this.masterSplitter.connect(this.masterAnalyserR, 1);
      this.masterSplitter.connect(this.masterMerger, 0, 0);
      this.masterSplitter.connect(this.masterMerger, 1, 1);
      this.masterMerger.connect(audioEngine.destination);
      console.log("[WebAudio] master connected");
    }
    return this.masterGain;
  }

  // ── track chain ──────────────────────────────────────────────────────────────

  getOrCreateTrack(trackId: TrackId, volume = 1, pan = 0): TrackNodes {
    if (!this.tracks.has(trackId)) {
      const ctx = audioEngine.ctx;

      const input        = ctx.createGain();
      const gain         = ctx.createGain();
      const insertInput  = ctx.createGain();
      const insertOutput = ctx.createGain();
      const phaseNode    = ctx.createGain();      // value = 1 or -1
      const panner       = ctx.createStereoPanner();
      const previewSplitter = ctx.createChannelSplitter(2);
      const previewMerger   = ctx.createChannelMerger(2);
      const previewLToL = ctx.createGain();
      const previewLToR = ctx.createGain();
      const previewRToL = ctx.createGain();
      const previewRToR = ctx.createGain();
      const splitter     = ctx.createChannelSplitter(2);
      const merger       = ctx.createChannelMerger(2);
      const analyserL    = ctx.createAnalyser();
      const analyserR    = ctx.createAnalyser();

      for (const a of [analyserL, analyserR]) {
        a.fftSize = ANALYSER_FFT;
        a.smoothingTimeConstant = ANALYSER_SMOOTH;
      }

      gain.gain.value      = volume;
      panner.pan.value     = pan;
      phaseNode.gain.value = 1; // normal polarity
      previewLToL.gain.value = 1;
      previewLToR.gain.value = 0;
      previewRToL.gain.value = 0;
      previewRToR.gain.value = 1;

      // input → gain → insertInput → [insert chain] → insertOutput → phaseNode → panner → split → analysers → merger → master
      input.connect(gain);
      gain.connect(insertInput);
      insertInput.connect(insertOutput);
      insertOutput.connect(phaseNode);
      phaseNode.connect(panner);
      panner.connect(previewSplitter);
      previewSplitter.connect(previewLToL, 0);
      previewSplitter.connect(previewLToR, 0);
      previewSplitter.connect(previewRToL, 1);
      previewSplitter.connect(previewRToR, 1);
      previewLToL.connect(previewMerger, 0, 0);
      previewRToL.connect(previewMerger, 0, 0);
      previewLToR.connect(previewMerger, 0, 1);
      previewRToR.connect(previewMerger, 0, 1);
      previewMerger.connect(splitter);
      splitter.connect(analyserL, 0);
      splitter.connect(analyserR, 1);
      splitter.connect(merger, 0, 0);
      splitter.connect(merger, 1, 1);
      merger.connect(this.master);
      this._outputNodes.set(trackId, this.master);

      this.tracks.set(trackId, {
        gain,
        input,
        insertInput,
        insertOutput,
        phaseNode,
        panner,
        previewSplitter,
        previewMerger,
        previewLToL,
        previewLToR,
        previewRToL,
        previewRToR,
        splitter,
        merger,
        analyserL,
        analyserR,
        bufL: analyserSampleBuffer(ANALYSER_FFT),
        bufR: analyserSampleBuffer(ANALYSER_FFT),
        spectrum: new Float32Array(analyserL.frequencyBinCount),
        _userMuted: false,
        muted: false,
        solo: false,
        volume,
        previewMode: "stereo",
        insertNodes: new Map(),
        insertChain: [],
        sendNodes: new Map(),
      });
    }
    return this.tracks.get(trackId)!;
  }

  getTrackInput(trackId: TrackId): GainNode {
    return this.getOrCreateTrack(trackId).input;
  }

  getMasterInput(): GainNode {
    return this.master;
  }

  // ── sends ───────────────────────────────────────────────────────────────────

  syncTrackSends(trackId: TrackId, sends: TrackSend[]): void {
    const nodes = this.getOrCreateTrack(trackId);
    const incomingIds = new Set(sends.map((send) => send.id));

    for (const [sendId, route] of nodes.sendNodes) {
      if (!incomingIds.has(sendId)) {
        this.disconnectSendRoute(route);
        nodes.sendNodes.delete(sendId);
      }
    }

    for (const send of sends) {
      const target = this.tracks.get(send.targetTrackId);
      if (!target || send.targetTrackId === trackId) {
        const existing = nodes.sendNodes.get(send.id);
        if (existing) {
          this.disconnectSendRoute(existing);
          nodes.sendNodes.delete(send.id);
        }
        continue;
      }

      const level = send.enabled === false ? 0 : Math.max(0, send.level ?? 1);
      const preFader = send.preFader === true;
      const source = preFader ? nodes.input : nodes.merger;
      const output = target.gain;
      const existing = nodes.sendNodes.get(send.id);

      if (!existing || existing.source !== source || existing.output !== output || existing.preFader !== preFader) {
        if (existing) this.disconnectSendRoute(existing);
        const gain = audioEngine.ctx.createGain();
        gain.gain.value = level;
        source.connect(gain);
        gain.connect(output);
        nodes.sendNodes.set(send.id, {
          gain,
          source,
          output,
          targetTrackId: send.targetTrackId,
          preFader,
        });
      } else {
        existing.gain.gain.setTargetAtTime(level, audioEngine.currentTime, 0.01);
      }
    }
  }

  // ── insert chain ─────────────────────────────────────────────────────────────

  syncTrackInserts(trackId: TrackId, inserts: InsertDevice[], bpm: number): void {
    const nodes = this.getOrCreateTrack(trackId);
    const ctx   = audioEngine.ctx;
    const now   = ctx.currentTime;

    const sorted = [...inserts].sort((a, b) => a.order - b.order);

    const updateCtx = { now, sampleRate: ctx.sampleRate, bpm };

    // Dispose inserts that are no longer in the list
    const incomingIds = new Set(sorted.map((d) => d.id));
    for (const [id, node] of nodes.insertNodes) {
      if (!incomingIds.has(id)) {
        node.dispose();
        nodes.insertNodes.delete(id);
      }
    }

    // Create or update inserts
    for (const device of sorted) {
      const existing = nodes.insertNodes.get(device.id);
      if (existing) {
        existing.update(device.params, updateCtx);
        existing.setEnabled(device.enabled, now);
      } else {
        const factory = getDspFactory(device);
        if (factory) {
          const insertNode = factory(ctx, device, updateCtx);
          insertNode.setEnabled(device.enabled, now);
          nodes.insertNodes.set(device.id, insertNode);
        }
      }
    }

    // Rebuild the audio chain in sorted order
    this.rebuildInsertChain(nodes, sorted);
  }

  private rebuildInsertChain(nodes: TrackNodes, sorted: InsertDevice[]): void {
    // Tear down old connections between insertInput, chain nodes, and insertOutput
    nodes.insertInput.disconnect();

    for (const node of nodes.insertChain) {
      node.output.disconnect();
    }

    // Gather active insert audio nodes in order
    const chain: InsertAudioNode[] = [];
    for (const device of sorted) {
      const insertNode = nodes.insertNodes.get(device.id);
      if (insertNode) chain.push(insertNode);
    }
    nodes.insertChain = chain;

    if (chain.length === 0) {
      nodes.insertInput.connect(nodes.insertOutput);
    } else {
      nodes.insertInput.connect(chain[0]!.input);
      for (let i = 0; i < chain.length - 1; i++) {
        chain[i]!.output.connect(chain[i + 1]!.input);
      }
      chain[chain.length - 1]!.output.connect(nodes.insertOutput);
    }
  }

  // ── level metering ───────────────────────────────────────────────────────────

  getLevel(trackId: TrackId): StereoLevel {
    const nodes = this.tracks.get(trackId);
    if (!nodes) return { l: 0, r: 0 };
    readAnalyserTimeDomain(nodes.analyserL, nodes.bufL);
    readAnalyserTimeDomain(nodes.analyserR, nodes.bufR);
    return { l: rms(nodes.bufL), r: rms(nodes.bufR) };
  }

  getSpectrum(trackId: TrackId): Float32Array | null {
    const nodes = this.tracks.get(trackId);
    if (!nodes) return null;
    nodes.analyserL.getFloatFrequencyData(nodes.spectrum as Float32Array<ArrayBuffer>);
    return nodes.spectrum;
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
    nodes._userMuted = muted;
    this.applyAllSolo();
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

  /** Invert (or restore) the polarity of a track's output signal. */
  setPhaseInvert(trackId: TrackId, inverted: boolean) {
    const nodes = this.tracks.get(trackId);
    if (!nodes) return;
    const target = inverted ? -1 : 1;
    nodes.phaseNode.gain.setTargetAtTime(target, audioEngine.currentTime, 0.005);
  }

  setPreviewMode(trackId: TrackId, mode: TrackPreviewMode) {
    const nodes = this.tracks.get(trackId);
    if (!nodes) return;
    nodes.previewMode = mode;
    const now = audioEngine.currentTime;
    let lToL = 1;
    let lToR = 0;
    let rToL = 0;
    let rToR = 1;
    if (mode === "mono" || mode === "mid") {
      lToL = 0.5;
      lToR = 0.5;
      rToL = 0.5;
      rToR = 0.5;
    } else if (mode === "side") {
      lToL = 0.5;
      lToR = 0.5;
      rToL = -0.5;
      rToR = -0.5;
    }
    nodes.previewLToL.gain.setTargetAtTime(lToL, now, 0.005);
    nodes.previewLToR.gain.setTargetAtTime(lToR, now, 0.005);
    nodes.previewRToL.gain.setTargetAtTime(rToL, now, 0.005);
    nodes.previewRToR.gain.setTargetAtTime(rToR, now, 0.005);
  }

  removeTrack(trackId: TrackId) {
    const nodes = this.tracks.get(trackId);
    if (nodes) {
      // Dispose all insert nodes
      for (const node of nodes.insertNodes.values()) {
        node.dispose();
      }
      nodes.insertNodes.clear();

      nodes.gain.disconnect();
      nodes.insertInput.disconnect();
      nodes.insertOutput.disconnect();
      nodes.phaseNode.disconnect();
      nodes.panner.disconnect();
      nodes.previewSplitter.disconnect();
      nodes.previewMerger.disconnect();
      nodes.previewLToL.disconnect();
      nodes.previewLToR.disconnect();
      nodes.previewRToL.disconnect();
      nodes.previewRToR.disconnect();
      nodes.splitter.disconnect();
      nodes.analyserL.disconnect();
      nodes.analyserR.disconnect();
      nodes.merger.disconnect();
      for (const route of nodes.sendNodes.values()) {
        this.disconnectSendRoute(route);
      }
      nodes.sendNodes.clear();
      this.tracks.delete(trackId);
      this._outputNodes.delete(trackId);
    }
  }

  /**
   * Re-route a track's output to a different destination.
   * `output` is "master", empty, or a track ID whose gain input acts as the bus.
   */
  setTrackOutput(trackId: TrackId, output: string): void {
    const nodes = this.tracks.get(trackId);
    if (!nodes) return;

    const currentOutput = this._outputNodes.get(trackId);
    if (currentOutput) {
      try { nodes.merger.disconnect(currentOutput); } catch { /* already disconnected */ }
    }

    const newOutput: AudioNode =
      !output || output === "master" || output === "none"
        ? this.master
        : (this.tracks.get(output as TrackId)?.gain ?? this.master);

    nodes.merger.connect(newOutput);
    this._outputNodes.set(trackId, newOutput);
  }

  // ── private ──────────────────────────────────────────────────────────────────

  private applyAllSolo() {
    const anySolo = [...this.tracks.values()].some((n) => n.solo);
    for (const nodes of this.tracks.values()) {
      nodes.muted = nodes._userMuted || (anySolo && !nodes.solo);
      this.recalcGain(nodes);
    }
  }

  private recalcGain(nodes: TrackNodes) {
    const target = nodes.muted ? 0 : nodes.volume;
    nodes.gain.gain.setTargetAtTime(target, audioEngine.currentTime, 0.01);
  }

  private disconnectSendRoute(route: SendRoute): void {
    try { route.source.disconnect(route.gain); } catch { /* already disconnected */ }
    try { route.gain.disconnect(route.output); } catch { /* already disconnected */ }
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
