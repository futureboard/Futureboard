/**
 * NativeSphereAudioEngineAdapter
 *
 * Implements AudioEngineAdapter by forwarding commands to the native
 * SphereDirectAudioEngine via window.dawElectron.sphereAudio (Electron preload).
 *
 * Safe to construct in a Web context — all methods check for the bridge and
 * fail gracefully when it's absent rather than throwing.
 *
 * UI code must never import this file directly. Use createAudioEngineAdapter()
 * or the active adapter singleton instead.
 */
import type {
  AudioEngineAdapter,
  AudioEngineStatus,
  AudioSelfTestResult,
  MeterCallback,
  TransportCallback,
} from "../AudioEngineAdapter";
import type { DawProject, DawTrack, DawClip, InsertDevice, TrackId, TrackPreviewMode } from "../../types/daw";
import type {
  EngineProjectSnapshot,
  EngineTrackSnapshot,
  EngineClipSnapshot,
  EngineAssetSnapshot,
  MeterSnapshot,
  StereoMeterLevel,
} from "./types";
import { platform } from "../../platform";
// ── Bridge accessor ───────────────────────────────────────────────────────────

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function getSphere(): any | null {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  return (window as any).dawElectron?.sphereAudio ?? null;
}

// ── Meters polling ────────────────────────────────────────────────────────────
const METER_POLL_MS = 16; // ~60 fps for responsive VU lights
const TRANSPORT_POLL_MS = 33; // ~30 fps is enough for UI transport sync
const INSERT_PARAM_FLUSH_MS = 16; // coalesce dense slider drags to one native batch per frame

// ── Snapshot builders ─────────────────────────────────────────────────────────

/**
 * Returns true when `p` looks like a real filesystem path rather than an
 * opaque storage key (IndexedDB keys start with "audio:", blobs with "blob:").
 * Accepts both Windows (`C:\…` / `C:/…`) and Unix (`/…`) absolute paths.
 */
function looksLikeFsPath(p: string): boolean {
  if (!p || p.length < 3) return false;
  if (p.startsWith("audio:") || p.startsWith("blob:") || p.startsWith("data:")) return false;
  return p.includes("/") || p.includes("\\") || /^[A-Za-z]:/.test(p);
}

const PROJECT_AUDIO_DIR = "Media/Audio";

function projectAudioRelativePathFromName(name?: string | null): string | null {
  const clean = name?.trim();
  if (!clean) return null;
  const basename = clean.replace(/\\/g, "/").split("/").filter(Boolean).pop();
  return basename ? `${PROJECT_AUDIO_DIR}/${basename}` : null;
}

function inferFolderFileRelativePath(
  file?: { relativePath?: string; storageProvider?: string; name?: string; originalFileName?: string } | null,
): string | null {
  if (!file) return null;
  if (file.relativePath) return file.relativePath;
  if (file.storageProvider !== "project-folder") return null;
  return projectAudioRelativePathFromName(file.name) ?? projectAudioRelativePathFromName(file.originalFileName);
}

function buildTrackSnapshot(track: DawTrack): EngineTrackSnapshot {
  return {
    id:            track.id,
    type:          track.type,
    volume:        track.volume,
    pan:           track.pan,
    muted:         track.muted ?? false,
    solo:          track.solo  ?? false,
    armed:         track.armed ?? false,
    previewMode:   track.monitor?.previewMode ?? "stereo",
    outputTrackId: track.output ?? null,
    inserts: (track.inserts ?? []).map((ins) => ({
      id:      ins.id,
      type:    ins.type,
      enabled: ins.enabled ?? true,
      params:  ins.params  ?? {},
    })),
    sends: (track.sends ?? []).map((send) => ({
      id: send.id,
      returnTrackId: send.targetTrackId,
      level: send.level ?? 1,
      enabled: send.enabled !== false,
    })),
  };
}

/**
 * Resolve an absolute filesystem path for a clip's audio file so that the
 * Rust native engine can read it via fs::read.
 *
 * Resolution order:
 *   0. Project asset manifest: clip.assetId → assets.relativePath + projectRoot
 *      (set for all Electron folder-project imports via Auto Import)
 *   1. DawFile.relativePath, or legacy folder-project file name inferred as
 *      Media/Audio/<filename>, plus runtime projectRoot
 *   2. DawFile.cacheKey / storageKey that looks like an absolute FS path
 *      — covers file-handle (native picker), drag-drop (Electron File.path),
 *        and project-folder reloaded from localStorage (cacheKey = absPath)
 *   3. null — pure IndexedDB / OPFS sources are unreachable from Rust
 *
 * Note: older .mochiproj files may have storageProvider="project-folder" but
 * no relativePath/assets.  For those files we infer Media/Audio/<file.name>
 * and let the trusted Electron side validate the path exists.
 */
