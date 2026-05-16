/**
 * AudioEngineAdapter — the stable interface the UI uses to talk to audio.
 *
 * All calls go through this interface so the implementation can later be
 * swapped for a WASM AudioWorklet or a native DSP service without touching
 * UI code.
 */
import type { DawProject, DawTrack, DawClip, InsertDevice, TrackId } from "../types/daw";
import type { StereoLevel } from "./Mixer";

export type AudioEngineStatus = "uninitialized" | "running" | "suspended" | "closed" | "error";

export type MeterCallback = (trackId: TrackId | "master", level: StereoLevel) => void;

export type TransportCallback = (state: {
  playing: boolean;
  positionSeconds: number;
}) => void;

export interface AudioEngineAdapter {
  // ── Lifecycle ──────────────────────────────────────────────────────────────
  init(): Promise<void>;
  dispose(): void;
  getStatus(): AudioEngineStatus;

  // ── Project sync ───────────────────────────────────────────────────────────
  loadProject(project: DawProject): Promise<void>;
  syncProject(project: DawProject): void;

  // ── Transport ──────────────────────────────────────────────────────────────
  play(positionSeconds?: number): Promise<void>;
  pause(): void;
  stop(): void;
  seekSeconds(seconds: number): void;
  setBpm(bpm: number): void;
  setLoop(enabled: boolean, startSeconds: number, endSeconds: number): void;

  // ── Track management ───────────────────────────────────────────────────────
  createTrack(track: DawTrack): void;
  removeTrack(trackId: TrackId): void;

  // ── Clip management ────────────────────────────────────────────────────────
  scheduleClip(trackId: TrackId, clip: DawClip): void;
  unscheduleClip(clipId: string): void;

  // ── Audio files ────────────────────────────────────────────────────────────
  loadAudioFile(fileId: string, buffer: AudioBuffer): void;
  unloadAudioFile(fileId: string): void;

  // ── Mixer ──────────────────────────────────────────────────────────────────
  setTrackVolume(trackId: TrackId, volume: number): void;
  setTrackPan(trackId: TrackId, pan: number): void;
  setTrackMute(trackId: TrackId, muted: boolean): void;
  setTrackSolo(trackId: TrackId, solo: boolean): void;
  setTrackPhaseInvert(trackId: TrackId, inverted: boolean): void;
  setTrackOutput(trackId: TrackId, output: string): void;
  setMasterVolume(volume: number): void;

  // ── Insert devices ─────────────────────────────────────────────────────────
  addInsertDevice(trackId: TrackId, device: InsertDevice): void;
  removeInsertDevice(trackId: TrackId, deviceId: string): void;
  setInsertEnabled(trackId: TrackId, deviceId: string, enabled: boolean): void;
  setInsertParam(trackId: TrackId, deviceId: string, param: string, value: number | string | boolean): void;

  // ── Metering ───────────────────────────────────────────────────────────────
  subscribeMeters(callback: MeterCallback): () => void;
  subscribeTransport(callback: TransportCallback): () => void;
}
