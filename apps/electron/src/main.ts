import {
  app,
  BrowserWindow,
  dialog,
  ipcMain,
  protocol,
  shell,
  type IpcMainInvokeEvent,
} from "electron";
import path from "node:path";
import fs from "node:fs/promises";
import { fileURLToPath } from "node:url";
import {
  IpcChannels,
  type MessageBoxOptions,
  type MessageBoxResult,
  type OpenDialogResult,
  type AudioFileStat,
  type PickedAudioFile,
  type SaveDialogResult,
  type WaveformCacheEntryIpc,
  type FolderProjectCreateOptions,
  type FolderProjectCreateResult,
  type FolderImportAudioResult,
  type BrowseFolderResult,
  type GpuFeatureStatus,
} from "./ipc/channels.js";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const DEV_SERVER_URL =
  process.env.VITE_DEV_SERVER_URL ?? "http://localhost:5173";
const PACKAGED_APP_URL = "miko://app/index.html";

const isMac = process.platform === "darwin";
const isWin = process.platform === "win32";

protocol.registerSchemesAsPrivileged([
  {
    scheme: "miko",
    privileges: {
      standard: true,
      secure: true,
      supportFetchAPI: true,
      corsEnabled: true,
      stream: true,
    },
  },
]);

// ── Cold-start tuning ─────────────────────────────────────────────────────
// Must be set before `app.whenReady()` to take effect. These switches keep
// the renderer responsive for a DAW workload (precise timers, GPU video
// decode disabled, larger disk cache for code/preload V8 caching, etc.).

// Hardware acceleration: enabled by default for DAW-quality canvas performance.
// Set FUTUREBOARD_SW_RENDER=1 to opt into software rendering (e.g. on machines
// with broken GPU drivers that cause Chromium GPU process crashes).
if (process.env.FUTUREBOARD_SW_RENDER === "1") {
  app.disableHardwareAcceleration();
  console.log("[GPU] Hardware acceleration disabled via FUTUREBOARD_SW_RENDER=1");
}

// Larger HTTP/disk cache so renderer JS, fonts, and assets get cached after
// first launch.
app.commandLine.appendSwitch("disk-cache-size", String(256 * 1024 * 1024));

// Audio-thread friendly: don't throttle background timers; the audio worklet
// and metronome scheduler rely on accurate intervals.
app.commandLine.appendSwitch("disable-background-timer-throttling");
app.commandLine.appendSwitch("disable-renderer-backgrounding");
app.commandLine.appendSwitch("disable-backgrounding-occluded-windows");

// Canvas OOP rasterization: offloads canvas 2D rasterization to the GPU
// process, eliminating main-thread stalls on waveform and meter draws.
app.commandLine.appendSwitch(
  "enable-features",
  "CanvasOopRasterization,SharedArrayBuffer",
);

// Single-instance lock — avoid duplicate processes hammering the audio
// device when the user double-clicks the launcher.
if (!app.requestSingleInstanceLock()) {
  app.quit();
  process.exit(0);
}

// Stable per-process model on Windows so the taskbar groups our windows
// under the right icon/identity.
if (isWin) app.setAppUserModelId("org.mochilinux.studio");

// Resolve the preload path once at module-load time so window creation
// doesn't pay the cost on every `new BrowserWindow`.
const PRELOAD_PATH = path.join(__dirname, "preload.js");

function packagedRendererRoot(): string {
  return __dirname;
}

function mimeForRendererAsset(filePath: string): string {
  switch (path.extname(filePath).toLowerCase()) {
    case ".html": return "text/html";
    case ".js":
    case ".mjs": return "text/javascript";
    case ".css": return "text/css";
    case ".wasm": return "application/wasm";
    case ".json": return "application/json";
    case ".png": return "image/png";
    case ".svg": return "image/svg+xml";
    case ".ico": return "image/x-icon";
    case ".jpg":
    case ".jpeg": return "image/jpeg";
    case ".woff": return "font/woff";
    case ".woff2": return "font/woff2";
    default: return "application/octet-stream";
  }
}