function resolveClipAsset(project: DawProject | null, clip: DawClip) {
  if (!project) return null;
  const assetId = clip.assetId ?? clip.fileId;
  return (project.assets ?? []).find((a) => a.id === assetId)
    ?? (project.assets ?? []).find((a) => a.id === clip.fileId)
    ?? null;
}

function resolveClipFile(project: DawProject | null, clip: DawClip) {
  if (!project) return null;
  return project.files.find((f) => f.id === clip.assetId)
    ?? project.files.find((f) => f.id === clip.fileId)
    ?? null;
}

function resolveMediaPath(project: DawProject | null, clip: DawClip): string | null {
  if (!project) return null;

  // ── 0. Project asset manifest (preferred for Auto-Import clips) ───────────
  if (clip.assetId || clip.fileId) {
    const asset = resolveClipAsset(project, clip);
    if (asset && asset.relativePath && !asset.missing) {
      const root = platform.folderProject.getProjectRoot();
      if (root) {
        return `${root}/${asset.relativePath}`.replace(/\\/g, "/");
      }
      // projectRoot not in memory; fall through to DawFile cacheKey below.
      // The DawFile (same id) will have cacheKey = absolutePath from saveLocal.
    }
  }

  const file = resolveClipFile(project, clip);
  if (!file) return null;

  // ── 1. Folder project: relativePath + runtime projectRoot ─────────────────
  const inferredRelativePath = inferFolderFileRelativePath(file);
  if (inferredRelativePath) {
    const root = platform.folderProject.getProjectRoot();
    if (root) {
      return `${root}/${inferredRelativePath}`.replace(/\\/g, "/");
    }
    // projectRoot not in memory — fall through to cacheKey.
  }

  // ── 2. Any storage provider whose cacheKey / storageKey is a real FS path ──
  if (file.cacheKey && looksLikeFsPath(file.cacheKey)) {
    return file.cacheKey.replace(/\\/g, "/");
  }
  if (file.storageKey && looksLikeFsPath(file.storageKey)) {
    return file.storageKey.replace(/\\/g, "/");
  }

  // ── 3. IndexedDB / OPFS / blob — unreachable from the Rust process ────────
  return null;
}

function buildClipSnapshot(project: DawProject | null, clip: DawClip, bpm: number): EngineClipSnapshot {
  // Convert timeline seconds → beats for the native engine.
  const bps = bpm / 60;
  const asset = resolveClipAsset(project, clip);
  const file = resolveClipFile(project, clip);
  const assetId = clip.assetId ?? file?.id ?? clip.fileId;
  const relativePath = asset?.relativePath ?? inferFolderFileRelativePath(file);
  return {
    id:            clip.id,
    trackId:       clip.trackId,
    assetId,
    relativePath,
    mediaPath:     resolveMediaPath(project, clip),
    startBeat:     clip.startTime  * bps,
    durationBeats: clip.duration   * bps,
    offsetSeconds: clip.offset,
    gain:          clip.gain ?? 1,
    fades:         null,
    audioProcess:  clip.audioProcess
      ? {
          speedRatio:     clip.audioProcess.speedRatio,
          pitchSemitones: clip.audioProcess.pitchSemitones,
          preservePitch:  clip.audioProcess.preservePitch,
          mode:           clip.audioProcess.mode,
          quality:        clip.audioProcess.quality ?? "balanced",
        }
      : null,
  };
}

function buildAssetSnapshot(project: DawProject): EngineAssetSnapshot[] {
  const assets: EngineAssetSnapshot[] = (project.assets ?? []).map((asset) => {
    const file = project.files.find((f) => f.id === asset.id);
    return {
      id: asset.id,
      type: asset.type,
      name: asset.name,
      relativePath: asset.relativePath || inferFolderFileRelativePath(file) || "",
      missing: asset.missing,
    };
  });
  const existingIds = new Set(assets.map((asset) => asset.id));
  for (const file of project.files) {
    const relativePath = inferFolderFileRelativePath(file);
    if (!relativePath || existingIds.has(file.id)) continue;
    assets.push({
      id: file.id,
      type: "audio",
      name: file.name,
      relativePath,
      missing: false,
    });
  }
  return assets;
}

