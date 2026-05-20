import {
  app,
  BrowserWindow,
  dialog,
  ipcMain,
  Menu,
  nativeImage,
  protocol,
  shell,
  type MenuItemConstructorOptions,
  type IpcMainInvokeEvent,
} from "electron";
import path from "node:path";
import fs from "node:fs/promises";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { randomUUID } from "node:crypto";
import { fileURLToPath } from "node:url";
import {
  IpcChannels,
  type MessageBoxOptions,
  type MessageBoxResult,
  type OpenDialogResult,
  type AudioFileStat,
  type BrowserFileEntry,
  type BrowserIndexStatus,
  type BrowserRootEntry,
  type PickedAudioFile,
  type SaveDialogResult,
  type WaveformCacheEntryIpc,
  type WavPeakResult,
  type FolderProjectCreateOptions,
  type FolderProjectCreateResult,
  type FolderImportAudioResult,
  type BrowseFolderResult,
  type GpuFeatureStatus,
  type ElectronPersistedSettings,
  type AudioPluginHostStatus,
  type AudioPluginRegistryEntry,
  type AudioPluginScanProgressEvent,
  type AudioPluginScanResult,
  type ExternalWindowConfig,
  type FloatingWindowOpenRequest,
  type FloatingWindowMixerUpdateRequest,
} from "./ipc/channels.js";
import { APP_MENUS, type AppMenuGroup, type AppMenuItem } from "./generated/menuItems.js";
import { initAutoUpdater } from "./updater.js";
import { getFloatingWindowManager } from "./floating-window-manager.js";
import { pluginHostNative } from "./native-plugin/PluginHostNative.js";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);

const DEV_SERVER_URL =
  process.env.VITE_DEV_SERVER_URL ?? "http://localhost:5173";
const PACKAGED_APP_URL = "miko://app/index.html";

const isMac = process.platform === "darwin";
const isWin = process.platform === "win32";
const closeAllowed = new WeakSet<BrowserWindow>();
const externalWindows = new Map<string, BrowserWindow>();

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

// ── GPU mode ──────────────────────────────────────────────────────────────
// FUTUREBOARD_GPU_MODE=auto (default) | force | software
//
//   auto     — let Chromium/Electron decide; no extra flags, no disabling.
//   force    — bypass blocklist, enable rasterization + zero-copy + 2D canvas.
//   software — disable GPU entirely (safe mode for broken drivers).
//
// Software rendering must be an explicit opt-in, not the packaged default.

type GpuMode = "auto" | "force" | "software";

function electronSettingsFilePath(): string {
  return path.join(app.getPath("userData"), "futureboard-settings.json");
}

function readPersistedGpuMode(): GpuMode | null {
  try {
    const raw = readFileSync(electronSettingsFilePath(), "utf-8");
    const parsed = JSON.parse(raw) as Partial<ElectronPersistedSettings>;
    if (parsed.graphicRenderingMode === "force") return "force";
    if (parsed.graphicRenderingMode === "auto") return "auto";
    if (parsed.graphicRenderingMode === "software") return "software";
  } catch {
    // file absent or malformed — silent fallback
  }
  return null;
}

function resolveGpuMode(): GpuMode {
  // Env var takes priority (dev/CI overrides)
  const envRaw = process.env.FUTUREBOARD_GPU_MODE?.toLowerCase().trim();
  if (envRaw === "force" || envRaw === "software" || envRaw === "auto") return envRaw as GpuMode;
  // Fall back to persisted settings.json
  const persisted = readPersistedGpuMode();
  if (persisted) return persisted;
  return "auto";
}

const GPU_MODE: GpuMode = resolveGpuMode();

if (GPU_MODE === "software") {
  app.disableHardwareAcceleration();
  app.commandLine.appendSwitch("disable-gpu");
  console.log("[GPU] Software rendering mode (FUTUREBOARD_GPU_MODE=software)");
} else {
  if (isWin) {
    // D3D12 is still unstable on some Electron/Chromium + NVIDIA driver
    // combinations and can produce white WebGL surfaces. D3D11 keeps ANGLE on
    // the hardware path while staying compatible with Chromium compositing.
    app.commandLine.appendSwitch("use-angle", "d3d11");
    console.log("[GPU] Windows ANGLE backend requested: D3D11");
  }
}

