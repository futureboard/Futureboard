/**
 * WebAudioEngineAdapter — wraps the existing Transport/Mixer/ClipScheduler
 * singletons behind the AudioEngineAdapter interface.
 *
 * Future: swap this for a WASM or native DSP adapter without touching UI code.
 */
import type { DawProject, DawTrack, DawClip, InsertDevice, TrackId, TrackPreviewMode } from "../types/daw";
import type { AudioEngineAdapter, AudioEngineStatus, AudioSelfTestResult, MeterCallback, TransportCallback } from "./AudioEngineAdapter";
import type { StereoLevel } from "./Mixer";
import { audioEngine } from "./AudioEngine";
import { transport } from "./Transport";
import { mixer } from "./Mixer";
import { clipScheduler } from "./ClipScheduler";
import { WasmAudioEngineAdapter } from "./WasmAudioEngineAdapter";
import { useAudioBackendStore } from "../store/audioBackendStore";
import { getVisualFrameIntervalMs, shouldRunVisualFrame } from "../utils/visualFrameRate";

class WebAudioEngineAdapter implements AudioEngineAdapter {
  private _meterCallbacks = new Set<MeterCallback>();
  private _transportCallbacks = new Set<TransportCallback>();
  private _meterRafId: number | null = null;
  private _transportRafId: number | null = null;
  private _initialized = false;
  private _wasmAdapter: WasmAudioEngineAdapter | null = null;
  private _wasmReady = false;
  private _trackIds: TrackId[] = [];

  // ── Lifecycle ──────────────────────────────────────────────────────────────

  async init(): Promise<void> {
    if (this._initialized) return;

    // Load the Rust WASM/AudioWorklet DSP sidecar, but keep clip playback on
    // the proven WebAudio scheduler until the worklet renders project clips.
    try {
      this._wasmAdapter = new WasmAudioEngineAdapter({ connectOutput: false });
      await this._wasmAdapter.init();
      this._wasmReady = true;
      useAudioBackendStore.getState().setAvailability({ rustWasm: true });
      console.log("[AudioEngine] Playback backend: web-audio");
      console.log("[AudioEngine] DSP backend: rust-wasm");
      console.log("[AudioEngine] Waveform backend: worker");
    } catch (e) {
      console.warn("[AudioEngine] WASM core failed to load, using WebAudio DSP fallback:", e);
      this._wasmAdapter = null;
      this._wasmReady = false;
      useAudioBackendStore.getState().setAvailability({ rustWasm: false });
      console.log("[AudioEngine] Playback backend: web-audio");
      console.log("[AudioEngine] DSP backend: web-audio fallback");
      console.log("[AudioEngine] Waveform backend: worker");
    }

    this._startMeterLoop();
    this._startTransportLoop();
    
    this._initialized = true;
  }

  dispose(): void {
    if (this._wasmAdapter) {
      this._wasmAdapter.dispose();
    }
    if (this._meterRafId !== null) cancelAnimationFrame(this._meterRafId);
    if (this._transportRafId !== null) cancelAnimationFrame(this._transportRafId);
    this._meterCallbacks.clear();
    this._transportCallbacks.clear();
    this._initialized = false;
  }

  getStatus(): AudioEngineStatus {
    const ctx = audioEngine.ctx;
    if (!ctx) return "uninitialized";
    switch (ctx.state) {
      case "running":   return "running";
      case "suspended": return "suspended";
      case "closed":    return "closed";
      default:          return "error";
    }
  }

  async runSelfTest(): Promise<AudioSelfTestResult> {
    try {
      await audioEngine.resume();
      const ctx = audioEngine.ctx;
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.type = "sine";
      osc.frequency.value = 440;
      gain.gain.setValueAtTime(0.0001, ctx.currentTime);
      gain.gain.exponentialRampToValueAtTime(0.12, ctx.currentTime + 0.015);
      gain.gain.exponentialRampToValueAtTime(0.0001, ctx.currentTime + 0.25);
      osc.connect(gain);
      gain.connect(mixer.getMasterInput());
      osc.start();
      osc.stop(ctx.currentTime + 0.26);
      return {
        ok: true,
        backend: this._wasmReady ? "web-audio + rust-wasm" : "web-audio",
        contextState: ctx.state,
        device: "browser destination",
      };
    } catch (error) {
      return {
        ok: false,
        backend: "web-audio",
        contextState: audioEngine.ctx.state,
        error: error instanceof Error ? error.message : String(error),
      };
    }
  }

