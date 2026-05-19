/**
 * SphereAudioNative — thin TypeScript wrapper around the napi-rs `.node` addon.
 *
 * This replaces the old `NativeSphereAudioService` (child-process spawner).
 * Instead of launching an external binary and communicating over stdio JSON,
 * we load the Rust library directly into the Electron main process via Node.js
 * native addons (napi-rs / N-API).
 *
 * The `.node` addon is built by:
 *   cd frameworks/SphereDirectAudioEngine && cargo build --release
 *   # → target/release/sphere_direct_audio_engine.dll  (Windows)
 *   #     copied to apps/electron/resources/sphere_direct_audio_engine.node
 *
 * Lookup order for the .node file:
 *   1. <app.getPath('exe')>/../resources/sphere_direct_audio_engine.node
 *      (packaged Electron app)
 *   2. <__dirname>/../../../../frameworks/SphereDirectAudioEngine/target/release/sphere_direct_audio_engine.dll
 *      (dev mode, Windows)
 *   3. <__dirname>/../../../../frameworks/SphereDirectAudioEngine/target/debug/sphere_direct_audio_engine.dll
 *      (dev mode, debug build)
 */

import { createRequire } from "module";
import { fileURLToPath } from "url";
import path from "path";
import fs from "fs";
import { app } from "electron";
import type {
  SphereDeviceOpenConfig,
  SphereMeterSnapshot,
  SphereDauxBackendInfo,
  SphereDauxConfig,
  SphereDauxStatus,
  SphereStartRecordingConfig,
  SphereRecordingResult,
  SphereRecordingStatus,
} from "../ipc/channels.js";

// ESM-safe __dirname (not available natively in "type":"module" packages)
const _dirname = path.dirname(fileURLToPath(import.meta.url));

// ── N-API addon shape ─────────────────────────────────────────────────────────
// These types mirror the Rust structs exposed via #[napi(object)].

interface NativeStatus {
  available:    boolean;
  running:      boolean;
  streamOpen:   boolean;
  transportPlaying: boolean;
  positionSeconds:  number;
  version:      string;
  backendName:  string;
  sampleRate:   number;
  bufferSize:   number;
  inputDevice:  string | null;
  outputDevice: string | null;
  lastError:    string | null;
}

interface NativeDeviceInfo {
  id:               string;
  name:             string;
  kind:             string;
  channels:         number;
  defaultSampleRate:number;
  isDefault:        boolean;
  backend:          string;
}

interface NativeMeterSnapshot {
  tracks?: NativeTrackMeterSnapshot[];
  masterPeakL: number;
  masterPeakR: number;
  masterRmsL:  number;
  masterRmsR:  number;
}

interface NativeTrackMeterSnapshot {
  trackId?: string;
  track_id?: string;
  peakL?: number;
  peakR?: number;
  rmsL?: number;
  rmsR?: number;
  peak_l?: number;
  peak_r?: number;
  rms_l?: number;
  rms_r?: number;
}

interface NativeDeviceOpenConfig {
  inputDeviceId?:  string;
  outputDeviceId?: string;
  sampleRate?:     number;
  bufferSize?:     number;
}

interface NativeDebugInfo {
  projectId:      string | null;
  loadedTracks:   number;
  loadedClips:    number;
  readyClips:     number;
  isPlaying:      boolean;
  positionSeconds: number;
  hasSolo:        boolean;
  clipSummaries:  string[];
}

interface NativeDauxBackendInfo {
  id:          string;
  name:        string;
  available:   boolean;
  isDefault:   boolean;
  description: string;
}

interface NativeDauxConfig {
  backendId:       string;
  outputDeviceId?: string;
  sampleRate?:     number;
  bufferSize?:     number;
  mmcssPriority:   boolean;
  safeMode:        boolean;
}

interface NativeDauxStatus {
  backendId:          string;
  backendName:        string;
  outputDevice:       string | null;
  sampleRate:         number;
  bufferSize:         number;
  estimatedLatencyMs: number;
  glitchCount:        number;
  mmcssActive:        boolean;
}

interface NativeWavPeakResult {
  fileId?: string;
  file_id?: string;
  sampleRate?: number;
  sample_rate?: number;
  channelCount?: number;
  channel_count?: number;
  duration: number;
  samplesPerPeak?: number;
  samples_per_peak?: number;
  peakCount?: number;
  peak_count?: number;
  peaks: number[];
}