if (GPU_MODE === "force") {
  app.commandLine.appendSwitch("ignore-gpu-blocklist");
  app.commandLine.appendSwitch("enable-gpu-rasterization");
  app.commandLine.appendSwitch("enable-accelerated-2d-canvas");
  app.commandLine.appendSwitch("enable-webgl");
  app.disableDomainBlockingFor3DAPIs();
  console.log("[GPU] Force hardware mode (FUTUREBOARD_GPU_MODE=force; ANGLE=D3D11 on Windows)");
} else if (GPU_MODE === "auto") {
  console.log("[GPU] Auto mode — GPU acceleration left to Electron defaults");
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
// HighRefreshRateAnimation: tells Chromium's frame scheduler to honour the
// display's native vsync interval rather than defaulting to 60 Hz.
app.commandLine.appendSwitch(
  "enable-features",
  "CanvasOopRasterization,SharedArrayBuffer,HighRefreshRateAnimation",
);

// Remove Chromium's artificial 60 fps renderer cap so requestAnimationFrame
// follows the actual display refresh rate (100 Hz, 120 Hz, 144 Hz, etc.).
// Without this flag Chromium clamps RAF to 60 fps regardless of the monitor.
app.commandLine.appendSwitch("disable-frame-rate-limit");

// Single-instance lock — avoid duplicate processes hammering the audio
// device when the user double-clicks the launcher.
if (!app.requestSingleInstanceLock()) {
  app.quit();
  process.exit(0);
}

// Stable per-process model on Windows so the taskbar groups our windows
// under the right icon/identity.
if (isWin) app.setAppUserModelId("org.futureboard.studio");

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

function assetsPath(...parts: string[]): string {
  if (app.isPackaged) {
    return path.join(process.resourcesPath, "assets", ...parts);
  }
  return path.join(__dirname, "..", "assets", ...parts);
}

function createSplashWindow(): BrowserWindow {
  const imgPath = assetsPath("splash.png");
  let w = 600;
  let h = 340;
  try {
    const img = nativeImage.createFromPath(imgPath);
    const size = img.getSize();
    if (size.width > 0 && size.height > 0) {
      // splash.png is authored as a 2x bitmap. BrowserWindow dimensions are
      // device-independent pixels, so display it at 1x CSS/DPI size.
      w = Math.round(size.width / 2);
      h = Math.round(size.height / 2);
    }
  } catch {
    // fall back to defaults
  }

  const splash = new BrowserWindow({
    width: w,
    height: h,
    center: true,
    frame: false,
    transparent: true,
    resizable: false,
    skipTaskbar: true,
    alwaysOnTop: true,
    hasShadow: true,
    webPreferences: { nodeIntegration: false, contextIsolation: true },
  });

  void splash.loadFile(assetsPath("splash.html"));
  return splash;
}

function sendCommandToFocusedWindow(commandId: string): void {
  BrowserWindow.getFocusedWindow()?.webContents.send("app-command", commandId);
}

function electronAccelerator(accelerator?: string): string | undefined {
  if (!accelerator) return undefined;
  return accelerator
    .replace(/\bCtrl\+/g, "CommandOrControl+")
    .replace(/\bEsc\b/g, "Escape")
    .replace(/\bArrowLeft\b/g, "Left")
    .replace(/\bArrowRight\b/g, "Right")
    .replace(/\bArrowUp\b/g, "Up")
    .replace(/\bArrowDown\b/g, "Down");
}

function standardRoleForItem(item: Extract<AppMenuItem, { type?: "item" }>): MenuItemConstructorOptions["role"] | undefined {
  if (item.role === "quit" || item.role === "minimize") return item.role;
  switch (item.action) {
    case "edit:cut": return "cut";
    case "edit:copy": return "copy";
    case "edit:paste": return "paste";
    case "edit:select-all": return "selectAll";
    case "window:minimize": return "minimize";
    case "window:toggle-fullscreen": return "togglefullscreen";
    default: return undefined;
  }
}

function buildNativeMenuItem(item: AppMenuItem): MenuItemConstructorOptions {
  if (item.type === "separator") return { type: "separator" };
  if (item.type === "submenu") {
    return {
      label: item.label,
      enabled: item.enabled ?? true,
      submenu: item.children.map(buildNativeMenuItem),
    };
  }

  const role = standardRoleForItem(item);
  const options: MenuItemConstructorOptions = {
    label: item.label,
    enabled: item.enabled ?? true,
    accelerator: electronAccelerator(item.accelerator),
  };
  if (item.checked != null) {
    options.type = "checkbox";
    options.checked = item.checked;
  }
  if (role) {
    options.role = role;
  } else if (item.action) {
    options.click = () => sendCommandToFocusedWindow(item.action!);
  }
  return options;
}

function buildNativeMenuGroup(group: AppMenuGroup): MenuItemConstructorOptions {
  return {
    label: group.label,
    submenu: group.children.map(buildNativeMenuItem),
  };
}

function buildMacAppMenu(): MenuItemConstructorOptions {
  return {
    label: app.name || "Futureboard Studio",
    submenu: [
      { label: "About Futureboard Studio", click: () => sendCommandToFocusedWindow("app:about") },
      { type: "separator" },
      {
        label: "Preferences...",
        accelerator: "CommandOrControl+,",
        click: () => sendCommandToFocusedWindow("app:preferences"),
      },
      {
        label: "Keyboard Shortcuts",
        accelerator: "CommandOrControl+/",
        click: () => sendCommandToFocusedWindow("help:keyboard-shortcuts"),
      },
      { type: "separator" },
      { role: "services" },
      { type: "separator" },
      { role: "hide" },
      { role: "hideOthers" },
      { role: "unhide" },
      { type: "separator" },
      { role: "quit" },
    ],
  };
}

function installApplicationMenu(): void {
  if (!isMac) {
    Menu.setApplicationMenu(null);
    return;
  }
  Menu.setApplicationMenu(Menu.buildFromTemplate([
    buildMacAppMenu(),
    ...APP_MENUS.map(buildNativeMenuGroup),
  ]));
}

function createWindow(showOnReady = true): BrowserWindow {
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
          height: 35,
        }
      : undefined,
    frame: false,
    backgroundColor: "#171B22",
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

  if (showOnReady) {
    win.once("ready-to-show", () => win.show());
  }

  win.on("close", (event) => {
    if (closeAllowed.has(win)) {
      closeAllowed.delete(win);
      return;
    }
    if (win.webContents.isDestroyed()) return;
    event.preventDefault();
    win.webContents.send("app-command", "app:request-close");
  });

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
const FACTORY_LIBRARY_FOLDERS = ["Presets", "Samples", "Loops", "Templates"] as const;

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

function factoryLibraryRoot(): string {
  return path.join(app.getPath("documents"), "Futureboard Studio");
}

async function ensureFactoryLibraryFolders(): Promise<BrowserRootEntry[]> {
  const root = factoryLibraryRoot();
  await fs.mkdir(root, { recursive: true });
  await Promise.all(
    FACTORY_LIBRARY_FOLDERS.map((folder) => fs.mkdir(path.join(root, folder), { recursive: true })),
  );
  return [
    { id: "factory", name: "Futureboard Studio", path: root, kind: "factory" },
    ...FACTORY_LIBRARY_FOLDERS.map((folder) => ({
      id: `factory:${folder.toLowerCase()}`,
      name: folder,
      path: path.join(root, folder),
      kind: "factory-folder" as const,
    })),
  ];
}

async function mountedDriveRoots(): Promise<BrowserRootEntry[]> {
  if (!isWin) return [{ id: "root", name: "/", path: "/", kind: "drive" }];
  const byPath = new Map<string, BrowserRootEntry>();
  const addDrive = (drivePath?: string | null) => {
    if (!drivePath) return;
    const match = path.resolve(drivePath).match(/^([A-Za-z]:)\\/);
    if (!match) return;
    const name = match[1].toUpperCase();
    byPath.set(`${name}\\`, { id: `drive:${name[0]}`, name, path: `${name}\\`, kind: "drive" });
  };
  addDrive(process.env.SystemDrive);
  addDrive(process.env.HOMEDRIVE);
  addDrive(process.cwd());
  addDrive(app.getPath("documents"));
  const letters = "ABCDEFGHIJKLMNOPQRSTUVWXYZ".split("");
  await Promise.all(
    letters.map(async (letter) => {
      const drive = `${letter}:\\`;
      try {
        await fs.stat(drive);
        byPath.set(drive, { id: `drive:${letter}`, name: `${letter}:`, path: drive, kind: "drive" });
      } catch {
        // drive not mounted/readable
      }
    }),
  );
  return Array.from(byPath.values()).sort((a, b) => a.name.localeCompare(b.name));
}

function isHiddenOrSystemName(name: string): boolean {
  return name.startsWith(".") || name === "$RECYCLE.BIN" || name === "System Volume Information";
}

const BROWSER_INDEX_BATCH_SIZE = 64;
const BROWSER_INDEX_YIELD_MS = 8;
const SKIP_INDEX_DIR_NAMES = new Set([
  "$RECYCLE.BIN",
  "System Volume Information",
  "Windows",
  "Program Files",
  "Program Files (x86)",
  "ProgramData",
  "AppData",
  "node_modules",
  ".git",
]);

function shouldSkipIndexDirectory(name: string): boolean {
  return isHiddenOrSystemName(name) || SKIP_INDEX_DIR_NAMES.has(name);
}

function waitForIndexYield(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, BROWSER_INDEX_YIELD_MS));
}