type ResolvedRendererAsset = {
  filePath: string;
  shouldFallbackToIndex: boolean;
  statusCode: number;
};

function isAllowedRendererPath(relativePath: string): boolean {
  const normalized = relativePath.replace(/\\/g, "/");
  return (
    normalized === "index.html" ||
    normalized === "favicon.svg" ||
    normalized === "icons.svg" ||
    normalized.startsWith("assets/") ||
    normalized.startsWith("wasm/") ||
    normalized.startsWith("worklets/")
  );
}

function resolveRendererAsset(requestUrl: string): ResolvedRendererAsset {
  const root = packagedRendererRoot();
  const indexPath = path.join(root, "index.html");
  const url = new URL(requestUrl);
  if (url.protocol !== "miko:" || url.hostname !== "app") {
    return { filePath: indexPath, shouldFallbackToIndex: false, statusCode: 404 };
  }

  let decodedPath: string;
  try {
    decodedPath = decodeURIComponent(url.pathname);
  } catch {
    return { filePath: indexPath, shouldFallbackToIndex: false, statusCode: 400 };
  }

  const routePath = decodedPath === "/" ? "/index.html" : decodedPath;
  const relativePath = path.normalize(routePath).replace(/^([/\\])+/, "");
  const assetPath = path.resolve(root, relativePath);
  const relativeToRoot = path.relative(root, assetPath);
  const hasFileExtension = path.extname(relativePath) !== "";

  if (relativeToRoot.startsWith("..") || path.isAbsolute(relativeToRoot)) {
    return { filePath: indexPath, shouldFallbackToIndex: false, statusCode: 403 };
  }

  if (!isAllowedRendererPath(relativePath)) {
    return {
      filePath: indexPath,
      shouldFallbackToIndex: !hasFileExtension,
      statusCode: hasFileExtension ? 404 : 200,
    };
  }

  return { filePath: assetPath, shouldFallbackToIndex: !hasFileExtension, statusCode: 200 };
}

async function fileExists(filePath: string): Promise<boolean> {
  try {
    const stat = await fs.stat(filePath);
    return stat.isFile();
  } catch {
    return false;
  }
}

function registerMikoProtocol(): void {
  const distPath = packagedRendererRoot();
  console.log("[miko] appPath", app.getAppPath());
  console.log("[miko] distPath", distPath);

  protocol.handle("miko", async (request) => {
    try {
      const root = packagedRendererRoot();
      const resolved = resolveRendererAsset(request.url);
      const exists = await fileExists(resolved.filePath);
      const filePath = exists
        ? resolved.filePath
        : resolved.shouldFallbackToIndex
          ? path.join(root, "index.html")
          : resolved.filePath;
      console.log("[miko] request", request.url, "->", filePath);

      if (!exists && !resolved.shouldFallbackToIndex) {
        return new Response("Not found", {
          status: resolved.statusCode,
          headers: {
            "content-type": "text/plain",
            "cache-control": "no-cache",
          },
        });
      }

      const data = await fs.readFile(filePath);
      return new Response(data, {
        status: resolved.statusCode,
        headers: {
          "content-type": mimeForRendererAsset(filePath),
          "cache-control": "no-cache",
        },
      });
    } catch (error) {
      console.error("[miko] protocol handler error:", error);
      return new Response("Internal protocol error", {
        status: 500,
        headers: {
          "content-type": "text/plain",
          "cache-control": "no-cache",
        },
      });
    }
  });
}

function windowIconPath(): string {
  const iconFile = isMac ? "app.png" : process.platform === "win32" ? "icon.ico" : "app.png";
  if (app.isPackaged) {
    return path.join(process.resourcesPath, "icons", iconFile);
  }
  return path.join(__dirname, "..", "icons", iconFile);
}

