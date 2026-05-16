/**
 * WebAudioEngineAdapter — wraps the existing Transport/Mixer/ClipScheduler
 * singletons behind the AudioEngineAdapter interface.
 *
 * Future: swap this for a WASM or native DSP adapter without touching UI code.
 */
import type { DawProject, DawTrack, DawClip, InsertDevice, TrackId } from "../types/daw";
import type { AudioEngineAdapter, AudioEngineStatus, MeterCallback, TransportCallback } from "./AudioEngineAdapter";
import type { StereoLevel } from "./Mixer";
import { audioEngine } from "./AudioEngine";
import { transport } from "./Transport";
import { mixer } from "./Mixer";
import { clipScheduler } from "./ClipScheduler";
import { WasmAudioEngineAdapter } from "./WasmAudioEngineAdapter";

class WebAudioEngineAdapter implements AudioEngineAdapter {
  private _meterCallbacks = new Set<MeterCallback>();
  private _transportCallbacks = new Set<TransportCallback>();
  private _meterRafId: number | null = null;
  private _transportRafId: number | null = null;
  private _initialized = false;
  private _wasmAdapter: WasmAudioEngineAdapter | null = null;
  private _useWasm = false;

  // ── Lifecycle ──────────────────────────────────────────────────────────────

  async init(): Promise<void> {
    if (this._initialized) return;

    // Try to load WASM engine first
    try {
      this._wasmAdapter = new WasmAudioEngineAdapter();
      await this._wasmAdapter.init();
      this._useWasm = true;
      console.log("[AudioEngine] Using WASM DSP core");
    } catch (e) {
      console.warn("[AudioEngine] WASM core failed to load, falling back to WebAudio:", e);
      this._wasmAdapter = null;
      this._useWasm = false;
    }

    if (!this._useWasm) {
      this._startMeterLoop();
      this._startTransportLoop();
    }
    
    this._initialized = true;
  }

  dispose(): void {
    if (this._useWasm && this._wasmAdapter) {
      this._wasmAdapter.dispose();
    }
    if (this._meterRafId !== null) cancelAnimationFrame(this._meterRafId);
    if (this._transportRafId !== null) cancelAnimationFrame(this._transportRafId);
    this._meterCallbacks.clear();
    this._transportCallbacks.clear();
    this._initialized = false;
  }

  getStatus(): AudioEngineStatus {
    if (this._useWasm && this._wasmAdapter) {
      return this._wasmAdapter.getStatus();
    }
    const ctx = audioEngine.ctx;
    if (!ctx) return "uninitialized";
    switch (ctx.state) {
      case "running":   return "running";
      case "suspended": return "suspended";
      case "closed":    return "closed";
      default:          return "error";
    }
  }

  // ── Project sync ───────────────────────────────────────────────────────────