function buildFileSnapshot(project: DawProject): NonNullable<EngineProjectSnapshot["files"]> {
  return project.files.map((file) => ({
    id: file.id,
    name: file.name,
    originalFileName: file.originalFileName,
    storageProvider: file.storageProvider,
    relativePath: inferFolderFileRelativePath(file),
    cacheKey: file.cacheKey ?? null,
    storageKey: file.storageKey ?? null,
  }));
}

/**
 * Build the project snapshot asynchronously, validating each clip's media path
 * exists on disk and emitting per-clip `[NativeSnapshot]` debug logs.
 *
 * Clips whose path cannot be resolved or does not exist are marked mediaPath=null
 * (the Rust engine skips them gracefully rather than crashing).
 */
async function buildProjectSnapshotAsync(project: DawProject): Promise<EngineProjectSnapshot> {
  const allClips = project.tracks.flatMap((t) => t.clips);
  const projectRoot = platform.folderProject.getProjectRoot();
  const assets = buildAssetSnapshot(project);

  const assetCount = assets.length;
  console.log(`[NativeSnapshot] projectRoot = ${projectRoot ?? "null (project loaded from cache or no folder project)"}`);
  console.log(`[NativeSnapshot] assets = ${assetCount} (project.assets), files = ${project.files.length}, clips = ${allClips.length}`);

  const clips: EngineClipSnapshot[] = [];
  for (const c of allClips) {
    const snap = buildClipSnapshot(project, c, project.bpm);
    if (snap.mediaPath) {
      const stat = await platform.fileSystem.statAudioFile(snap.mediaPath).catch(() => null);
      const exists = stat !== null;
      console.log(
        `[NativeSnapshot] clip=${c.id} assetId=${snap.assetId} relativePath=${snap.relativePath ?? ""} mediaPath="${snap.mediaPath}" exists=${exists}`,
      );
      if (!exists) {
        console.warn(
          `[NativeSnapshot] ⚠ clip ${c.id} — file not found on disk, marking mediaPath=null`,
        );
        snap.mediaPath = null;
      }
    } else {
      const file = project.files.find((f) => f.id === c.fileId);
      const inferredRelativePath = inferFolderFileRelativePath(file);
      console.warn(
        `[NativeSnapshot] clip=${c.id} assetId=${snap.assetId} — no mediaPath resolved ` +
        `(storageProvider=${file?.storageProvider ?? "?"} ` +
        `cacheKey="${file?.cacheKey ?? ""}" ` +
        `storageKey="${file?.storageKey ?? ""}" ` +
        `relativePath="${inferredRelativePath ?? file?.relativePath ?? ""}")`,
      );
    }
    clips.push(snap);
  }

  return {
    projectId:     project.id,
    projectRoot,
    bpm:           project.bpm,
    timeSignature: [
      project.timeSignature?.numerator   ?? 4,
      project.timeSignature?.denominator ?? 4,
    ],
    sampleRate:    project.sampleRate ?? 44100,
    tracks:        project.tracks.map(buildTrackSnapshot),
    clips,
    assets,
    files:         buildFileSnapshot(project),
    routing: {
      masterOutputDevice: null,
      sampleRate:         project.sampleRate ?? 44100,
      bufferSize:         256,
    },
  };
}

// ── Adapter ───────────────────────────────────────────────────────────────────

export class NativeSphereAudioEngineAdapter implements AudioEngineAdapter {
  private _status:              AudioEngineStatus   = "uninitialized";
  private _meterCallbacks       = new Set<MeterCallback>();
  private _transportCallbacks   = new Set<TransportCallback>();
  private _meterPollId:         ReturnType<typeof setInterval> | null = null;
  private _transportPollId:     ReturnType<typeof setInterval> | null = null;
  private _meterPollInFlight    = false;
  private _transportPollInFlight = false;
  private _lastTransport        = { playing: false, positionSeconds: 0 };
  private _lastProjectSignature: string | null = null;
  // Debounce timer for syncProject — rapid edits batch into one Rust rebuild.
  private _syncTimer:           ReturnType<typeof setTimeout> | null = null;
  private _insertParamTimer:    ReturnType<typeof setTimeout> | null = null;
  private _pendingInsertParams  = new Map<string, {
    trackId: string;
    deviceId: string;
    param: string;
    value: number | boolean;
  }>();

