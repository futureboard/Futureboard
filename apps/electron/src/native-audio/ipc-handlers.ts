/**
 * registerSphereAudioHandlers — wires IPC channels to the native Rust addon.
 *
 * Call once from Electron main after app.whenReady().
 * Uses `SphereAudioNative` (direct N-API addon) instead of a child process.
 *
 * If the addon fails to load (dev machine without a Rust build, CI without
 * native deps, etc.) every handler returns a safe "unavailable" response
 * rather than throwing, so the rest of the app continues working.
 */
import { app, ipcMain } from "electron";
import fs from "fs";
import path from "path";
import { IpcChannels } from "../ipc/channels.js";
import { sphereAudioNative } from "./SphereAudioNative.js";
import type {
  SphereDeviceOpenConfig,
  SphereTransportState,
  SphereDauxConfig,
  SphereStartRecordingConfig,
} from "../ipc/channels.js";

type NativeSnapshotLike = {
  projectId?: string;
  projectRoot?: string | null;
  tracks?: unknown[];
  assets?: Array<{
    id?: string;
    type?: string;
    name?: string;
    relativePath?: string | null;
    missing?: boolean;
  }>;
  files?: Array<{
    id?: string;
    name?: string;
    originalFileName?: string;
    storageProvider?: string;
    relativePath?: string | null;
    cacheKey?: string | null;
    storageKey?: string | null;
  }>;
  clips?: Array<{
    id?: string;
    trackId?: string;
    assetId?: string;
    fileId?: string;
    sourceId?: string;
    importId?: string;
    relativePath?: string | null;
    mediaPath?: string | null;
  }>;
};

function normalizePathForLog(p: string): string {
  return p.replace(/\\/g, "/");
}

function isRealAbsolutePath(p: string | null | undefined): p is string {
  if (!p) return false;
  if (p.startsWith("miko://") || p.startsWith("file://") || p.startsWith("blob:") || p.startsWith("audio:")) {
    return false;
  }
  return path.isAbsolute(p);
}

function resolveUnderRoot(projectRoot: string, relativePath: string): string | null {
  if (!relativePath || path.isAbsolute(relativePath)) return null;
  const root = path.resolve(projectRoot);
  const resolved = path.resolve(root, relativePath);
  const rel = path.relative(root, resolved);
  if (rel === "" || (!rel.startsWith("..") && !path.isAbsolute(rel))) {
    return resolved;
  }
  return null;
}

function mediaRelativePathFromName(name: string | null | undefined): string | null {
  const clean = name?.trim();
  if (!clean) return null;
  const basename = clean.replace(/\\/g, "/").split("/").filter(Boolean).pop();
  return basename ? `Media/Audio/${basename}` : null;
}

function inferFileRelativePath(file: NonNullable<NativeSnapshotLike["files"]>[number] | undefined): string | null {
  if (!file) return null;
  if (file.relativePath) return file.relativePath;
  if (file.storageProvider !== "project-folder") return null;
  return mediaRelativePathFromName(file.name) ?? mediaRelativePathFromName(file.originalFileName);
}

function standardMediaPathCandidates(file: NonNullable<NativeSnapshotLike["files"]>[number] | undefined): string[] {
  if (!file) return [];
  const names = [file.name, file.originalFileName]
    .map((name) => name?.trim())
    .filter((name): name is string => Boolean(name))
    .map((name) => name.replace(/\\/g, "/").split("/").filter(Boolean).pop())
    .filter((name): name is string => Boolean(name));
  const uniqueNames = [...new Set(names)];
  if (uniqueNames.length === 0) return [];

  const roots = ["downloads", "music", "desktop", "documents"]
    .map((key) => {
      try {
        return app.getPath(key as Parameters<typeof app.getPath>[0]);
      } catch {
        return null;
      }
    })
    .filter((root): root is string => Boolean(root));

  return roots.flatMap((root) => uniqueNames.map((name) => path.join(root, name)));
}

function firstExistingFile(paths: string[]): string | null {
  for (const candidate of paths) {
    try {
      const stat = fs.existsSync(candidate) ? fs.statSync(candidate) : null;
      if (stat?.isFile()) return candidate;
    } catch {
      // ignore inaccessible candidates
    }
  }
  return null;
}