function createWindow(): BrowserWindow {
  const win = new BrowserWindow({
    width: 1400,
    height: 900,
    minWidth: 960,
    minHeight: 600,
    title: "Futureboard Studio",
    icon: windowIconPath(),
    titleBarStyle: "hidden",
    titleBarOverlay: !isMac
      ? {
          color: "#00000000",
          symbolColor: "#c5ced9",
          height: 32,
        }
      : undefined,
    frame: false,
    backgroundColor: "#00000000",
    hasShadow: true,
    show: false,
    paintWhenInitiallyHidden: true,
    webPreferences: {
      preload: PRELOAD_PATH,
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false,
      // V8 bytecode cache for preload + renderer scripts — significantly
      // reduces parse/compile time on second+ launches.
      v8CacheOptions: "code",
      // DAW: don't throttle timers/RAF when window is hidden/occluded.
      backgroundThrottling: false,
      // Trim unused features for a leaner renderer.
      spellcheck: false,
      enableWebSQL: false,
      webgl: true,
      devTools: !app.isPackaged,
    },
  });

  // Show only after the first frame is ready — avoids the white flash and
  // makes perceived startup feel instant.
  win.once("ready-to-show", () => win.show());

  if (app.isPackaged) {
    void win.loadURL(PACKAGED_APP_URL);
  } else {
    void win.loadURL(DEV_SERVER_URL);
    win.webContents.openDevTools({ mode: "detach" });
  }

  win.webContents.on("before-input-event", (event, input) => {
    const isReloadShortcut =
      input.type === "keyDown" &&
      input.code === "KeyR" &&
      (input.control || input.meta) &&
      !input.shift &&
      !input.alt;

    if (!isReloadShortcut) return;

    event.preventDefault();
    void win.webContents.executeJavaScript(
      `window.dispatchEvent(new CustomEvent("futureboard:main-shortcut", { detail: "audio:render-selection" }));`,
      true,
    );
  });

  return win;
}

const AUDIO_EXTENSIONS = ["wav", "mp3", "flac", "ogg", "m4a", "aac"] as const;

const MIME_BY_EXT: Record<string, string> = {
  wav: "audio/wav",
  mp3: "audio/mpeg",
  flac: "audio/flac",
  ogg: "audio/ogg",
  m4a: "audio/mp4",
  aac: "audio/aac",
};

function mimeFor(filePath: string): string {
  const ext = path.extname(filePath).slice(1).toLowerCase();
  return MIME_BY_EXT[ext] ?? "application/octet-stream";
}

function isImportableAudioPath(filePath: string): boolean {
  const ext = path.extname(filePath).slice(1).toLowerCase();
  return (AUDIO_EXTENSIONS as readonly string[]).includes(ext);
}

async function readPickedAudioFile(filePath: string): Promise<PickedAudioFile | null> {
  const normalized = path.normalize(filePath);
  if (!isImportableAudioPath(normalized)) return null;
  const [buf, stat] = await Promise.all([fs.readFile(normalized), fs.stat(normalized)]);
  return {
    name: path.basename(normalized),
    mimeType: mimeFor(normalized),
    bytes: buf.buffer.slice(
      buf.byteOffset,
      buf.byteOffset + buf.byteLength,
    ) as ArrayBuffer,
    path: normalized,
    size: stat.size,
    lastModified: Math.round(stat.mtimeMs),
  };
}

async function statAudioFile(filePath: string): Promise<AudioFileStat | null> {
  const normalized = path.normalize(filePath);
  if (!isImportableAudioPath(normalized)) return null;
  const stat = await fs.stat(normalized);
  if (!stat.isFile()) return null;
  return {
    name: path.basename(normalized),
    mimeType: mimeFor(normalized),
    path: normalized,
    size: stat.size,
    lastModified: Math.round(stat.mtimeMs),
  };
}

// ── Folder-based project helpers ───────────────────────────────────────────────

const PROJECT_SUBFOLDERS = [
  path.join("Cache", "Waveform"),
  path.join("Cache", "Peaks"),
  path.join("Cache", "Processed"),
  path.join("Cache", "Analysis"),
  path.join("Media", "Audio"),
  path.join("Media", "MIDI"),
  path.join("Media", "Samples"),
  path.join("Media", "Imports"),
  path.join("Rendered", "Mixdowns"),
  path.join("Rendered", "Stems"),
  path.join("Rendered", "Bounces"),
];