/** Shape of the `SphereDirectAudioEngine` napi class instance. */
interface NativeEngine {
  getVersion(): string;
  getStatus(): NativeStatus;
  listInputDevices(): NativeDeviceInfo[];
  listOutputDevices(): NativeDeviceInfo[];
  openDevice(config: NativeDeviceOpenConfig): void;
  closeDevice(): void;
  start(): void;
  stop(): void;
  play(): void;
  pause(): void;
  seek(seconds: number): void;
  setTestTone(enabled: boolean, frequency: number): void;
  setMasterVolume(value: number): void;
  loadProject(snapshotJson: string): void;
  updateTrackParam(trackId: string, paramId: string, value: number): void;
  updateInsertParam(trackId: string, insertId: string, paramId: string, value: number): void;
  updateClip(clipId: string, patchJson: string): void;
  getMeters(): NativeMeterSnapshot;
  getDebugInfo(): NativeDebugInfo;
  // DAUx backend selection
  listDauxBackends(): NativeDauxBackendInfo[];
  openDaux(config: NativeDauxConfig): void;
  openDauxSafe(config: NativeDauxConfig): void;
  getDauxStatus(): NativeDauxStatus;

  // Recording
  startRecording(config: NativeStartRecordingConfig): void;
  stopRecording(): NativeRecordingResult[];
  getRecordingStatus(): NativeRecordingStatus;
  generateWavPeaks(filePath: string, fileId: string, samplesPerPeak: number): NativeWavPeakResult;
}

interface NativeRecordingTrackConfig {
  trackId: string;
  inputChannels: number[];
  name: string;
}

interface NativeStartRecordingConfig {
  projectRoot: string;
  sessionId: string;
  bpm: number;
  startBeat: number;
  sampleRate: number;
  inputDeviceId?: string | null;
  tracks: NativeRecordingTrackConfig[];
}

interface NativeRecordingResult {
  trackId: string;
  filePath: string;
  relativePath: string;
  startBeat: number;
  durationSeconds: number;
  sampleRate: number;
  channels: number;
  success: boolean;
  error?: string | null;
}

interface NativeRecordingStatus {
  active: boolean;
  durationSeconds: number;
  trackCount: number;
}

function encodeTrackParamValue(paramId: string, value: unknown): number {
  if (paramId === "previewMode") {
    switch (value) {
      case "mono": return 1;
      case "mid": return 2;
      case "side": return 3;
      default: return 0;
    }
  }
  return Number(value);
}

/** Addon module as loaded by require(). */
interface SphereAudioAddon {
  SphereDirectAudioEngine: new () => NativeEngine;
}

function isDefaultDeviceAlias(id: string): boolean {
  const normalized = id.trim().toLowerCase();
  return normalized === "" ||
    normalized === "__default__" ||
    normalized === "default" ||
    normalized === "communications";
}

function resolveNativeDeviceId(
  requestedId: string | undefined,
  devices: NativeDeviceInfo[],
  kind: "input" | "output",
): string | undefined {
  if (typeof requestedId !== "string") return undefined;
  if (isDefaultDeviceAlias(requestedId)) return undefined;

  const exact = devices.find((device) => device.id === requestedId);
  if (exact) return exact.id;

  const byName = devices.find((device) => device.name === requestedId);
  if (byName) return byName.id;

  console.warn(
    `[SphereAudio] Ignoring unknown ${kind} device id '${requestedId}' and using system default`,
  );
  return undefined;
}

function toNativeDeviceOpenConfig(
  config: SphereDeviceOpenConfig | null | undefined = {},
  devices: {
    inputs:  NativeDeviceInfo[];
    outputs: NativeDeviceInfo[];
  } = { inputs: [], outputs: [] },
): NativeDeviceOpenConfig {
  const source = config ?? {};
  const nativeConfig: NativeDeviceOpenConfig = {};

  const inputDeviceId = resolveNativeDeviceId(source.inputDeviceId, devices.inputs, "input");
  if (inputDeviceId) {
    nativeConfig.inputDeviceId = inputDeviceId;
  }
  const outputDeviceId = resolveNativeDeviceId(source.outputDeviceId, devices.outputs, "output");
  if (outputDeviceId) {
    nativeConfig.outputDeviceId = outputDeviceId;
  }
  if (
    typeof source.sampleRate === "number" &&
    Number.isFinite(source.sampleRate) &&
    source.sampleRate > 0
  ) {
    nativeConfig.sampleRate = source.sampleRate;
  }
  if (
    typeof source.bufferSize === "number" &&
    Number.isFinite(source.bufferSize) &&
    source.bufferSize > 0
  ) {
    nativeConfig.bufferSize = source.bufferSize;
  }

  return nativeConfig;
}