async function listBrowserDirectory(dirPath: string): Promise<BrowserFileEntry[]> {
  const normalized = path.normalize(dirPath);
  const entries = await fs.readdir(normalized, { withFileTypes: true });
  const rows: BrowserFileEntry[] = [];
  for (const entry of entries) {
    if (isHiddenOrSystemName(entry.name)) continue;
    const childPath = path.join(normalized, entry.name);
    if (entry.isDirectory()) {
      rows.push({ name: entry.name, path: childPath, kind: "folder" });
      continue;
    }
    if (!entry.isFile()) continue;
    if (!isImportableAudioPath(childPath)) continue;
    try {
      const stat = await fs.stat(childPath);
      rows.push({
        name: entry.name,
        path: childPath,
        kind: "audio",
        size: stat.size,
        lastModified: Math.round(stat.mtimeMs),
        mimeType: mimeFor(childPath),
      });
    } catch {
      // skip unreadable file
    }
  }
  return rows.sort((a, b) => {
    if (a.kind === b.kind) return a.name.localeCompare(b.name);
    return a.kind === "folder" ? -1 : 1;
  });
}

type SqliteStatement = {
  run: (...args: unknown[]) => unknown;
};

type SqliteDatabase = {
  exec: (sql: string) => unknown;
  prepare: (sql: string) => SqliteStatement;
};

let browserIndexDb: SqliteDatabase | null | undefined;
let browserIndexDbWarningShown = false;
const browserIndexJobs = new Map<string, BrowserIndexStatus>();

function browserIndexDbPath(): string {
  return path.join(app.getPath("userData"), "file-browser-index.sqlite");
}

async function getBrowserIndexDb(): Promise<SqliteDatabase | null> {
  if (browserIndexDb !== undefined) return browserIndexDb;
  try {
    await fs.mkdir(app.getPath("userData"), { recursive: true });
    const sqlite = require("node:sqlite") as {
      DatabaseSync?: new (location: string) => SqliteDatabase;
    };
    if (!sqlite.DatabaseSync) throw new Error("node:sqlite DatabaseSync is unavailable");
    const db = new sqlite.DatabaseSync(browserIndexDbPath());
    db.exec(`
      PRAGMA journal_mode = WAL;
      CREATE TABLE IF NOT EXISTS browser_index (
        root_path TEXT NOT NULL,
        path TEXT PRIMARY KEY,
        parent_path TEXT NOT NULL,
        name TEXT NOT NULL,
        kind TEXT NOT NULL,
        size INTEGER,
        last_modified INTEGER,
        mime_type TEXT,
        indexed_at INTEGER NOT NULL
      );
      CREATE INDEX IF NOT EXISTS idx_browser_index_root ON browser_index(root_path);
      CREATE INDEX IF NOT EXISTS idx_browser_index_parent ON browser_index(parent_path);
      CREATE INDEX IF NOT EXISTS idx_browser_index_kind ON browser_index(kind);
    `);
    browserIndexDb = db;
  } catch (err) {
    if (!browserIndexDbWarningShown) {
      browserIndexDbWarningShown = true;
      console.warn("[FileBrowser] SQLite index unavailable:", err);
    }
    browserIndexDb = null;
  }
  return browserIndexDb;
}

