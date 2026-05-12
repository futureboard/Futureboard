import type { TrackId } from "../types/daw";
import { audioEngine } from "./AudioEngine";

type TrackNodes = {
  gain: GainNode;
  panner: StereoPannerNode;
  muted: boolean;
  solo: boolean;
  volume: number;
};

class Mixer {
  private tracks = new Map<TrackId, TrackNodes>();
  private masterGain: GainNode | null = null;

  private get master(): GainNode {
    if (!this.masterGain) {
      this.masterGain = audioEngine.ctx.createGain();
      this.masterGain.connect(audioEngine.destination);
    }
    return this.masterGain;
  }

  getOrCreateTrack(trackId: TrackId, volume = 1, pan = 0): TrackNodes {
    if (!this.tracks.has(trackId)) {
      const gain = audioEngine.ctx.createGain();
      const panner = audioEngine.ctx.createStereoPanner();
      gain.gain.value = volume;
      panner.pan.value = pan;
      gain.connect(panner);
      panner.connect(this.master);
      this.tracks.set(trackId, { gain, panner, muted: false, solo: false, volume });
    }
    return this.tracks.get(trackId)!;
  }

  getTrackInput(trackId: TrackId): GainNode {
    return this.getOrCreateTrack(trackId).gain;
  }

  setVolume(trackId: TrackId, value: number) {
    const nodes = this.tracks.get(trackId);
    if (!nodes) return;
    nodes.volume = value;
    if (!nodes.muted) {
      nodes.gain.gain.setTargetAtTime(value, audioEngine.currentTime, 0.01);
    }
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

  setMasterVolume(value: number) {
    this.master.gain.setTargetAtTime(value, audioEngine.currentTime, 0.01);
  }

  removeTrack(trackId: TrackId) {
    const nodes = this.tracks.get(trackId);
    if (nodes) {
      nodes.gain.disconnect();
      nodes.panner.disconnect();
      this.tracks.delete(trackId);
    }
  }
}

export const mixer = new Mixer();