function resolveProjectMediaPaths(snapshot: unknown): unknown {
  const s = snapshot as NativeSnapshotLike;
  if (!s || !Array.isArray(s.clips)) return snapshot;

  const projectRoot = typeof s.projectRoot === "string" && s.projectRoot.trim()
    ? path.resolve(s.projectRoot)
    : null;
  const assetsById = new Map<string, NonNullable<NativeSnapshotLike["assets"]>[number]>();
  for (const asset of s.assets ?? []) {
    if (asset?.id) assetsById.set(asset.id, asset);
  }
  const filesById = new Map<string, NonNullable<NativeSnapshotLike["files"]>[number]>();
  for (const file of s.files ?? []) {
    if (file?.id) filesById.set(file.id, file);
  }

  console.log(`[NativeSnapshot] projectRoot=${projectRoot ? normalizePathForLog(projectRoot) : "null"}`);
  console.log(`[NativeSnapshot] assets=${assetsById.size} files=${filesById.size}`);

  let withPath = 0;
  let missingPath = 0;
  const resolvedClips = s.clips.map((clip) => {
    const clipId = clip.id ?? "?";
    const assetId = clip.assetId ?? clip.fileId ?? clip.sourceId ?? clip.importId ?? "";
    const asset = assetId ? assetsById.get(assetId) : undefined;
    const file = assetId ? filesById.get(assetId) : undefined;
    const relativePath = clip.relativePath ?? asset?.relativePath ?? inferFileRelativePath(file);

    let mediaPath: string | null = null;
    let exists = false;
    let reason = "";

    if (projectRoot && relativePath) {
      const resolved = resolveUnderRoot(projectRoot, relativePath);
      if (resolved) {
        mediaPath = resolved;
        exists = fs.existsSync(resolved);
        if (!exists) reason = "resolved file does not exist";
      } else {
        reason = "relativePath escapes projectRoot or is absolute";
      }
    }
    if (!exists && isRealAbsolutePath(clip.mediaPath)) {
      const resolved = path.resolve(clip.mediaPath);
      mediaPath = resolved;
      exists = fs.existsSync(resolved);
      if (!exists) reason = "absolute mediaPath does not exist";
    }
    if (!exists && isRealAbsolutePath(file?.cacheKey)) {
      const resolved = path.resolve(file.cacheKey);
      mediaPath = resolved;
      exists = fs.existsSync(resolved);
      if (!exists) reason = "file cacheKey does not exist";
    }
    if (!exists && isRealAbsolutePath(file?.storageKey)) {
      const resolved = path.resolve(file.storageKey);
      mediaPath = resolved;
      exists = fs.existsSync(resolved);
      if (!exists) reason = "file storageKey does not exist";
    }
    if (!exists && file?.storageProvider === "indexeddb") {
      const resolved = firstExistingFile(standardMediaPathCandidates(file));
      if (resolved) {
        mediaPath = resolved;
        exists = true;
        reason = "";
      }
    }
    if (!mediaPath) {
      reason = projectRoot ? "missing relativePath/mediaPath/file path" : "missing projectRoot and file path";
    }

    if (mediaPath && exists) {
      withPath++;
    } else {
      missingPath++;
      mediaPath = null;
    }

    console.log(
      `[NativeSnapshot] clip=${clipId} assetId=${assetId || "?"} relativePath=${relativePath ?? ""} ` +
      `mediaPath=${mediaPath ? normalizePathForLog(mediaPath) : "null"} exists=${exists} ` +
      `fileProvider=${file?.storageProvider ?? "?"}`,
    );
    if (!exists) {
      console.warn(
        `[NativeSnapshot] skipped clip=${clipId} track=${clip.trackId ?? "?"} asset=${assetId || "?"} reason=${reason}`,
      );
    }

    return {
      ...clip,
      assetId,
      relativePath,
      mediaPath: mediaPath ? normalizePathForLog(mediaPath) : null,
    };
  });

  if (s.clips.length > 0 && withPath === 0) {
    console.warn("[NativeSnapshot] WARNING: all clips have null/empty mediaPath after main-process resolution");
  }

  return {
    ...s,
    projectRoot: projectRoot ? normalizePathForLog(projectRoot) : s.projectRoot,
    clips: resolvedClips,
    __mediaPathStats: {
      clipCount: s.clips.length,
      withPath,
      missingPath,
    },
  };
}