function normalizedBrowserRoot(rootPath: string): string {
  return path.normalize(rootPath);
}

function idleBrowserIndexStatus(rootPath: string): BrowserIndexStatus {
  return {
    rootPath: normalizedBrowserRoot(rootPath),
    dbPath: browserIndexDbPath(),
    status: "idle",
    scannedDirs: 0,
    scannedFiles: 0,
    audioFiles: 0,
  };
}

function startBrowserIndex(rootPath: string): BrowserIndexStatus {
  const root = normalizedBrowserRoot(rootPath);
  const existing = browserIndexJobs.get(root);
  if (existing?.status === "indexing") return existing;
  const status: BrowserIndexStatus = {
    rootPath: root,
    dbPath: browserIndexDbPath(),
    status: "indexing",
    scannedDirs: 0,
    scannedFiles: 0,
    audioFiles: 0,
    currentPath: root,
    startedAt: Date.now(),
    updatedAt: Date.now(),
  };
  browserIndexJobs.set(root, status);
  void runBrowserIndex(root, status);
  return status;
}

async function runBrowserIndex(root: string, status: BrowserIndexStatus): Promise<void> {
  const db = await getBrowserIndexDb();
  if (!db) {
    status.status = "error";
    status.error = "SQLite index unavailable in this Electron runtime";
    status.finishedAt = Date.now();
    status.updatedAt = status.finishedAt;
    return;
  }
  const indexedAt = Date.now();
  const deleteRoot = db.prepare("DELETE FROM browser_index WHERE root_path = ?");
  const insert = db.prepare(`
    INSERT OR REPLACE INTO browser_index
      (root_path, path, parent_path, name, kind, size, last_modified, mime_type, indexed_at)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
  `);
  try {
    deleteRoot.run(root);
    const stack = [root];
    let workSinceYield = 0;
    while (stack.length > 0) {
      const dir = stack.pop();
      if (!dir) continue;
      if (workSinceYield >= BROWSER_INDEX_BATCH_SIZE) {
        workSinceYield = 0;
        await waitForIndexYield();
      }
      status.currentPath = dir;
      status.updatedAt = Date.now();
      let entries: Array<{ name: string; isDirectory(): boolean; isFile(): boolean }>;
      try {
        entries = await fs.readdir(dir, { withFileTypes: true });
        status.scannedDirs++;
      } catch {
        continue;
      }
      workSinceYield++;
      for (const entry of entries) {
        if (workSinceYield >= BROWSER_INDEX_BATCH_SIZE) {
          workSinceYield = 0;
          await waitForIndexYield();
        }
        workSinceYield++;
        if (isHiddenOrSystemName(entry.name)) continue;
        const childPath = path.join(dir, entry.name);
        if (entry.isDirectory()) {
          if (shouldSkipIndexDirectory(entry.name)) continue;
          insert.run(root, childPath, dir, entry.name, "folder", null, null, null, indexedAt);
          stack.push(childPath);
          continue;
        }
        if (!entry.isFile()) continue;
        status.scannedFiles++;
        if (!isImportableAudioPath(childPath)) continue;
        try {
          const stat = await fs.stat(childPath);
          insert.run(
            root,
            childPath,
            dir,
            entry.name,
            "audio",
            stat.size,
            Math.round(stat.mtimeMs),
            mimeFor(childPath),
            indexedAt,
          );
          status.audioFiles++;
        } catch {
          // skip unreadable file
        }
      }
    }
    status.status = "done";
    status.currentPath = undefined;
    status.finishedAt = Date.now();
    status.updatedAt = status.finishedAt;
  } catch (err) {
    status.status = "error";
    status.error = err instanceof Error ? err.message : String(err);
    status.finishedAt = Date.now();
    status.updatedAt = status.finishedAt;
  }
}

