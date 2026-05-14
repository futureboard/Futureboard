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

class WebAudioEngineAdapter implements AudioEngineAdapter {
  private _meterCallbacks = new Set<MeterCallback>();
  private _transportCallbacks = new Set<TransportCallback>();
  private _meterRafId: number | null = null;
  private _transportRafId: number | null = null;
  private _initialized = false;

  // ── Lifecycle ──────────────────────────────────────────────────────────────

  async init(): Promise<void> {
    if (this._initialized) return;
    this._initialized = true;
    this._startMeterLoop();
    this._startTransportLoop();
  }

  dispose(): void {
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

  // ── Project sync ───────────────────────────────────────────────────────────

  async loadProject(project: DawProject): Promise<void> {
    transport.stop();
    clipScheduler.cancelAll();
    for (const track of project.tracks) {
      mixer.getOrCreateTrack(track.id, track.volume, track.pan);
      if (track.muted) mixer.setMute(track.id, true);
      if (track.solo)  mixer.setSolo(track.id, true);
    }
  }

  syncProject(project: DawProject): void {
    for (const track of project.tracks) {
      mixer.getOrCreateTrack(track.id, track.volume, track.pan);
    }
  }

  // ── Transport ──────────────────────────────────────────────────────────────

  async play(positionSeconds?: number): Promise<void> {
    if (positionSeconds !== undefined) {
      transport.seek(positionSeconds);
    }
    await transport.play();
  }

  pause(): void {
    transport.pause();
  }

  stop(): void {
    transport.stop();
  }

  seekSeconds(seconds: number): void {
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
    mixer.getOrCreateTrack(track.id, track.volume, track.pan);
  }

  removeTrack(trackId: TrackId): void {
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

  setMasterVolume(volume: number): void {
    mixer.setMasterVolume(volume);
  }

  // ── Insert devices ─────────────────────────────────────────────────────────
  // WebAudio first pass: state is persisted in projectStore; no-op here.
  // Individual WebAudio nodes (BiquadFilter, DynamicsCompressor, etc.)
  // would be wired per-device in a full implementation.

  addInsertDevice(_trackId: TrackId, _device: InsertDevice): void {}
  removeInsertDevice(_trackId: TrackId, _deviceId: string): void {}

  setInsertEnabled(_trackId: TrackId, _deviceId: string, _enabled: boolean): void {}

  setInsertParam(
    _trackId: TrackId,
    _deviceId: string,
    _param: string,
    _value: number | string | boolean,
  ): void {}

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