// ── Candidate .node paths ─────────────────────────────────────────────────────

function candidatePaths(): string[] {
  const addonName = process.platform === "win32"
    ? "DAUx.dll"
    : process.platform === "darwin"
    ? "libDAUx.dylib"
    : "libDAUx.so";

  // napi-rs / Node.js native addon extension
  const nodeAddonName = "DAUx.node";

  const candidates: string[] = [];

  // 1. Packaged Electron — process.resourcesPath is always the unpacked
  //    resources directory next to the asar, even when asar is active.
  //    (app.getPath("exe") points into the asar on some setups; avoid it.)
  if (process.resourcesPath) {
    candidates.push(path.join(process.resourcesPath, nodeAddonName));
  }

  // 2. Dev build — dist/native-audio/ → go up 4 levels to workspace root.
  //    _dirname = …/apps/electron/dist/native-audio  (compiled output)
  const wsRoot = path.resolve(_dirname, "..", "..", "..", "..");
  const engineRoot = path.join(wsRoot, "frameworks", "SphereDirectAudioEngine");

  // 2a. After `napi build` — .node placed in engine crate root
  candidates.push(path.join(engineRoot, nodeAddonName));

  // 2b. After `cargo build --release` — raw platform binary
  candidates.push(path.join(engineRoot, "target", "release", addonName));

  // 2c. After `cargo build` (debug)
  candidates.push(path.join(engineRoot, "target", "debug", addonName));

  // 3. Electron resources/ subdir (copy-native-audio.mjs destination in dev)
  const electronRoot = path.resolve(_dirname, "..", "..");
  candidates.push(path.join(electronRoot, "resources", nodeAddonName));

  return candidates;
}

// ── Loader ────────────────────────────────────────────────────────────────────

let _addon: SphereAudioAddon | null = null;
let _loadError: string | null = null;

function loadAddon(): SphereAudioAddon | null {
  if (_addon) return _addon;
  if (_loadError) return null;

  const req = createRequire(import.meta.url);
  const paths = candidatePaths();

  for (const p of paths) {
    if (fs.existsSync(p)) {
      try {
        _addon = req(p) as SphereAudioAddon;
        console.log(`[SphereAudio] Loaded native addon from: ${p}`);
        return _addon;
      } catch (err) {
        console.warn(`[SphereAudio] Failed to load addon from ${p}: ${String(err)}`);
      }
    }
  }

  _loadError = `Native addon not found. Searched:\n${paths.map((p) => `  ${p}`).join("\n")}`;
  console.error(`[SphereAudio] ${_loadError}`);
  return null;
}

// ── SphereAudioNative ────────────────────────────────────────────────────────

/**
 * Singleton wrapper around the napi-rs native engine instance.
 *
 * All public methods are safe to call even if the addon failed to load —
 * they return sensible defaults and log a warning instead of throwing.
 */
export class SphereAudioNative {
  private _engine: NativeEngine | null = null;

  /** Attempt to load the addon and create an engine instance. */
  initialize(): boolean {
    if (this._engine) return true;
    const addon = loadAddon();
    if (!addon) return false;

    try {
      this._engine = new addon.SphereDirectAudioEngine();
      console.log(`[SphereAudio] Engine v${this._engine.getVersion()} ready`);
      return true;
    } catch (err) {
      console.error(`[SphereAudio] Failed to create engine instance: ${String(err)}`);
      return false;
    }
  }

  get isAvailable(): boolean {
    return this._engine !== null;
  }

  // ── Version / Status ─────────────────────────────────────────────────────

  getVersion(): string {
    return this._engine?.getVersion() ?? "0.0.0";
  }