  // ── Lifecycle ──────────────────────────────────────────────────────────────

  async init(): Promise<void> {
    const sphere = getSphere();
    if (!sphere) {
      console.warn("[NativeSphere] Preload bridge absent — adapter inactive");
      this._status = "error";
      return;
    }
    try {
      // Check if the engine is already running (auto-started by ipc-handlers).
      // If not, open the default device and start the stream ourselves.
      const status = await sphere.getStatus() as { running: boolean; streamOpen: boolean };
      if (!status.running) {
        if (!status.streamOpen) {
          // Open default output device/config.  User-selected device/buffer is
          // applied from Preferences via the same native bridge.
          await sphere.openDevice({});
        }
        await sphere.start();
      }
      this._status = "running";
      this._startPolling();
      console.log("[NativeSphere] Native audio engine ready");
    } catch (e) {
      console.error("[NativeSphere] Failed to start native engine:", e);
      this._status = "error";
      throw e;
    }
  }

  dispose(): void {
    this._stopPolling();
    this._meterCallbacks.clear();
    this._transportCallbacks.clear();
    const sphere = getSphere();
    if (sphere) {
      sphere.stop().catch((e: unknown) =>
        console.warn("[NativeSphere] stop() error during dispose:", e),
      );
    }
    this._status = "closed";
  }

  getStatus(): AudioEngineStatus {
    return this._status;
  }

  async runSelfTest(): Promise<AudioSelfTestResult> {
    const sphere = getSphere();
    if (!sphere) {
      return { ok: false, backend: "sphere-native", error: "SphereAudio preload bridge is unavailable" };
    }
    try {
      await sphere.setTestTone(true, 440);
      await new Promise((resolve) => setTimeout(resolve, 250));
      await sphere.setTestTone(false, 440);
      const status = await sphere.getStatus();
      return {
        ok: true,
        backend: "sphere-native",
        contextState: status.running ? "running" : "stopped",
        device: status.outputDevice ?? "default output",
      };
    } catch (error) {
      try {
        await sphere.setTestTone(false, 440);
      } catch {
        // ignore cleanup errors
      }
      return {
        ok: false,
        backend: "sphere-native",
        error: error instanceof Error ? error.message : String(error),
      };
    }
  }

  // ── Project sync ───────────────────────────────────────────────────────────

  async loadProject(project: DawProject): Promise<void> {
    const sphere = getSphere();
    if (!sphere) return;
    // buildProjectSnapshotAsync validates each path on disk and emits
    // [NativeSnapshot] per-clip logs so failures are visible in DevTools.
    const snapshot = await buildProjectSnapshotAsync(project);
    const trackCount = snapshot.tracks.length;
    const clipCount  = snapshot.clips.length;
    const pathCount  = snapshot.clips.filter((c) => c.mediaPath).length;
    console.log(
      `[SphereNativeAdapter] loadProject("${project.name ?? project.id}") → ` +
      `${trackCount} tracks, ${clipCount} clips (${pathCount} with validated media paths) → IPC`,
    );
    if (clipCount > 0 && pathCount === 0) {
      console.warn(
        "[SphereNativeAdapter] ⚠ All clips have null mediaPath — Rust engine will play silence! " +
        "Check [NativeSnapshot] logs above for per-clip resolution details.",
      );
    }
    await sphere.loadProject(snapshot);
    this._lastProjectSignature = projectGraphSignature(project);
    console.log("[SphereNativeAdapter] loadProject() → IPC call complete");
  }

  syncProject(project: DawProject): void {
    const signature = projectGraphSignature(project);
    if (signature === this._lastProjectSignature) {
      return;
    }
    // Debounce: batch rapid edits (clip drags, fader moves) into one Rust rebuild.
    // 120 ms keeps the engine in sync without decoding audio files on every frame.
    if (this._syncTimer !== null) clearTimeout(this._syncTimer);
    this._syncTimer = setTimeout(() => {
      this._syncTimer = null;
      const sphere = getSphere();
      if (!sphere) return;
      // Use async builder so paths are validated and logged consistently.
      buildProjectSnapshotAsync(project)
        .then((snapshot) => {
          const clipCount = snapshot.clips.filter((c) => c.mediaPath).length;
          console.log(
            `[SphereNativeAdapter] syncProject (debounced) → ${snapshot.clips.length} clips (${clipCount} with paths) → IPC`,
          );
          this._lastProjectSignature = signature;
          return sphere.loadProject(snapshot);
        })
        .catch((e: unknown) =>
          console.warn("[SphereNativeAdapter] syncProject error:", e),
        );
    }, 120);
  }

