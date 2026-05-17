import type { TimeSignature } from "../types/daw";
import { audioEngine } from "./AudioEngine";
import { mixer } from "./Mixer";
import { transport } from "./Transport";
import { secondsPerBeat, beatsPerBar } from "../utils/musicalTime";

type MetronomeSound = "classic" | "wood" | "digital" | "soft";

export type MetronomeConfig = {
  bpm: number;
  timeSignature: TimeSignature;
  enabled: boolean;
  volume: number;
  accentVolume: number;
  sound: MetronomeSound;
  subdivision?: "quarter" | "eighth" | "sixteenth";
};

const DEFAULT_CONFIG: MetronomeConfig = {
  bpm: 120,
  timeSignature: { numerator: 4, denominator: 4 },
  enabled: false,
  volume: 0.8,
  accentVolume: 1.0,
  sound: "digital",
};

class MetronomeScheduler {
  private intervalId: number | null = null;
  private nextNoteTime: number = 0;
  private currentBeat: number = 0;
  private configGetter: (() => MetronomeConfig) | null = null;
  /**
   * Optional override for the canonical project-time source.
   * When the native Rust engine is active, inject `() => activeAudioEngine.projectTime`
   * so the beat grid stays in sync with the native transport position instead of
   * reading the WebAudio Transport (which is paused in native mode).
   */
  private projectTimeGetter: (() => number) | null = null;

  setConfigGetter(fn: () => MetronomeConfig): void {
    this.configGetter = fn;
  }

  /** Override the default `transport.projectTime` source. */
  setProjectTimeGetter(fn: () => number): void {
    this.projectTimeGetter = fn;
  }

  private getConfig(): MetronomeConfig {
    return this.configGetter?.() ?? DEFAULT_CONFIG;
  }

  private getProjectTime(): number {
    if (this.projectTimeGetter) return this.projectTimeGetter();
    return transport.projectTime;
  }

  start() {
    this.stop();
    this.sync();
    this.intervalId = window.setInterval(() => this.scheduleNextNotes(), 25);
  }

  stop() {
    if (this.intervalId !== null) {
      window.clearInterval(this.intervalId);
      this.intervalId = null;
    }
  }

  seek() {
    if (this.intervalId !== null) {
      this.sync();
    }
  }

  private sync() {
    const { bpm } = this.getConfig();
    const spb = secondsPerBeat(bpm);
    const pTime = this.getProjectTime();

    // Find the next beat boundary on or after the playhead time
    this.currentBeat = Math.ceil(pTime / spb);

    // The exact project time of the next beat
    const nextBeatProjectTime = this.currentBeat * spb;
    const beatOffset = nextBeatProjectTime - pTime;

    this.nextNoteTime = audioEngine.currentTime + beatOffset;
    // reset sub-beat counter
    this.currentSubdivision = 0;
  }

  private currentSubdivision: number = 0;

  private scheduleNextNotes() {
    const { enabled, volume, accentVolume, sound, bpm, timeSignature, subdivision } = this.getConfig();
    const spb = secondsPerBeat(bpm);
    const bpb = beatsPerBar(timeSignature);
    
    let subsPerBeat = 1;
    if (subdivision === "eighth") subsPerBeat = 2;
    if (subdivision === "sixteenth") subsPerBeat = 4;
    
    const stepTime = spb / subsPerBeat;

    const lookahead = 0.1; // 100ms
    while (this.nextNoteTime < audioEngine.currentTime + lookahead) {
      if (enabled) {
        const isAccent = this.currentSubdivision === 0 && (this.currentBeat % bpb) === 0;
        const isBeat = this.currentSubdivision === 0;
        
        // Slightly lower volume for sub-beats
        const finalVolume = isBeat ? volume : volume * 0.6;
        const finalAccent = isAccent ? accentVolume : finalVolume;
        
        this.scheduleClick(isAccent, this.nextNoteTime, finalVolume, finalAccent, sound);
      }
      this.nextNoteTime += stepTime;
      this.currentSubdivision++;
      if (this.currentSubdivision >= subsPerBeat) {
        this.currentSubdivision = 0;
        this.currentBeat++;
      }
    }
  }

  private scheduleClick(
    isAccent: boolean,
    time: number,
    volume: number,
    accentVolume: number,
    sound: MetronomeSound
  ) {
    const ctx = audioEngine.ctx;
    const osc = ctx.createOscillator();
    const env = ctx.createGain();
    const panner = ctx.createStereoPanner();

    let highFreq = 1200;
    let lowFreq = 800;
    let type: OscillatorType = "sine";
    let decay = 0.05;

    switch (sound) {
      case "classic":
        type = "square";
        highFreq = 1500;
        lowFreq = 1000;
        decay = 0.03;
        break;
      case "wood":
        type = "triangle";
        highFreq = 800;
        lowFreq = 600;
        decay = 0.04;
        break;
      case "digital":
        type = "sine";
        highFreq = 1200;
        lowFreq = 800;
        decay = 0.05;
        break;
      case "soft":
        type = "sine";
        highFreq = 600;
        lowFreq = 400;
        decay = 0.08;
        break;
    }

    osc.frequency.value = isAccent ? highFreq : lowFreq;
    osc.type = type;

    const baseVol = (isAccent ? accentVolume : volume) * 0.5;
    const scheduledTime = Math.max(ctx.currentTime, time);

    env.gain.value = 0;
    env.gain.setValueAtTime(baseVol, scheduledTime);
    env.gain.exponentialRampToValueAtTime(0.001, scheduledTime + decay);

    // Force stereo output so it shows on both L/R master meters
    panner.pan.value = 0;

    osc.connect(env);
    env.connect(panner);
    panner.connect(mixer.getMasterInput());

    osc.start(scheduledTime);
    osc.stop(scheduledTime + decay);
  }
}

export const metronomeScheduler = new MetronomeScheduler();