function browserIndexStatuses(paths?: unknown): BrowserIndexStatus[] {
  const requested = Array.isArray(paths)
    ? paths.filter((p): p is string => typeof p === "string" && p.length > 0).map(normalizedBrowserRoot)
    : [];
  if (requested.length === 0) return Array.from(browserIndexJobs.values());
  return requested.map((root) => browserIndexJobs.get(root) ?? idleBrowserIndexStatus(root));
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

type WavInfo = {
  sampleRate: number;
  channels: number;
  bitsPerSample: number;
  audioFormat: number;
  dataOffset: number;
  dataBytes: number;
  duration: number;
};

async function readWavInfo(filePath: string): Promise<WavInfo | null> {
  const handle = await fs.open(filePath, "r");
  try {
    const header = Buffer.alloc(65536);
    const { bytesRead } = await handle.read(header, 0, header.length, 0);
    const view = header.subarray(0, bytesRead);
    if (view.length < 44 || view.toString("ascii", 0, 4) !== "RIFF" || view.toString("ascii", 8, 12) !== "WAVE") {
      return null;
    }

    let offset = 12;
    let sampleRate = 0;
    let channels = 0;
    let bitsPerSample = 0;
    let audioFormat = 0;
    let dataOffset = 0;
    let dataBytes = 0;
    while (offset + 8 <= view.length) {
      const id = view.toString("ascii", offset, offset + 4);
      const size = view.readUInt32LE(offset + 4);
      const chunk = offset + 8;
      if (id === "fmt " && chunk + 16 <= view.length) {
        audioFormat = view.readUInt16LE(chunk);
        channels = view.readUInt16LE(chunk + 2);
        sampleRate = view.readUInt32LE(chunk + 4);
        bitsPerSample = view.readUInt16LE(chunk + 14);
      } else if (id === "data") {
        dataOffset = chunk;
        dataBytes = size;
        break;
      }
      offset = chunk + size + (size % 2);
    }

    if (!sampleRate || !channels || !bitsPerSample || !dataOffset || !dataBytes) return null;
    const bytesPerFrame = channels * (bitsPerSample / 8);
    return {
      sampleRate,
      channels,
      bitsPerSample,
      audioFormat,
      dataOffset,
      dataBytes,
      duration: dataBytes / bytesPerFrame / sampleRate,
    };
  } finally {
    await handle.close();
  }
}

async function generateWavPeaksFromPath(filePath: string, fileId: string, samplesPerPeak: number): Promise<WavPeakResult | null> {
  const normalized = path.normalize(filePath);
  if (!isImportableAudioPath(normalized) || path.extname(normalized).toLowerCase() !== ".wav") return null;
  const info = await readWavInfo(normalized);
  if (!info || info.audioFormat !== 1 || ![16, 24, 32].includes(info.bitsPerSample)) return null;

  const bytesPerSample = info.bitsPerSample / 8;
  const bytesPerFrame = bytesPerSample * info.channels;
  const totalFrames = Math.floor(info.dataBytes / bytesPerFrame);
  const safeSamplesPerPeak = Math.max(1, Math.floor(samplesPerPeak));
  const peakCount = Math.ceil(totalFrames / safeSamplesPerPeak);
  const peaks = new Int16Array(peakCount * info.channels * 2);
  const min = new Float32Array(info.channels);
  const max = new Float32Array(info.channels);
  resetPeakMinMax(min, max);

  const handle = await fs.open(normalized, "r");
  try {
    const chunk = Buffer.alloc(1024 * 1024);
    let byteOffset = info.dataOffset;
    const dataEnd = info.dataOffset + info.dataBytes;
    let frameIndex = 0;
    let currentPeak = 0;

    while (byteOffset < dataEnd) {
      const remaining = dataEnd - byteOffset;
      const wanted = Math.min(chunk.length, remaining);
      const alignedWanted = remaining <= chunk.length
        ? wanted
        : Math.max(bytesPerFrame, Math.floor(wanted / bytesPerFrame) * bytesPerFrame);
      const { bytesRead } = await handle.read(chunk, 0, alignedWanted, byteOffset);
      if (bytesRead <= 0) break;
      const frameCount = Math.floor(bytesRead / bytesPerFrame);

      for (let frame = 0; frame < frameCount; frame++) {
        const frameByte = frame * bytesPerFrame;
        for (let ch = 0; ch < info.channels; ch++) {
          const sampleByte = frameByte + ch * bytesPerSample;
          const value = readPcmSample(chunk, sampleByte, info.bitsPerSample);
          if (value < min[ch]) min[ch] = value;
          if (value > max[ch]) max[ch] = value;
        }
        frameIndex++;
        if (frameIndex % safeSamplesPerPeak === 0) {
          writePeak(peaks, currentPeak, info.channels, min, max);
          currentPeak++;
          resetPeakMinMax(min, max);
        }
      }

      byteOffset += bytesRead;
    }

    if (currentPeak < peakCount) writePeak(peaks, currentPeak, info.channels, min, max);
  } finally {
    await handle.close();
  }

  return {
    fileId,
    sampleRate: info.sampleRate,
    channelCount: info.channels,
    duration: info.duration,
    samplesPerPeak: safeSamplesPerPeak,
    peakCount,
    peaks: Array.from(peaks),
  };
}

function readPcmSample(buffer: Buffer, offset: number, bitsPerSample: number): number {
  if (bitsPerSample === 16) return buffer.readInt16LE(offset) / 32768;
  if (bitsPerSample === 24) {
    let sample = buffer[offset] | (buffer[offset + 1] << 8) | (buffer[offset + 2] << 16);
    if (sample & 0x800000) sample |= 0xff000000;
    return sample / 8388608;
  }
  return buffer.readInt32LE(offset) / 2147483648;
}

function resetPeakMinMax(min: Float32Array, max: Float32Array): void {
  for (let i = 0; i < min.length; i++) {
    min[i] = 1;
    max[i] = -1;
  }
}

function writePeak(peaks: Int16Array, peakIndex: number, channels: number, min: Float32Array, max: Float32Array): void {
  for (let ch = 0; ch < channels; ch++) {
    const base = (peakIndex * channels + ch) * 2;
    peaks[base] = Math.max(-32768, Math.min(32767, Math.round(min[ch] * 32767)));
    peaks[base + 1] = Math.max(-32768, Math.min(32767, Math.round(max[ch] * 32767)));
  }
}

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

function externalRouteForContent(contentType: string): string {
  switch (contentType) {
    case "mixer":
      return "/external/mixer";
    case "projectWizard":
      return "/projectwizard";
    case "preferences":
      return "/settings";
    case "pluginManager":
      return "/plugin-manager";
    default:
      return "/";
  }
}

function rendererRouteUrl(route: string, payload?: Record<string, unknown>): string {
  const params = new URLSearchParams();
  if (payload?.initialTab && typeof payload.initialTab === "string") {
    params.set("tab", payload.initialTab);
  }
  const suffix = params.size > 0 ? `?${params.toString()}` : "";
  const hashRoute = `#${route.startsWith("/") ? route : `/${route}`}${suffix}`;
  if (app.isPackaged) {
    return `miko://app/index.html${hashRoute}`;
  }
  return `${DEV_SERVER_URL.replace(/\/$/, "")}/${hashRoute}`;
}

function openExternalRendererWindow(config: ExternalWindowConfig): string | null {
  const id = config.id && config.id.trim().length > 0 ? config.id : randomUUID();
  const existing = externalWindows.get(id);
  if (existing && !existing.isDestroyed()) {
    if (existing.isMinimized()) existing.restore();
    existing.focus();
    return id;
  }

  const win = new BrowserWindow({
    width: Math.max(config.minWidth ?? 320, config.width),
    height: Math.max(config.minHeight ?? 240, config.height),
    minWidth: config.minWidth,
    minHeight: config.minHeight,
    title: config.title,
    icon: windowIconPath(),
    frame: config.frame ?? true,
    transparent: config.transparent ?? false,
    resizable: config.resizable ?? true,
    maximizable: config.maximizable ?? true,
    alwaysOnTop: config.alwaysOnTop ?? false,
    backgroundColor: "#0b0f14",
    show: false,
    webPreferences: {
      preload: PRELOAD_PATH,
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false,
      backgroundThrottling: false,
      spellcheck: false,
      enableWebSQL: false,
      webgl: true,
      devTools: !app.isPackaged,
    },
  });

  externalWindows.set(id, win);
  win.once("ready-to-show", () => win.show());
  win.on("closed", () => externalWindows.delete(id));

  const route = externalRouteForContent(config.contentType);
  void win.loadURL(rendererRouteUrl(route, config.payload));

  return id;
}

function closeAuxiliaryWindows(): void {
  for (const win of externalWindows.values()) {
    if (!win.isDestroyed()) win.close();
  }
  externalWindows.clear();
  try {
    getFloatingWindowManager().stop();
  } catch (error) {
    console.warn("[Window] failed to stop floating window runtime:", error);
  }
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
    IpcChannels.FsGenerateWavPeaks,
    async (_event, filePath: unknown, fileId: unknown, samplesPerPeak: unknown): Promise<WavPeakResult | null> => {
      if (!isValidString(filePath) || !isValidString(fileId)) return null;
      const spp = typeof samplesPerPeak === "number" && Number.isFinite(samplesPerPeak) ? samplesPerPeak : 8192;
      try {
        const { sphereAudioNative } = await import("./native-audio/SphereAudioNative.js");
        if (sphereAudioNative.initialize()) {
          const native = sphereAudioNative.generateWavPeaks(filePath, fileId, spp);
          if (native) return native;
        }
      } catch (err) {
        console.warn("[WaveformPeaks] Rust generate failed; falling back to TS scanner:", err);
      }
      try {
        return await generateWavPeaksFromPath(filePath, fileId, spp);
      } catch (err) {
        console.error("[WaveformPeaks] fallback generate failed:", err);
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
    IpcChannels.FsEnsureFactoryLibrary,
    async (): Promise<BrowserRootEntry[]> => {
      return ensureFactoryLibraryFolders();
    },
  );

  ipcMain.handle(
    IpcChannels.FsBrowserRoots,
    async (): Promise<BrowserRootEntry[]> => {
      const [factory, drives] = await Promise.all([
        ensureFactoryLibraryFolders(),
        mountedDriveRoots(),
      ]);
      return [...factory, ...drives];
    },
  );

  ipcMain.handle(
    IpcChannels.FsBrowserListDir,
    async (_event, dirPath: unknown): Promise<BrowserFileEntry[]> => {
      if (!isValidString(dirPath)) return [];
      try {
        return await listBrowserDirectory(dirPath);
      } catch (err) {
        console.warn("[FileBrowser] listDir failed:", dirPath, err);
        return [];
      }
    },
  );

  ipcMain.handle(
    IpcChannels.FsBrowserIndexStart,
    async (_event, rootPath: unknown): Promise<BrowserIndexStatus> => {
      if (!isValidString(rootPath)) return idleBrowserIndexStatus("");
      return startBrowserIndex(rootPath);
    },
  );

  ipcMain.handle(
    IpcChannels.FsBrowserIndexStatus,
    async (_event, rootPaths: unknown): Promise<BrowserIndexStatus[]> => {
      return browserIndexStatuses(rootPaths);
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
      const defaultProjectsDir = path.join(app.getPath("documents"), "Futureboard Studio", "Projects");
      const opts: Electron.OpenDialogOptions = {
        title: "Open Project",
        defaultPath: defaultProjectsDir,
        filters: [{ name: "Futureboard Project", extensions: ["mochiproj"] }],
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
        defaultId: options?.defaultId,
        cancelId: options?.cancelId,
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

  ipcMain.handle(IpcChannels.WindowForceClose, (event): void => {
    const win = senderWindow(event);
    if (!win) return;
    closeAuxiliaryWindows();
    closeAllowed.add(win);
    win.close();
  });

  ipcMain.handle(IpcChannels.WindowsOpenExternal, (_event, config: unknown): string | null => {
    const c = config as ExternalWindowConfig;
    if (!c || !isValidString(c.title) || !isValidString(c.contentType)) return null;
    return openExternalRendererWindow(c);
  });

  ipcMain.handle(IpcChannels.WindowsCloseExternal, (_event, id: unknown): void => {
    if (!isValidString(id)) return;
    const win = externalWindows.get(id);
    if (!win || win.isDestroyed()) return;
    win.close();
  });

  ipcMain.handle(IpcChannels.WindowsFocusExternal, (_event, id: unknown): void => {
    if (!isValidString(id)) return;
    const win = externalWindows.get(id);
    if (!win || win.isDestroyed()) return;
    if (win.isMinimized()) win.restore();
    win.focus();
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

  // ── Binary peak chunk storage ─────────────────────────────────────────────
  // Files live at: <projectRoot>/Cache/Peaks/<fileId>/<spp>/chunk_<n>.bin

  function peakChunkFilePath(projectRoot: string, fileId: string, spp: number, chunkIndex: number): string {
    const safeId = fileId.replace(/[^a-zA-Z0-9_\-]/g, "_");
    return path.join(path.normalize(projectRoot), "Cache", "Peaks", safeId, String(spp), `chunk_${chunkIndex}.bin`);
  }

  ipcMain.handle(
    IpcChannels.PeakChunkRead,
    async (_event, fileId: unknown, spp: unknown, chunkIndex: unknown, projectRoot: unknown): Promise<ArrayBuffer | null> => {
      if (!isValidString(fileId) || typeof spp !== "number" || typeof chunkIndex !== "number" || !isValidString(projectRoot)) return null;
      try {
        const buf = await fs.readFile(peakChunkFilePath(projectRoot, fileId, spp, chunkIndex));
        return buf.buffer.slice(buf.byteOffset, buf.byteOffset + buf.byteLength) as ArrayBuffer;
      } catch {
        return null;
      }
    },
  );

  ipcMain.handle(
    IpcChannels.PeakChunkWrite,
    async (_event, fileId: unknown, spp: unknown, chunkIndex: unknown, data: unknown, projectRoot: unknown): Promise<void> => {
      if (!isValidString(fileId) || typeof spp !== "number" || typeof chunkIndex !== "number" || !isValidString(projectRoot)) return;
      const buf = data instanceof ArrayBuffer ? Buffer.from(data) : data instanceof Uint8Array ? Buffer.from(data) : null;
      if (!buf) return;
      try {
        const filePath = peakChunkFilePath(projectRoot, fileId, spp, chunkIndex);
        await fs.mkdir(path.dirname(filePath), { recursive: true });
        await fs.writeFile(filePath, buf);
      } catch (e) {
        console.warn("[PeakChunk] write failed:", e);
      }
    },
  );

  // ── System / diagnostics ────────────────────────────────────────────────────

  async function readElectronSettings(): Promise<ElectronPersistedSettings> {
    try {
      const raw = await fs.readFile(electronSettingsFilePath(), "utf-8");
      const parsed = JSON.parse(raw) as Partial<ElectronPersistedSettings>;
      return {
        graphicRenderingMode:
          parsed.graphicRenderingMode === "force"
            ? "force"
            : parsed.graphicRenderingMode === "software"
              ? "software"
              : "auto",
      };
    } catch {
      return { graphicRenderingMode: "auto" };
    }
  }

  async function getPrimaryGpuDescription(): Promise<string | null> {
    try {
      const info = await app.getGPUInfo("basic");
      const gpuInfo = info as Record<string, unknown>;
      const gpuList = Array.isArray(gpuInfo.gpuDevice)
        ? (gpuInfo.gpuDevice as Array<{ description?: string; vendorId?: number; deviceId?: number }>)
        : [];
      const primary = gpuList.find((gpu) => typeof gpu.description === "string" && gpu.description.trim().length > 0)
        ?? gpuList[0];
      if (!primary) return null;
      const description = primary.description?.trim();
      if (description) return description;
      const vendor = primary.vendorId?.toString(16) ?? "?";
      const device = primary.deviceId?.toString(16) ?? "?";
      return `GPU vendor=${vendor} device=${device}`;
    } catch {
      return null;
    }
  }

  async function writeElectronSettings(settings: ElectronPersistedSettings): Promise<void> {
    const p = electronSettingsFilePath();
    await fs.mkdir(path.dirname(p), { recursive: true });
    const tmp = `${p}.tmp`;
    await fs.writeFile(tmp, JSON.stringify(settings, null, 2), "utf-8");
    await fs.rename(tmp, p);
  }

  ipcMain.handle(
    IpcChannels.SysReadElectronSettings,
    async (): Promise<ElectronPersistedSettings> => readElectronSettings(),
  );

  ipcMain.handle(
    IpcChannels.SysWriteElectronSettings,
    async (_event, settings: unknown): Promise<void> => {
      if (typeof settings !== "object" || settings === null) return;
      const s = settings as Partial<ElectronPersistedSettings>;
      await writeElectronSettings({
        graphicRenderingMode:
          s.graphicRenderingMode === "force"
            ? "force"
            : s.graphicRenderingMode === "software"
              ? "software"
              : "auto",
      });
    },
  );

  ipcMain.handle(
    IpcChannels.SysGetGpuInfo,
    async (): Promise<GpuFeatureStatus> => {
      let features: Record<string, string> = {};
      try {
        features = app.getGPUFeatureStatus() as unknown as Record<string, string>;
      } catch { /* ignore */ }
      return {
        hardwareAccelerationEnabled: app.isHardwareAccelerationEnabled(),
        gpuMode: GPU_MODE,
        features,
        gpuDescription: await getPrimaryGpuDescription(),
        electronVersion: process.versions.electron ?? "unknown",
        chromeVersion: process.versions.chrome ?? "unknown",
      };
    },
  );

  ipcMain.handle(
    IpcChannels.SysGetDefaultProjectsPath,
    async (): Promise<string> => {
      const docsDir = app.getPath("documents");
      const projectsDir = path.join(docsDir, "Futureboard Studio", "Projects");
      try { await fs.mkdir(projectsDir, { recursive: true }); } catch { /* already exists */ }
      return projectsDir;
    },
  );

  // ── Native audio plug-in registry ──────────────────────────────────────────

  ipcMain.handle(
    IpcChannels.PluginHostGetStatus,
    async (): Promise<AudioPluginHostStatus> => pluginHostNative.getStatus(),
  );

  ipcMain.handle(
    IpcChannels.PluginHostListPlugins,
    async (): Promise<AudioPluginRegistryEntry[]> => pluginHostNative.listPlugins(),
  );

  ipcMain.handle(
    IpcChannels.PluginHostScanVst3,
    async (event, scanPaths: unknown): Promise<AudioPluginScanResult> => {
      const paths = Array.isArray(scanPaths)
        ? scanPaths.filter((p): p is string => typeof p === "string" && p.trim().length > 0)
        : undefined;
      const sendProgress = (payload: AudioPluginScanProgressEvent) => {
        if (!event.sender.isDestroyed()) {
          event.sender.send(IpcChannels.PluginHostScanProgress, payload);
        }
      };
      return pluginHostNative.scanVst3(paths, sendProgress);
    },
  );

  ipcMain.handle(
    IpcChannels.PluginHostRevealPreset,
    async (_event, pluginId: unknown): Promise<void> => {
      if (!isValidString(pluginId)) return;
      const presetPath = pluginHostNative.presetPathForPlugin(pluginId);
      if (!presetPath) return;
      await shell.showItemInFolder(presetPath);
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
      const defaultProjectsDir = path.join(app.getPath("documents"), "Futureboard Studio", "Projects");
      try { await fs.mkdir(defaultProjectsDir, { recursive: true }); } catch { /* already exists */ }
      const opts: Electron.OpenDialogOptions = {
        title: "Choose Project Location",
        defaultPath: defaultProjectsDir,
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

  // ── Native Floating Window runtime ────────────────────────────────────────
  ipcMain.handle(IpcChannels.FloatingWindowOpen, (_event, req: unknown): boolean => {
    const r = req as FloatingWindowOpenRequest;
    if (!r?.id || !r?.kind) return false;
    const mgr = getFloatingWindowManager();
    if (!mgr.running && !mgr.start()) return false;
    mgr.openWindow({ id: r.id, kind: r.kind, title: r.title ?? r.kind, alwaysOnTop: r.alwaysOnTop ?? false });
    return true;
  });

  ipcMain.handle(IpcChannels.FloatingWindowClose, (_event, id: unknown): void => {
    if (typeof id !== "string") return;
    getFloatingWindowManager().closeWindow(id);
  });

  ipcMain.handle(IpcChannels.FloatingWindowFocus, (_event, id: unknown): void => {
    if (typeof id !== "string") return;
    getFloatingWindowManager().focusWindow(id);
  });

  ipcMain.handle(IpcChannels.FloatingWindowMixerUpdate, (_event, req: unknown): void => {
    const r = req as FloatingWindowMixerUpdateRequest;
    if (!Array.isArray(r?.tracks) || !r?.master) return;
    getFloatingWindowManager().pushMixerUpdate(r.tracks, r.master);
  });
}

// Register IPC handlers eagerly so they are guaranteed to be live before the
// renderer's preload script issues its first `ipcRenderer.invoke`.
registerIpcHandlers();

function sendOpenFileCommand(win: BrowserWindow, filePath: string): void {
  win.webContents.send("app-command", `project:open-file:${filePath}`);
}

function tryOpenFileArg(win: BrowserWindow, argv: string[]): void {
  const file = argv.find((a) => a.endsWith(".mochiproj") && !a.startsWith("-"));
  if (file) sendOpenFileCommand(win, file);
}

app.on("second-instance", (_event, argv) => {
  const [win] = BrowserWindow.getAllWindows();
  if (!win) return;
  if (win.isMinimized()) win.restore();
  win.focus();
  tryOpenFileArg(win, argv);
});

// macOS: file opened via Finder / OS association while app is already running.
app.on("open-file", (event, filePath) => {
  event.preventDefault();
  const [win] = BrowserWindow.getAllWindows();
  if (!win) return;
  sendOpenFileCommand(win, filePath);
});

app.whenReady().then(async () => {
  installApplicationMenu();

  // Log GPU diagnostics so we can verify hardware acceleration is active.
  function logGpuStatus(): void {
    try {
      const hwEnabled = app.isHardwareAccelerationEnabled();
      const gpuFeatures = app.getGPUFeatureStatus();
      console.log(`[GPU] mode=${GPU_MODE} hw_acceleration=${hwEnabled}`);
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
      }).catch((e) => console.warn("[GPU] getGPUInfo failed:", e));
    } catch (e) {
      console.warn("[GPU] getGPUFeatureStatus failed:", e);
    }
  }

  logGpuStatus();
  app.on("gpu-info-update", () => {
    console.log("[GPU] gpu-info-update fired — re-logging status");
    logGpuStatus();
  });

  registerMikoProtocol();

  try {
    await pluginHostNative.ensurePresetFolders();
  } catch (error) {
    console.warn("[PluginHost] failed to create preset folders:", error);
  }

  // Register SphereDirectAudioEngine IPC handlers and try to start the native engine.
  const { registerSphereAudioHandlers } = await import("./native-audio/ipc-handlers.js");
  registerSphereAudioHandlers(__dirname);

  initAutoUpdater();

  const splash = createSplashWindow();
  const win = createWindow(false);
  win.once("ready-to-show", () => {
    splash.close();
    win.show();
    // Open project passed on the command line (e.g. double-click .mochiproj in Explorer).
    tryOpenFileArg(win, process.argv);
  });

  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      createWindow();
    }
  });
});

app.on("window-all-closed", () => {
  if (!isMac) app.quit();
});