  // ── Transport ──────────────────────────────────────────────────────────────

  async play(positionSeconds?: number): Promise<void> {
    const sphere = getSphere();
    if (!sphere) {
      console.error("[SphereNativeAdapter] play(): sphere bridge absent — no audio output!");
      return;
    }
    console.log(
      `[SphereNativeAdapter] play() → IPC setTransportState({ playing: true, positionSeconds: ${positionSeconds ?? "undefined"} })`,
    );
    await sphere.setTransportState({ playing: true, positionSeconds });
    console.log("[SphereNativeAdapter] play() → IPC call complete");
  }

  pause(): void {
    const sphere = getSphere();
    if (!sphere) {
      console.error("[SphereNativeAdapter] pause(): sphere bridge absent");
      return;
    }
    console.log("[SphereNativeAdapter] pause() → IPC setTransportState({ playing: false })");
    sphere
      .setTransportState({ playing: false })
      .catch((e: unknown) => console.warn("[SphereNativeAdapter] pause error:", e));
  }

  stop(): void {
    const sphere = getSphere();
    if (!sphere) {
      console.error("[SphereNativeAdapter] stop(): sphere bridge absent");
      return;
    }
    console.log("[SphereNativeAdapter] stop() → IPC setTransportState({ playing: false, positionSeconds: 0 })");
    sphere
      .setTransportState({ playing: false, positionSeconds: 0 })
      .catch((e: unknown) => console.warn("[SphereNativeAdapter] stop error:", e));
    this._notifyTransport({ playing: false, positionSeconds: 0 });
  }

  seekSeconds(seconds: number): void {
    const sphere = getSphere();
    if (!sphere) {
      console.error("[SphereNativeAdapter] seekSeconds(): sphere bridge absent");
      return;
    }
    console.log(`[SphereNativeAdapter] seekSeconds(${seconds}) → IPC setTransportState`);
    sphere
      .setTransportState({ positionSeconds: seconds })
      .catch((e: unknown) => console.warn("[SphereNativeAdapter] seek error:", e));
  }

  setBpm(bpm: number): void {
    const sphere = getSphere();
    if (!sphere) return;
    sphere
      .updateTrackParam("__transport__", "bpm", bpm)
      .catch((e: unknown) => console.warn("[NativeSphere] setBpm error:", e));
  }

  setLoop(enabled: boolean, startSeconds: number, endSeconds: number): void {
    const sphere = getSphere();
    if (!sphere) return;
    sphere
      .setTransportState({ loop: enabled, loopStart: startSeconds, loopEnd: endSeconds })
      .catch((e: unknown) => console.warn("[NativeSphere] setLoop error:", e));
  }

  // ── Track management ───────────────────────────────────────────────────────

  createTrack(track: DawTrack): void {
    console.log(`[NativeSphere] createTrack(${track.id}) deferred to next project snapshot`);
  }

  removeTrack(trackId: TrackId): void {
    console.log(`[NativeSphere] removeTrack(${trackId}) deferred to next project snapshot`);
  }

  // ── Clip management ────────────────────────────────────────────────────────

  scheduleClip(trackId: TrackId, clip: DawClip): void {
    const sphere = getSphere();
    if (!sphere) return;
    // BPM is unknown at call site; use 120 as a safe default.
    // The full project snapshot (loadProject) keeps the engine in sync accurately.
    const snapshot = buildClipSnapshot(null, clip, 120);
    sphere
      .updateClip(clip.id, { ...snapshot, trackId })
      .catch((e: unknown) => console.warn("[NativeSphere] scheduleClip error:", e));
  }

  unscheduleClip(clipId: string): void {
    const sphere = getSphere();
    if (!sphere) return;
    sphere
      .updateClip(clipId, { __remove__: true })
      .catch((e: unknown) => console.warn("[NativeSphere] unscheduleClip error:", e));
  }