  getStatus(): NativeStatus {
    if (!this._engine) {
      return {
        available:    false,
        running:      false,
        streamOpen:   false,
        transportPlaying: false,
        positionSeconds:  0,
        version:      "0.0.0",
        backendName:  "unavailable",
        sampleRate:   0,
        bufferSize:   0,
        inputDevice:  null,
        outputDevice: null,
        lastError:    _loadError,
      };
    }
    return this._engine.getStatus();
  }

  // ── Device enumeration ───────────────────────────────────────────────────

  listInputDevices(): NativeDeviceInfo[] {
    return this._engine?.listInputDevices() ?? [];
  }

  listOutputDevices(): NativeDeviceInfo[] {
    return this._engine?.listOutputDevices() ?? [];
  }

  // ── Stream lifecycle ─────────────────────────────────────────────────────

  openDevice(config: SphereDeviceOpenConfig | null | undefined = {}): void {
    if (!this._engine) {
      throw new Error("[SphereAudio] Engine not available — addon failed to load");
    }
    this._engine.openDevice(toNativeDeviceOpenConfig(config, {
      inputs:  this._engine.listInputDevices(),
      outputs: this._engine.listOutputDevices(),
    }));
  }

  closeDevice(): void {
    this._engine?.closeDevice();
  }

  start(): void {
    if (!this._engine) throw new Error("[SphereAudio] Engine not available");
    this._engine.start();
  }

  stop(): void {
    this._engine?.stop();
  }

  // ── Transport ────────────────────────────────────────────────────────────

  play(): void {
    if (!this._engine) throw new Error("[SphereAudio] Engine not available");
    this._engine.play();
  }

  pause(): void {
    this._engine?.pause();
  }

  seek(seconds: number): void {
    if (!this._engine) throw new Error("[SphereAudio] Engine not available");
    this._engine.seek(seconds);
  }

  // ── Test tone ────────────────────────────────────────────────────────────

  setTestTone(enabled: boolean, frequency: number): void {
    this._engine?.setTestTone(enabled, frequency);
  }

  // ── Master volume ────────────────────────────────────────────────────────

  setMasterVolume(value: number): void {
    this._engine?.setMasterVolume(value);
  }

  // ── Project snapshot ─────────────────────────────────────────────────────

  loadProject(snapshot: unknown): void {
    if (!this._engine) throw new Error("[SphereAudio] Engine not available");
    const json = typeof snapshot === "string" ? snapshot : JSON.stringify(snapshot);
    this._engine.loadProject(json);
  }

  // ── Param updates ────────────────────────────────────────────────────────

  updateTrackParam(trackId: string, paramId: string, value: unknown): void {
    if (!this._engine) throw new Error("[SphereAudio] Engine not available");
    this._engine.updateTrackParam(trackId, paramId, encodeTrackParamValue(paramId, value));
  }

  updateInsertParam(trackId: string, insertId: string, paramId: string, value: unknown): void {
    if (!this._engine) throw new Error("[SphereAudio] Engine not available");
    this._engine.updateInsertParam(trackId, insertId, paramId, Number(value));
  }

  updateClip(clipId: string, patch: unknown): void {
    if (!this._engine) throw new Error("[SphereAudio] Engine not available");
    const json = typeof patch === "string" ? patch : JSON.stringify(patch);
    this._engine.updateClip(clipId, json);
  }

  // ── Meters ───────────────────────────────────────────────────────────────

  getMeters(): SphereMeterSnapshot {
    if (!this._engine) {
      return { tracks: {}, master: { left: 0, right: 0 }, timestamp: Date.now() };
    }
    const m = this._engine.getMeters();
    const tracks: SphereMeterSnapshot["tracks"] = {};
    for (const meter of m.tracks ?? []) {
      const trackId = meter.trackId ?? meter.track_id;
      if (!trackId) continue;
      tracks[trackId] = {
        left:  meter.peakL ?? meter.peak_l ?? 0,
        right: meter.peakR ?? meter.peak_r ?? 0,
      };
    }
    return {
      tracks,
      master:    { left: m.masterPeakL, right: m.masterPeakR },
      timestamp: Date.now(),
    };
  }

  // ── Debug info ────────────────────────────────────────────────────────────