  async loadProject(project: DawProject): Promise<void> {
    if (this._useWasm && this._wasmAdapter) {
      return this._wasmAdapter.loadProject(project);
    }
    transport.stop();
    clipScheduler.cancelAll();

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
      const outId = track.routing?.outputId;
      if (outId && outId !== "master") mixer.setTrackOutput(track.id, outId);
    }
  }

  syncProject(project: DawProject): void {
    if (this._useWasm && this._wasmAdapter) {
      return this._wasmAdapter.syncProject(project);
    }
    for (const track of project.tracks) {
      mixer.getOrCreateTrack(track.id, track.volume, track.pan);
      mixer.setVolume(track.id, track.volume);
      mixer.setPan(track.id, track.pan);
      mixer.setPhaseInvert(track.id, track.advanced?.phaseInvert ?? false);
    }
  }

  // ── Transport ──────────────────────────────────────────────────────────────

  async play(positionSeconds?: number): Promise<void> {
    if (this._useWasm && this._wasmAdapter) {
      return this._wasmAdapter.play(positionSeconds);
    }
    if (positionSeconds !== undefined) {
      transport.seek(positionSeconds);
    }
    await transport.play();
  }

  pause(): void {
    if (this._useWasm && this._wasmAdapter) {
      return this._wasmAdapter.pause();
    }
    transport.pause();
  }

  stop(): void {
    if (this._useWasm && this._wasmAdapter) {
      return this._wasmAdapter.stop();
    }
    transport.stop();
  }

  seekSeconds(seconds: number): void {
    if (this._useWasm && this._wasmAdapter) {
      return this._wasmAdapter.seekSeconds(seconds);
    }
    transport.seek(seconds);
  }

  setBpm(bpm: number): void {
    if (this._useWasm && this._wasmAdapter) {
      return this._wasmAdapter.setBpm(bpm);
    }
    // BPM is read from project state at scheduling time; no runtime change needed
  }

  setLoop(enabled: boolean, startSeconds: number, endSeconds: number): void {
    if (this._useWasm && this._wasmAdapter) {
      return this._wasmAdapter.setLoop(enabled, startSeconds, endSeconds);
    }
    // Placeholder — loop scheduling not yet implemented in ClipScheduler
  }

  // ── Track management ───────────────────────────────────────────────────────

  createTrack(track: DawTrack): void {
    if (this._useWasm && this._wasmAdapter) {
      return this._wasmAdapter.createTrack(track);
    }
    mixer.getOrCreateTrack(track.id, track.volume, track.pan);
    mixer.setPhaseInvert(track.id, track.advanced?.phaseInvert ?? false);
    const outId = track.routing?.outputId;
    if (outId && outId !== "master") mixer.setTrackOutput(track.id, outId);
  }

  removeTrack(trackId: TrackId): void {
    if (this._useWasm && this._wasmAdapter) {
      return this._wasmAdapter.removeTrack(trackId);
    }
    mixer.removeTrack(trackId);
  }

  // ── Clip management ────────────────────────────────────────────────────────

  scheduleClip(trackId: TrackId, clip: DawClip): void {
    if (this._useWasm && this._wasmAdapter) {
      return this._wasmAdapter.scheduleClip(trackId, clip);
    }
    // ClipScheduler bulk-schedules on play(); individual scheduling not yet needed
  }

  unscheduleClip(clipId: string): void {
    if (this._useWasm && this._wasmAdapter) {
      return this._wasmAdapter.unscheduleClip(clipId);
    }
    // placeholder
  }

  // ── Audio files ────────────────────────────────────────────────────────────

  loadAudioFile(fileId: string, buffer: AudioBuffer): void {
    if (this._useWasm && this._wasmAdapter) {
      return this._wasmAdapter.loadAudioFile(fileId, buffer);
    }
    // AudioEngine manages its own buffer registry; no-op here
  }

  unloadAudioFile(fileId: string): void {
    if (this._useWasm && this._wasmAdapter) {
      return this._wasmAdapter.unloadAudioFile(fileId);
    }
    // placeholder
  }

  // ── Mixer ──────────────────────────────────────────────────────────────────

  setTrackVolume(trackId: TrackId, volume: number): void {
    if (this._useWasm && this._wasmAdapter) {
      return this._wasmAdapter.setTrackVolume(trackId, volume);
    }
    mixer.setVolume(trackId, volume);
  }

  setTrackPan(trackId: TrackId, pan: number): void {
    if (this._useWasm && this._wasmAdapter) {
      return this._wasmAdapter.setTrackPan(trackId, pan);
    }
    mixer.setPan(trackId, pan);
  }

  setTrackMute(trackId: TrackId, muted: boolean): void {
    if (this._useWasm && this._wasmAdapter) {
      return this._wasmAdapter.setTrackMute(trackId, muted);
    }
    mixer.setMute(trackId, muted);
  }

  setTrackSolo(trackId: TrackId, solo: boolean): void {
    if (this._useWasm && this._wasmAdapter) {
      return this._wasmAdapter.setTrackSolo(trackId, solo);
    }
    mixer.setSolo(trackId, solo);
  }

  setTrackPhaseInvert(trackId: TrackId, inverted: boolean): void {
    if (this._useWasm && this._wasmAdapter) {
      return this._wasmAdapter.setTrackPhaseInvert(trackId, inverted);
    }
    mixer.setPhaseInvert(trackId, inverted);
  }

  setTrackOutput(trackId: TrackId, output: string): void {
    if (this._useWasm && this._wasmAdapter) {
      return this._wasmAdapter.setTrackOutput(trackId, output);
    }
    mixer.setTrackOutput(trackId, output);
  }

  setMasterVolume(volume: number): void {
    if (this._useWasm && this._wasmAdapter) {
      return this._wasmAdapter.setMasterVolume(volume);
    }
    mixer.setMasterVolume(volume);
  }

  // ── Insert devices ─────────────────────────────────────────────────────────

  addInsertDevice(trackId: TrackId, device: InsertDevice): void {
    if (this._useWasm && this._wasmAdapter) {
      return this._wasmAdapter.addInsertDevice(trackId, device);
    }
  }

  removeInsertDevice(trackId: TrackId, deviceId: string): void {
    if (this._useWasm && this._wasmAdapter) {
      return this._wasmAdapter.removeInsertDevice(trackId, deviceId);
    }
  }

  setInsertEnabled(trackId: TrackId, deviceId: string, enabled: boolean): void {
    if (this._useWasm && this._wasmAdapter) {
      return this._wasmAdapter.setInsertEnabled(trackId, deviceId, enabled);
    }
  }

  setInsertParam(
    trackId: TrackId,
    deviceId: string,
    param: string,
    value: number | string | boolean,
  ): void {
    if (this._useWasm && this._wasmAdapter) {
      return this._wasmAdapter.setInsertParam(trackId, deviceId, param, value);
    }
  }

  // ── Metering ───────────────────────────────────────────────────────────────

  subscribeMeters(callback: MeterCallback): () => void {
    if (this._useWasm && this._wasmAdapter) {
      return this._wasmAdapter.subscribeMeters(callback);
    }
    this._meterCallbacks.add(callback);
    if (this._meterCallbacks.size === 1) this._startMeterLoop();
    return () => this._meterCallbacks.delete(callback);
  }

  subscribeTransport(callback: TransportCallback): () => void {
    if (this._useWasm && this._wasmAdapter) {
      return this._wasmAdapter.subscribeTransport(callback);
    }
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
      // Throttle to ~20 FPS for meters
      if (now - this._meterTickMs >= 50) {
        this._meterTickMs = now;
        for (const cb of this._meterCallbacks) {
          // Emit master level
          cb("master", mixer.getMasterLevel() as StereoLevel);
        }
      }
      this._meterRafId = requestAnimationFrame(tick);
    };
    this._meterRafId = requestAnimationFrame(tick);
  }

  private _startTransportLoop(): void {
    const tick = () => {
      if (this._transportCallbacks.size === 0) {
        this._transportRafId = null;
        return;
      }
      const state = {
        playing: transport.isPlaying,
        positionSeconds: transport.projectTime,
      };
      for (const cb of this._transportCallbacks) cb(state);
      this._transportRafId = requestAnimationFrame(tick);
    };
    this._transportRafId = requestAnimationFrame(tick);
  }
}

export const webAudioEngineAdapter: AudioEngineAdapter = new WebAudioEngineAdapter();