  // ── Audio files ────────────────────────────────────────────────────────────
  // Native engine loads files from disk paths — no buffer transfer over IPC.

  loadAudioFile(_fileId: string, _buffer: AudioBuffer): void {
    // No-op: native engine resolves media paths from the project snapshot.
  }

  unloadAudioFile(_fileId: string): void {
    // No-op.
  }

  // ── Mixer ──────────────────────────────────────────────────────────────────

  setTrackVolume(trackId: TrackId, volume: number): void {
    this._paramUpdate(trackId, "volume", volume);
  }

  setTrackPan(trackId: TrackId, pan: number): void {
    this._paramUpdate(trackId, "pan", pan);
  }

  setTrackMute(trackId: TrackId, muted: boolean): void {
    this._paramUpdate(trackId, "muted", muted);
  }

  setTrackSolo(trackId: TrackId, solo: boolean): void {
    this._paramUpdate(trackId, "solo", solo);
  }

  setTrackPhaseInvert(trackId: TrackId, inverted: boolean): void {
    this._paramUpdate(trackId, "phaseInvert", inverted);
  }

  setTrackPreviewMode(trackId: TrackId, mode: TrackPreviewMode): void {
    this._paramUpdate(trackId, "previewMode", mode);
  }

  setTrackOutput(trackId: TrackId, output: string): void {
    this._paramUpdate(trackId, "outputTrackId", output);
  }

  setMasterVolume(volume: number): void {
    this._paramUpdate("__master__", "volume", volume);
  }

  // ── Insert devices ─────────────────────────────────────────────────────────

  addInsertDevice(trackId: TrackId, device: InsertDevice): void {
    console.log(`[NativeSphere] addInsertDevice(${trackId}, ${device.id}) deferred to next project snapshot`);
  }

  removeInsertDevice(trackId: TrackId, deviceId: string): void {
    console.log(`[NativeSphere] removeInsertDevice(${trackId}, ${deviceId}) deferred to next project snapshot`);
  }

  setInsertEnabled(trackId: TrackId, deviceId: string, enabled: boolean): void {
    this._queueInsertParam(trackId, deviceId, "enabled", enabled);
  }

  setInsertParam(
    trackId: TrackId,
    deviceId: string,
    param: string,
    value: number | string | boolean,
  ): void {
    const sphere = getSphere();
    if (!sphere) return;
    if (param.startsWith("__")) {
      console.warn(`[NativeSphere] blocked invalid insert param: "${param}"`);
      return;
    }
    if (typeof value === "string") {
      console.log(
        `[NativeSphere] setInsertParam(${trackId}, ${deviceId}, ${param}) string value deferred to project snapshot`,
      );
      return;
    }
    this._queueInsertParam(trackId, deviceId, param, value);
  }

  // ── Metering ───────────────────────────────────────────────────────────────

  subscribeMeters(callback: MeterCallback): () => void {
    this._meterCallbacks.add(callback);
    return () => this._meterCallbacks.delete(callback);
  }

  subscribeTransport(callback: TransportCallback): () => void {
    this._transportCallbacks.add(callback);
    return () => this._transportCallbacks.delete(callback);
  }

  // ── Internal: realtime param update ───────────────────────────────────────

  private _paramUpdate(trackId: string, paramId: string, value: number | string | boolean): void {
    const sphere = getSphere();
    if (!sphere) return;
    sphere
      .updateTrackParam(trackId, paramId, value)
      .catch((e: unknown) =>
        console.warn(`[NativeSphere] updateTrackParam(${trackId}, ${paramId}) error:`, e),
      );
  }

  private _queueInsertParam(
    trackId: string,
    deviceId: string,
    param: string,
    value: number | boolean,
  ): void {
    this._pendingInsertParams.set(`${trackId}:${deviceId}:${param}`, {
      trackId,
      deviceId,
      param,
      value,
    });
    if (this._insertParamTimer !== null) return;
    this._insertParamTimer = setTimeout(() => {
      this._insertParamTimer = null;
      const sphere = getSphere();
      if (!sphere) {
        this._pendingInsertParams.clear();
        return;
      }
      const updates = [...this._pendingInsertParams.values()];
      this._pendingInsertParams.clear();
      for (const update of updates) {
        sphere
          .updateInsertParam(update.trackId, update.deviceId, update.param, update.value)
          .catch((e: unknown) => console.warn("[NativeSphere] setInsertParam error:", e));
      }
    }, INSERT_PARAM_FLUSH_MS);
  }