export function registerSphereAudioHandlers(_appDir: string): void {
  const svc = sphereAudioNative;

  // Try to initialise the native addon on first registration.
  const available = svc.initialize();
  if (!available) {
    console.warn(
      "[SphereAudio] Native addon unavailable — IPC handlers registered in degraded mode"
    );
  } else {
    // Auto-open the system default output device and start the audio stream.
    // This runs immediately so the engine is "running" before the renderer
    // ever queries getStatus(), making the settings panel show the correct state.
    try {
      svc.openDevice({});   // omitted config fields → system default device/config
      svc.start();          // stream.play() — silent until transport play or test tone
      console.log("[SphereAudio] Auto-started on default output device");
    } catch (e) {
      console.warn("[SphereAudio] Auto-start failed (non-fatal):", e);
    }
  }

  // ── Status / version ───────────────────────────────────────────────────────

  ipcMain.handle(IpcChannels.SphereAudioGetStatus, () => {
    return svc.getStatus();
  });

  ipcMain.handle(IpcChannels.SphereAudioGetVersion, () => {
    return svc.getVersion();
  });

  // ── Device enumeration ─────────────────────────────────────────────────────

  ipcMain.handle(IpcChannels.SphereAudioListInputDevices, () => {
    return svc.listInputDevices();
  });

  ipcMain.handle(IpcChannels.SphereAudioListOutputDevices, () => {
    return svc.listOutputDevices();
  });

  // ── Stream lifecycle ───────────────────────────────────────────────────────

  ipcMain.handle(
    IpcChannels.SphereAudioOpenDevice,
    (_event, config: SphereDeviceOpenConfig) => {
      svc.openDevice(config); // throws if addon unavailable
    },
  );

  ipcMain.handle(IpcChannels.SphereAudioCloseDevice, () => {
    svc.closeDevice();
  });

  ipcMain.handle(IpcChannels.SphereAudioStart, () => {
    svc.start(); // opens cpal stream + begins audio output
  });

  ipcMain.handle(IpcChannels.SphereAudioStop, () => {
    svc.stop();
  });

  ipcMain.handle(
    IpcChannels.SphereAudioSetTestTone,
    (_event, enabled: boolean, frequency: number) => {
      svc.setTestTone(enabled, frequency);
    },
  );

  // ── Transport ──────────────────────────────────────────────────────────────
  // The old IPC shape used a `SphereTransportState` bag with optional fields.
  // Map it to the individual engine calls.

  ipcMain.handle(
    IpcChannels.SphereAudioSetTransport,
    (_event, state: SphereTransportState) => {
      try {
        console.log("[SphereAudio IPC] setTransportState →", JSON.stringify(state));
        if (typeof state.positionSeconds === "number") {
          svc.seek(state.positionSeconds);
        }
        if (state.playing === true) {
          console.log("[SphereAudio IPC] → play()");
          svc.play();
        }
        if (state.playing === false) {
          console.log("[SphereAudio IPC] → pause()");
          svc.pause();
        }
      } catch (e) {
        // Surface transport errors to the renderer so they appear in DevTools.
        console.error("[SphereAudio IPC] setTransport error:", e);
        throw e;
      }
    },
  );

  ipcMain.handle(IpcChannels.SphereAudioGetTransport, () => {
    const st = svc.getStatus();
    return {
      playing:         st.transportPlaying,
      positionSeconds: st.positionSeconds,
    };
  });

  // ── Param updates ──────────────────────────────────────────────────────────

  ipcMain.handle(
    IpcChannels.SphereAudioUpdateTrackParam,
    (_event, trackId: string, paramId: string, value: unknown) => {
      if (paramId.startsWith("__")) {
        console.warn(`[SphereAudio IPC] blocked invalid updateTrackParam ${trackId}.${paramId}`);
        return;
      }
      svc.updateTrackParam(trackId, paramId, value);
    },
  );

  ipcMain.handle(
    IpcChannels.SphereAudioUpdateInsertParam,
    (_event, trackId: string, insertId: string, paramId: string, value: unknown) => {
      svc.updateInsertParam(trackId, insertId, paramId, value);
    },
  );

  ipcMain.handle(
    IpcChannels.SphereAudioOpenInsertEditor,
    (
      _event,
      options: {
        trackId?: string;
        insertId?: string;
        windowId?: string;
        title?: string;
        width?: number;
        height?: number;
      },
    ) => {
      const trackId = typeof options?.trackId === "string" ? options.trackId : "";
      const insertId = typeof options?.insertId === "string" ? options.insertId : "";
      if (!trackId || !insertId) return null;
      return svc.openInsertEditor(
        trackId,
        insertId,
        typeof options.windowId === "string" ? options.windowId : `plugin-editor:${trackId}:${insertId}`,
        typeof options.title === "string" ? options.title : "Plugin Editor",
        typeof options.width === "number" ? options.width : 820,
        typeof options.height === "number" ? options.height : 560,
      );
    },
  );

  ipcMain.handle(
    IpcChannels.SphereAudioCloseInsertEditor,
    (_event, trackId: string, insertId: string) => {
      svc.closeInsertEditor(trackId, insertId);
    },
  );

  ipcMain.handle(
    IpcChannels.SphereAudioFocusInsertEditor,
    (_event, trackId: string, insertId: string) => {
      return svc.focusInsertEditor(trackId, insertId);
    },
  );

  // ── Project snapshot ───────────────────────────────────────────────────────

  ipcMain.handle(
    IpcChannels.SphereAudioLoadProject,
    (_event, snapshot: unknown) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const resolvedSnapshot = resolveProjectMediaPaths(snapshot);
      const s = resolvedSnapshot as any;
      const trackCount = Array.isArray(s?.tracks) ? s.tracks.length : "?";
      const clipCount  = Array.isArray(s?.clips)  ? s.clips.length  : "?";
      const withPath   = Array.isArray(s?.clips)
        ? s.clips.filter((c: { mediaPath?: string }) => c.mediaPath).length
        : "?";
      const missingPath = Array.isArray(s?.clips) ? s.clips.length - Number(withPath) : "?";
      const nativeInsertCount = Array.isArray(s?.tracks)
        ? s.tracks.reduce((count: number, track: { inserts?: Array<{ type?: string }> }) => (
            count + (Array.isArray(track.inserts)
              ? track.inserts.filter((insert) => insert.type === "native-plugin").length
              : 0)
          ), 0)
        : "?";
      console.log(
        `[SphereAudio IPC] loadProject("${s?.projectId ?? "?"}") — ${trackCount} tracks, ${clipCount} clips (${withPath} with paths, ${missingPath} missing paths), native inserts=${nativeInsertCount}`,
      );
      svc.loadProject(resolvedSnapshot);
    },
  );

  ipcMain.handle(
    IpcChannels.SphereAudioUpdateClip,
    (_event, clipId: string, patch: unknown) => {
      svc.updateClip(clipId, patch);
    },
  );

  // ── Meters ─────────────────────────────────────────────────────────────────

  ipcMain.handle(IpcChannels.SphereAudioGetMeters, () => {
    return svc.getMeters();
  });

  ipcMain.handle(IpcChannels.SphereAudioGetDebugInfo, () => {
    return svc.getDebugInfo();
  });

  // ── DAUx backend selection ─────────────────────────────────────────────────

  ipcMain.handle(IpcChannels.SphereAudioListDauxBackends, () => {
    return svc.listDauxBackends();
  });

  ipcMain.handle(
    IpcChannels.SphereAudioOpenDaux,
    (_event, config: SphereDauxConfig) => {
      svc.openDaux(config);
    },
  );

  ipcMain.handle(
    IpcChannels.SphereAudioOpenDauxSafe,
    (_event, config: SphereDauxConfig) => {
      svc.openDauxSafe(config);
    },
  );

  ipcMain.handle(IpcChannels.SphereAudioGetDauxStatus, () => {
    return svc.getDauxStatus();
  });

  // ── Recording ──────────────────────────────────────────────────────────────

  ipcMain.handle(
    IpcChannels.SphereAudioStartRecording,
    (_event, config: SphereStartRecordingConfig) => {
      svc.startRecording(config);
    },
  );

  ipcMain.handle(IpcChannels.SphereAudioStopRecording, () => {
    return svc.stopRecording();
  });

  ipcMain.handle(IpcChannels.SphereAudioGetRecordingStatus, () => {
    return svc.getRecordingStatus();
  });

  console.log(
    `[SphereAudio] IPC handlers registered (addon ${available ? "✓ loaded" : "✗ unavailable"})`
  );
}