function sanitizeProjectName(name: string): string {
  return name.replace(/[<>:"/\\|?*\x00-\x1f]/g, "").trim() || "Untitled Project";
}

async function ensureProjectFolders(projectRoot: string): Promise<void> {
  for (const sub of PROJECT_SUBFOLDERS) {
    await fs.mkdir(path.join(projectRoot, sub), { recursive: true });
  }
}

async function uniqueAudioDestPath(dir: string, fileName: string): Promise<string> {
  const ext = path.extname(fileName);
  const base = path.basename(fileName, ext);
  let candidate = path.join(dir, fileName);
  let counter = 2;
  for (;;) {
    try {
      await fs.access(candidate);
      candidate = path.join(dir, `${base} ${counter}${ext}`);
      counter++;
    } catch {
      return candidate;
    }
  }
}

function senderWindow(event: IpcMainInvokeEvent): BrowserWindow | null {
  return BrowserWindow.fromWebContents(event.sender);
}

function isValidString(value: unknown): value is string {
  return typeof value === "string" && value.length > 0;
}

function registerIpcHandlers(): void {
  ipcMain.handle(
    IpcChannels.FsPickAudioFiles,
    async (event): Promise<PickedAudioFile[]> => {
      const win = senderWindow(event);
      const result = win
        ? await dialog.showOpenDialog(win, {
            title: "Import Audio",
            filters: [{ name: "Audio", extensions: [...AUDIO_EXTENSIONS] }],
            properties: ["openFile", "multiSelections"],
          })
        : await dialog.showOpenDialog({
            title: "Import Audio",
            filters: [{ name: "Audio", extensions: [...AUDIO_EXTENSIONS] }],
            properties: ["openFile", "multiSelections"],
          });

      if (result.canceled || result.filePaths.length === 0) return [];

      const files: PickedAudioFile[] = [];
      for (const p of result.filePaths) {
        try {
          const picked = await readPickedAudioFile(p);
          if (picked) files.push(picked);
        } catch (err) {
          console.error("Failed reading audio file", p, err);
        }
      }
      return files;
    },
  );

  ipcMain.handle(
    IpcChannels.FsReadAudioFile,
    async (_event, filePath: unknown): Promise<PickedAudioFile | null> => {
      if (!isValidString(filePath)) return null;
      try {
        return await readPickedAudioFile(filePath);
      } catch (err) {
        console.error("Failed reading audio file", filePath, err);
        return null;
      }
    },
  );

  ipcMain.handle(
    IpcChannels.FsStatAudioFile,
    async (_event, filePath: unknown): Promise<AudioFileStat | null> => {
      if (!isValidString(filePath)) return null;
      try {
        return await statAudioFile(filePath);
      } catch {
        return null;
      }
    },
  );

  ipcMain.handle(
    IpcChannels.FsRevealInFileManager,
    async (_event, filePath: unknown): Promise<void> => {
      if (!isValidString(filePath)) return;
      shell.showItemInFolder(path.normalize(filePath));
    },
  );

  ipcMain.handle(
    IpcChannels.ProjectSaveDialog,
    async (event, suggestedName: unknown): Promise<SaveDialogResult> => {
      const win = senderWindow(event);
      const defaultPath = isValidString(suggestedName)
        ? suggestedName
        : "project.mochiproj";
      const opts: Electron.SaveDialogOptions = {
        title: "Save Project",
        defaultPath,
        filters: [{ name: "Mochi Project", extensions: ["mochiproj", "json"] }],
      };
      const res = win
        ? await dialog.showSaveDialog(win, opts)
        : await dialog.showSaveDialog(opts);
      if (res.canceled || !res.filePath) return { canceled: true };
      return { canceled: false, path: res.filePath };
    },
  );

  ipcMain.handle(
    IpcChannels.ProjectOpenDialog,
    async (event): Promise<OpenDialogResult> => {
      const win = senderWindow(event);
      const opts: Electron.OpenDialogOptions = {
        title: "Open Project",
        filters: [{ name: "Mochi Project", extensions: ["mochiproj", "json"] }],
        properties: ["openFile"],
      };
      const res = win
        ? await dialog.showOpenDialog(win, opts)
        : await dialog.showOpenDialog(opts);
      if (res.canceled || res.filePaths.length === 0) return { canceled: true };
      return { canceled: false, path: res.filePaths[0] };
    },
  );

  ipcMain.handle(
    IpcChannels.ProjectRead,
    async (_event, filePath: unknown): Promise<string | null> => {
      if (!isValidString(filePath)) return null;
      try {
        return await fs.readFile(path.normalize(filePath), "utf-8");
      } catch (err) {
        console.error("Failed reading project file", filePath, err);
        return null;
      }
    },
  );

  ipcMain.handle(
    IpcChannels.ProjectWrite,
    async (
      _event,
      filePath: unknown,
      contents: unknown,
    ): Promise<boolean> => {
      if (!isValidString(filePath) || typeof contents !== "string") return false;
      try {
        await fs.writeFile(path.normalize(filePath), contents, "utf-8");
        return true;
      } catch (err) {
        console.error("Failed writing project file", filePath, err);
        return false;
      }
    },
  );

  ipcMain.handle(
    IpcChannels.DialogMessageBox,
    async (
      event,
      options: MessageBoxOptions,
    ): Promise<MessageBoxResult> => {
      const win = senderWindow(event);
      const opts: Electron.MessageBoxOptions = {
        type: options?.type ?? "info",
        title: options?.title,
        message: options?.message ?? "",
        detail: options?.detail,
        buttons: options?.buttons,
      };
      const res = win
        ? await dialog.showMessageBox(win, opts)
        : await dialog.showMessageBox(opts);
      return { response: res.response };
    },
  );

  ipcMain.handle(
    IpcChannels.DialogErrorBox,
    (_event, title: unknown, message: unknown): void => {
      dialog.showErrorBox(
        isValidString(title) ? title : "Error",
        isValidString(message) ? message : "",
      );
    },
  );

  ipcMain.handle(IpcChannels.WindowMinimize, (event): void => {
    senderWindow(event)?.minimize();
  });

  ipcMain.handle(IpcChannels.WindowToggleMaximize, (event): void => {
    const win = senderWindow(event);
    if (!win) return;
    if (win.isMaximized()) win.unmaximize();
    else win.maximize();
  });

  ipcMain.handle(IpcChannels.WindowClose, (event): void => {
    senderWindow(event)?.close();
  });

  // ── Waveform peak cache ────────────────────────────────────────────────────
  // For global (non-folder) projects: userData/cache/waveforms/<key>.json
  // For folder projects: <projectRoot>/Cache/Peaks/<key>.json

  function globalWaveformCacheDir(): string {
    return path.join(app.getPath("userData"), "cache", "waveforms");
  }

  function cacheKeyToFilename(key: string): string {
    return key.replace(/[^a-zA-Z0-9_\-]/g, "_") + ".json";
  }

  async function resolveWaveformCachePath(key: string, projectRoot?: string | null): Promise<string> {
    if (projectRoot && isValidString(projectRoot)) {
      const dir = path.join(path.normalize(projectRoot), "Cache", "Peaks");
      await fs.mkdir(dir, { recursive: true });
      return path.join(dir, cacheKeyToFilename(key));
    }
    const dir = globalWaveformCacheDir();
    await fs.mkdir(dir, { recursive: true });
    return path.join(dir, cacheKeyToFilename(key));
  }

  ipcMain.handle(
    IpcChannels.WaveformCacheGet,
    async (_event, key: unknown, projectRoot?: unknown): Promise<WaveformCacheEntryIpc | null> => {
      if (!isValidString(key)) return null;
      try {
        const filePath = await resolveWaveformCachePath(key, isValidString(projectRoot) ? projectRoot : null);
        const raw = await fs.readFile(filePath, "utf-8");
        return JSON.parse(raw) as WaveformCacheEntryIpc;
      } catch {
        return null;
      }
    },
  );

  ipcMain.handle(
    IpcChannels.WaveformCacheSet,
    async (_event, key: unknown, entry: unknown, projectRoot?: unknown): Promise<void> => {
      if (!isValidString(key) || typeof entry !== "object" || entry === null) return;
      try {
        const filePath = await resolveWaveformCachePath(key, isValidString(projectRoot) ? projectRoot : null);
        await fs.writeFile(filePath, JSON.stringify(entry), "utf-8");
      } catch (e) {
        console.warn("[WaveformCache] write failed:", e);
      }
    },
  );

  ipcMain.handle(
    IpcChannels.WaveformCacheDelete,
    async (_event, key: unknown, projectRoot?: unknown): Promise<void> => {
      if (!isValidString(key)) return;
      try {
        const filePath = await resolveWaveformCachePath(key, isValidString(projectRoot) ? projectRoot : null);
        await fs.unlink(filePath);
      } catch {
        // File may not exist — ignore
      }
    },
  );

  ipcMain.handle(
    IpcChannels.WaveformCacheClear,
    async (): Promise<void> => {
      // Only clears the global userData cache; project-folder cache is managed
      // at the project level and cleared by deleting Cache/Peaks manually.
      try {
        const dir = globalWaveformCacheDir();
        const entries = await fs.readdir(dir);
        await Promise.all(
          entries
            .filter((f) => f.endsWith(".json"))
            .map((f) => fs.unlink(path.join(dir, f)).catch(() => {})),
        );
      } catch {
        // Directory may not exist — ignore
      }
    },
  );

  // ── System / diagnostics ────────────────────────────────────────────────────

  ipcMain.handle(
    IpcChannels.SysGetGpuInfo,
    (): GpuFeatureStatus => {
      let features: Record<string, string> = {};
      let gpuDescription: string | null = null;
      try {
        features = app.getGPUFeatureStatus() as unknown as Record<string, string>;
      } catch { /* ignore */ }
      return {
        hardwareAccelerationEnabled: process.env.FUTUREBOARD_SW_RENDER !== "1",
        features,
        gpuDescription,
        electronVersion: process.versions.electron ?? "unknown",
        chromeVersion: process.versions.chrome ?? "unknown",
      };
    },
  );

  // ── Folder-based project operations ────────────────────────────────────────

  ipcMain.handle(
    IpcChannels.FsEnsureProjectFolders,
    async (_event, projectRoot: unknown): Promise<boolean> => {
      if (!isValidString(projectRoot)) return false;
      try {
        await ensureProjectFolders(path.normalize(projectRoot));
        return true;
      } catch (e) {
        console.error("[FolderProject] ensureProjectFolders failed:", e);
        return false;
      }
    },
  );

  ipcMain.handle(
    IpcChannels.ProjectFolderBrowseLocation,
    async (event): Promise<BrowseFolderResult> => {
      const win = senderWindow(event);
      const opts: Electron.OpenDialogOptions = {
        title: "Choose Project Location",
        properties: ["openDirectory", "createDirectory"],
      };
      const res = win
        ? await dialog.showOpenDialog(win, opts)
        : await dialog.showOpenDialog(opts);
      if (res.canceled || res.filePaths.length === 0) return { canceled: true };
      return { canceled: false, folderPath: res.filePaths[0] };
    },
  );

  ipcMain.handle(
    IpcChannels.ProjectFolderCreate,
    async (_event, options: unknown): Promise<FolderProjectCreateResult | null> => {
      if (typeof options !== "object" || options === null) return null;
      const { name, location } = options as FolderProjectCreateOptions;
      if (!isValidString(name) || !isValidString(location)) return null;

      const safeName = sanitizeProjectName(name);
      const normalizedLocation = path.normalize(location);
      const projectRoot = path.join(normalizedLocation, safeName);

      // Path traversal guard
      const rel = path.relative(normalizedLocation, projectRoot);
      if (rel.startsWith("..") || path.isAbsolute(rel)) return null;

      try {
        await ensureProjectFolders(projectRoot);
        const projectFilePath = path.join(projectRoot, `${safeName}.mochiproj`);
        return { projectRoot, projectFilePath };
      } catch (e) {
        console.error("[FolderProject] create failed:", e);
        return null;
      }
    },
  );

  ipcMain.handle(
    IpcChannels.ProjectFolderSave,
    async (_event, projectRoot: unknown, contents: unknown): Promise<boolean> => {
      if (!isValidString(projectRoot) || typeof contents !== "string") return false;
      const normalizedRoot = path.normalize(projectRoot);
      const projectName = path.basename(normalizedRoot);
      const filePath = path.join(normalizedRoot, `${projectName}.mochiproj`);
      const tmpPath = `${filePath}.tmp`;
      try {
        await fs.writeFile(tmpPath, contents, "utf-8");
        await fs.rename(tmpPath, filePath);
        return true;
      } catch (e) {
        console.error("[FolderProject] save failed:", e);
        try { await fs.unlink(tmpPath); } catch { /* ignore */ }
        return false;
      }
    },
  );

  ipcMain.handle(
    IpcChannels.ProjectFolderOpenFile,
    async (_event, filePath: unknown): Promise<string | null> => {
      if (!isValidString(filePath)) return null;
      try {
        return await fs.readFile(path.normalize(filePath), "utf-8");
      } catch (e) {
        console.error("[FolderProject] openFile failed:", e);
        return null;
      }
    },
  );

  ipcMain.handle(
    IpcChannels.ProjectFolderImportAudio,
    async (
      _event,
      projectRoot: unknown,
      sourcePath: unknown,
    ): Promise<FolderImportAudioResult | null> => {
      if (!isValidString(projectRoot) || !isValidString(sourcePath)) return null;
      const normalizedRoot = path.normalize(projectRoot);
      const normalizedSource = path.normalize(sourcePath);

      if (!isImportableAudioPath(normalizedSource)) return null;

      const mediaAudioDir = path.join(normalizedRoot, "Media", "Audio");
      try {
        await fs.mkdir(mediaAudioDir, { recursive: true });
        const destPath = await uniqueAudioDestPath(mediaAudioDir, path.basename(normalizedSource));

        // Path traversal guard
        if (!destPath.startsWith(normalizedRoot)) return null;

        await fs.copyFile(normalizedSource, destPath);
        const stat = await fs.stat(destPath);
        const relPath = path.relative(normalizedRoot, destPath).replace(/\\/g, "/");

        return {
          relativePath: relPath,
          absolutePath: destPath,
          name: path.basename(destPath),
          size: stat.size,
          lastModified: Math.round(stat.mtimeMs),
        };
      } catch (e) {
        console.error("[FolderProject] importAudio failed:", e);
        return null;
      }
    },
  );
}

// Register IPC handlers eagerly so they are guaranteed to be live before the
// renderer's preload script issues its first `ipcRenderer.invoke`.
registerIpcHandlers();

app.on("second-instance", () => {
  const [win] = BrowserWindow.getAllWindows();
  if (!win) return;
  if (win.isMinimized()) win.restore();
  win.focus();
});

app.whenReady().then(() => {
  // Log GPU diagnostics so we can verify hardware acceleration is active.
  try {
    const gpuFeatures = app.getGPUFeatureStatus();
    console.log("[GPU] Feature status:", JSON.stringify(gpuFeatures, null, 2));
    app.getGPUInfo("basic").then((info) => {
      const gpuInfo = info as Record<string, unknown>;
      const gpuList = Array.isArray(gpuInfo.gpuDevice)
        ? (gpuInfo.gpuDevice as Array<{ description?: string; vendorId?: number; deviceId?: number }>)
        : [];
      const primary = gpuList[0];
      console.log(
        "[GPU] Primary device:",
        primary
          ? `${primary.description ?? "unknown"} (vendor=${primary.vendorId?.toString(16) ?? "?"} device=${primary.deviceId?.toString(16) ?? "?"})`
          : "none reported",
      );
      console.log("[GPU] HW acceleration enabled:", process.env.FUTUREBOARD_SW_RENDER !== "1");
    }).catch((e) => console.warn("[GPU] getGPUInfo failed:", e));
  } catch (e) {
    console.warn("[GPU] getGPUFeatureStatus failed:", e);
  }

  registerMikoProtocol();
  createWindow();

  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      createWindow();
    }
  });
});

app.on("window-all-closed", () => {
  if (!isMac) app.quit();
});