  getDebugInfo(): NativeDebugInfo {
    if (!this._engine) {
      return {
        projectId: null, loadedTracks: 0, loadedClips: 0, readyClips: 0,
        isPlaying: false, positionSeconds: 0, hasSolo: false, clipSummaries: [],
      };
    }
    return this._engine.getDebugInfo();
  }

  // ── DAUx backend selection ────────────────────────────────────────────────

  listDauxBackends(): SphereDauxBackendInfo[] {
    return this._engine?.listDauxBackends() ?? [];
  }

  openDaux(config: SphereDauxConfig): void {
    if (!this._engine) throw new Error("[SphereAudio] Engine not available");
    const outputs = this.listOutputDevices() as NativeDeviceInfo[];
    const outputDeviceId = resolveNativeDeviceId(config.outputDeviceId, outputs, "output");
    const backendId = config.backendId === "wasapi" ? "wasapi-shared" : config.backendId;
    this._engine.openDaux({
      backendId,
      outputDeviceId,
      sampleRate:      config.sampleRate,
      bufferSize:      config.bufferSize,
      mmcssPriority:   config.mmcssPriority ?? true,
      safeMode:        config.safeMode ?? false,
    });
  }

  /**
   * Safe variant: tries the new config and restores the previous backend on failure.
   * Throws with the error message so callers can show it in the UI.
   */
  openDauxSafe(config: SphereDauxConfig): void {
    if (!this._engine) throw new Error("[SphereAudio] Engine not available");
    const outputs = this.listOutputDevices() as NativeDeviceInfo[];
    const outputDeviceId = resolveNativeDeviceId(config.outputDeviceId, outputs, "output");
    const backendId = config.backendId === "wasapi" ? "wasapi-shared" : config.backendId;
    this._engine.openDauxSafe({
      backendId,
      outputDeviceId,
      sampleRate:      config.sampleRate,
      bufferSize:      config.bufferSize,
      mmcssPriority:   config.mmcssPriority ?? true,
      safeMode:        config.safeMode ?? false,
    });
  }

  getDauxStatus(): SphereDauxStatus {
    if (!this._engine) {
      return {
        backendId: "none", backendName: "Unavailable", outputDevice: null,
        sampleRate: 0, bufferSize: 0, estimatedLatencyMs: 0,
        glitchCount: 0, mmcssActive: false,
      };
    }
    return this._engine.getDauxStatus();
  }

  // ── Recording ────────────────────────────────────────────────────────────

  startRecording(config: SphereStartRecordingConfig): void {
    if (!this._engine) throw new Error("[SphereAudio] Engine not available");
    this._engine.startRecording({
      projectRoot:   config.projectRoot,
      sessionId:     config.sessionId,
      bpm:           config.bpm,
      startBeat:     config.startBeat,
      sampleRate:    config.sampleRate,
      inputDeviceId: config.inputDeviceId ?? null,
      tracks:        config.tracks.map((t) => ({
        trackId:       t.trackId,
        inputChannels: t.inputChannels,
        name:          t.name,
      })),
    });
  }

  stopRecording(): SphereRecordingResult[] {
    if (!this._engine) throw new Error("[SphereAudio] Engine not available");
    return this._engine.stopRecording();
  }

  getRecordingStatus(): SphereRecordingStatus {
    if (!this._engine) return { active: false, durationSeconds: 0, trackCount: 0 };
    return this._engine.getRecordingStatus();
  }

  generateWavPeaks(filePath: string, fileId: string, samplesPerPeak: number): {
    fileId: string;
    sampleRate: number;
    channelCount: number;
    duration: number;
    samplesPerPeak: number;
    peakCount: number;
    peaks: number[];
  } | null {
    if (!this._engine) return null;
    const result = this._engine.generateWavPeaks(filePath, fileId, samplesPerPeak);
    return {
      fileId: result.fileId ?? result.file_id ?? fileId,
      sampleRate: result.sampleRate ?? result.sample_rate ?? 48000,
      channelCount: result.channelCount ?? result.channel_count ?? 1,
      duration: result.duration,
      samplesPerPeak: result.samplesPerPeak ?? result.samples_per_peak ?? samplesPerPeak,
      peakCount: result.peakCount ?? result.peak_count ?? 0,
      peaks: result.peaks,
    };
  }
}

/** Singleton instance — created lazily on first IPC call. */
export const sphereAudioNative = new SphereAudioNative();