  // ── Project sync ───────────────────────────────────────────────────────────

  async loadProject(project: DawProject): Promise<void> {
    transport.stop();
    clipScheduler.cancelAll();
    this._trackIds = project.tracks.map((track) => track.id);

    // Create all track nodes first so bus routing targets exist when we wire outputs.
    for (const track of project.tracks) {
      mixer.getOrCreateTrack(track.id, track.volume, track.pan);
    }
    // Now sync all per-track state (mute/solo/phase/output).
    for (const track of project.tracks) {
      mixer.setVolume(track.id, track.volume);
      mixer.setPan(track.id, track.pan);
      mixer.setMute(track.id, track.muted ?? false);
      mixer.setSolo(track.id, track.solo ?? false);
      mixer.setPhaseInvert(track.id, track.advanced?.phaseInvert ?? false);
      mixer.setPreviewMode(track.id, track.monitor?.previewMode ?? "stereo");
      const outId = track.routing?.outputId;
      if (outId && outId !== "master") mixer.setTrackOutput(track.id, outId);
      else mixer.setTrackOutput(track.id, track.output ?? "master");
      mixer.syncTrackSends(track.id, track.sends ?? []);
    }
  }

  syncProject(project: DawProject): void {
    this._trackIds = project.tracks.map((track) => track.id);
    for (const track of project.tracks) {
      mixer.getOrCreateTrack(track.id, track.volume, track.pan);
      mixer.setVolume(track.id, track.volume);
      mixer.setPan(track.id, track.pan);
      mixer.setMute(track.id, track.muted ?? false);
      mixer.setSolo(track.id, track.solo ?? false);
      mixer.setPhaseInvert(track.id, track.advanced?.phaseInvert ?? false);
      mixer.setPreviewMode(track.id, track.monitor?.previewMode ?? "stereo");
    }
    for (const track of project.tracks) {
      const outId = track.routing?.outputId;
      if (outId && outId !== "master") mixer.setTrackOutput(track.id, outId);
      else mixer.setTrackOutput(track.id, track.output ?? "master");
      mixer.syncTrackSends(track.id, track.sends ?? []);
    }
  }

  // ── Transport ──────────────────────────────────────────────────────────────

  async play(positionSeconds?: number): Promise<void> {
    console.log(
      `[WebAudioAdapter] play(${positionSeconds ?? "current"}) — WebAudio/ClipScheduler path`,
    );
    if (positionSeconds !== undefined) {
      transport.seek(positionSeconds);
    }
    await transport.play();
  }

  pause(): void {
    console.log("[WebAudioAdapter] pause()");
    transport.pause();
  }

  stop(): void {
    console.log("[WebAudioAdapter] stop()");
    transport.stop();
  }

  seekSeconds(seconds: number): void {
    console.log(`[WebAudioAdapter] seekSeconds(${seconds})`);
    transport.seek(seconds);
  }

  setBpm(_bpm: number): void {
    // BPM is read from project state at scheduling time; no runtime change needed
  }

  setLoop(_enabled: boolean, _startSeconds: number, _endSeconds: number): void {
    // Placeholder — loop scheduling not yet implemented in ClipScheduler
  }

  // ── Track management ───────────────────────────────────────────────────────

  createTrack(track: DawTrack): void {
    if (!this._trackIds.includes(track.id)) this._trackIds.push(track.id);
    mixer.getOrCreateTrack(track.id, track.volume, track.pan);
    mixer.setPhaseInvert(track.id, track.advanced?.phaseInvert ?? false);
    mixer.setPreviewMode(track.id, track.monitor?.previewMode ?? "stereo");
    const outId = track.routing?.outputId;
    if (outId && outId !== "master") mixer.setTrackOutput(track.id, outId);
    else mixer.setTrackOutput(track.id, track.output ?? "master");
    mixer.syncTrackSends(track.id, track.sends ?? []);
  }

  removeTrack(trackId: TrackId): void {
    this._trackIds = this._trackIds.filter((id) => id !== trackId);
    mixer.removeTrack(trackId);
  }

  // ── Clip management ────────────────────────────────────────────────────────