  // ── Internal: polling loops ────────────────────────────────────────────────

  private _startPolling(): void {
    this._stopPolling();

    // Meter poll — ~60 fps. Guard against overlapping IPC calls so slow
    // machines do not build a backlog that makes the VU feel delayed.
    this._meterPollId = setInterval(() => {
      if (this._meterCallbacks.size === 0) return;
      if (this._meterPollInFlight) return;
      const sphere = getSphere();
      if (!sphere) return;
      this._meterPollInFlight = true;
      sphere
        .getMeters()
        .then((snap: MeterSnapshot) => {
          for (const [trackId, level] of meterEntriesByTrackId(snap.tracks)) {
            const sl: { left: number; right: number } = level;
            for (const cb of this._meterCallbacks) {
              cb(trackId, { l: sl.left, r: sl.right });
            }
          }
          for (const cb of this._meterCallbacks) {
            cb("master", { l: snap.master.left, r: snap.master.right });
          }
        })
        .catch(() => {/* native engine may be busy — ignore */})
        .finally(() => {
          this._meterPollInFlight = false;
        });
    }, METER_POLL_MS);

    // Transport poll — ~30 fps
    this._transportPollId = setInterval(() => {
      if (this._transportCallbacks.size === 0) return;
      if (this._transportPollInFlight) return;
      const sphere = getSphere();
      if (!sphere) return;
      this._transportPollInFlight = true;
      sphere
        .getTransportState()
        .then((state: { playing: boolean; positionSeconds: number }) => {
          if (
            state.playing         !== this._lastTransport.playing ||
            Math.abs(state.positionSeconds - this._lastTransport.positionSeconds) > 0.01
          ) {
            this._lastTransport = state;
            this._notifyTransport(state);
          }
        })
        .catch(() => {})
        .finally(() => {
          this._transportPollInFlight = false;
        });
    }, TRANSPORT_POLL_MS);
  }

  private _stopPolling(): void {
    if (this._meterPollId     !== null) clearInterval(this._meterPollId);
    if (this._transportPollId !== null) clearInterval(this._transportPollId);
    this._meterPollId     = null;
    this._transportPollId = null;
  }

  private _notifyTransport(state: { playing: boolean; positionSeconds: number }): void {
    for (const cb of this._transportCallbacks) {
      cb(state);
    }
  }

}

function projectGraphSignature(project: DawProject): string {
  return JSON.stringify({
    id: project.id,
    bpm: project.bpm,
    sampleRate: project.sampleRate,
    assets: (project.assets ?? []).map((asset) => ({
      id: asset.id,
      relativePath: asset.relativePath,
      missing: asset.missing,
    })),
    files: project.files.map((file) => ({
      id: file.id,
      name: file.name,
      originalFileName: file.originalFileName,
      storageProvider: file.storageProvider,
      relativePath: inferFolderFileRelativePath(file),
      cacheKey: file.cacheKey,
      storageKey: file.storageKey,
    })),
    tracks: project.tracks.map((track) => ({
      id: track.id,
      type: track.type,
      clips: track.clips.map((clip) => ({
        id: clip.id,
        fileId: clip.fileId,
        assetId: clip.assetId,
        startTime: clip.startTime,
        offset: clip.offset,
        duration: clip.duration,
        gain: clip.gain,
        muted: clip.muted,
        fadeIn: clip.fadeIn,
        fadeOut: clip.fadeOut,
        audioProcess: clip.audioProcess,
      })),
      inserts: (track.inserts ?? []).map((insert) => ({
        id: insert.id,
        type: insert.type,
        enabled: insert.enabled,
        order: insert.order,
      })),
    })),
  });
}

function meterEntriesByTrackId(
  tracks: MeterSnapshot["tracks"],
): Array<[string, StereoMeterLevel]> {
  if (Array.isArray(tracks)) {
    return tracks
      .map((level): [string, StereoMeterLevel] | null => {
        const trackId = level.trackId ?? level.id;
        if (!trackId) return null;
        return [trackId, { left: level.left, right: level.right }];
      })
      .filter((entry): entry is [string, StereoMeterLevel] => entry !== null);
  }
  return Object.entries(tracks);
}