  scheduleClip(_trackId: TrackId, _clip: DawClip): void {
    // ClipScheduler bulk-schedules on play(); individual scheduling not yet needed
  }

  unscheduleClip(_clipId: string): void {
    // placeholder
  }

  // ── Audio files ────────────────────────────────────────────────────────────

  loadAudioFile(_fileId: string, _buffer: AudioBuffer): void {
    // AudioEngine manages its own buffer registry; no-op here
  }

  unloadAudioFile(_fileId: string): void {
    // placeholder
  }

  // ── Mixer ──────────────────────────────────────────────────────────────────

  setTrackVolume(trackId: TrackId, volume: number): void {
    mixer.setVolume(trackId, volume);
  }

  setTrackPan(trackId: TrackId, pan: number): void {
    mixer.setPan(trackId, pan);
  }

  setTrackMute(trackId: TrackId, muted: boolean): void {
    mixer.setMute(trackId, muted);
  }

  setTrackSolo(trackId: TrackId, solo: boolean): void {
    mixer.setSolo(trackId, solo);
  }

  setTrackPhaseInvert(trackId: TrackId, inverted: boolean): void {
    mixer.setPhaseInvert(trackId, inverted);
  }

  setTrackPreviewMode(trackId: TrackId, mode: TrackPreviewMode): void {
    mixer.setPreviewMode(trackId, mode);
  }

  setTrackOutput(trackId: TrackId, output: string): void {
    mixer.setTrackOutput(trackId, output);
  }

  setMasterVolume(volume: number): void {
    mixer.setMasterVolume(volume);
  }

  // ── Insert devices ─────────────────────────────────────────────────────────

  addInsertDevice(_trackId: TrackId, _device: InsertDevice): void {
  }

  removeInsertDevice(_trackId: TrackId, _deviceId: string): void {
  }

  setInsertEnabled(_trackId: TrackId, _deviceId: string, _enabled: boolean): void {
  }

  setInsertParam(
    _trackId: TrackId,
    _deviceId: string,
    _param: string,
    _value: number | string | boolean,
  ): void {
  }

  // ── Metering ───────────────────────────────────────────────────────────────

  subscribeMeters(callback: MeterCallback): () => void {
    this._meterCallbacks.add(callback);
    if (this._meterCallbacks.size === 1) this._startMeterLoop();
    return () => this._meterCallbacks.delete(callback);
  }

  subscribeTransport(callback: TransportCallback): () => void {
    this._transportCallbacks.add(callback);
    if (this._transportCallbacks.size === 1 && this._transportRafId === null) {
      this._startTransportLoop();
    }
    return () => this._transportCallbacks.delete(callback);
  }

  // ── Private loops ──────────────────────────────────────────────────────────

  private _meterTickMs = 0;

  private _startMeterLoop(): void {
    const tick = (now: number) => {
      if (this._meterCallbacks.size === 0) {
        this._meterRafId = null;
        return;
      }
      // Throttle from Preferences > Timeline FPS; unlimited follows RAF.
      if (shouldRunVisualFrame(this._meterTickMs, now)) {
        this._meterTickMs = now;
        for (const cb of this._meterCallbacks) {
          cb("master", mixer.getMasterLevel() as StereoLevel);
          for (const trackId of this._trackIds) {
            cb(trackId, mixer.getLevel(trackId) as StereoLevel);
          }
        }
      }
      this._meterRafId = requestAnimationFrame(tick);
    };
    this._meterRafId = requestAnimationFrame(tick);
  }

  private _startTransportLoop(): void {
    let lastTransportAt = 0;
    const tick = (now: number) => {
      if (this._transportCallbacks.size === 0) {
        this._transportRafId = null;
        return;
      }
      // 0 = unlimited → run every RAF tick (true unlimited via RAF)
      const interval = getVisualFrameIntervalMs();
      if (now - lastTransportAt >= interval) {
        lastTransportAt = now;
        const state = {
          playing: transport.isPlaying,
          positionSeconds: transport.projectTime,
        };
        for (const cb of this._transportCallbacks) cb(state);
      }
      this._transportRafId = requestAnimationFrame(tick);
    };
    this._transportRafId = requestAnimationFrame(tick);
  }
}

export const webAudioEngineAdapter: AudioEngineAdapter = new WebAudioEngineAdapter();
