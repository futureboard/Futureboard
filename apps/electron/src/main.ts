import {
  app,
  BrowserWindow,
  dialog,
  ipcMain,
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
  type PickedAudioFile,
  type SaveDialogResult,
  type WaveformCacheEntryIpc,
} from "./ipc/channels.js";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const DEV_SERVER_URL =
  process.env.VITE_DEV_SERVER_URL ?? "http://localhost:5173";

const isMac = process.platform === "darwin";
const isWin = process.platform === "win32";

// ── Cold-start tuning ─────────────────────────────────────────────────────
// Must be set before `app.whenReady()` to take effect. These switches keep
// the renderer responsive for a DAW workload (precise timers, GPU video
// decode disabled, larger disk cache for code/preload V8 caching, etc.).

// Larger HTTP/disk cache so renderer JS, fonts, and assets get cached after
// first launch.
app.commandLine.appendSwitch("disk-cache-size", String(256 * 1024 * 1024));

// Audio-thread friendly: don't throttle background timers; the audio worklet
// and metronome scheduler rely on accurate intervals.
app.commandLine.appendSwitch("disable-background-timer-throttling");
app.commandLine.appendSwitch("disable-renderer-backgrounding");
app.commandLine.appendSwitch("disable-backgrounding-occluded-windows");

// Enable shared memory + faster canvas rasterization (waveforms, meters).
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

/**
 * Path to the packaged renderer's `index.html`. electron-builder copies the
 * web build into `resources/renderer/` via `extraResources` (see package.json
 * `build.extraResources`).
 */
function packagedRendererIndex(): string {
  return path.join(process.resourcesPath, "renderer", "index.html");
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
    void win.loadFile(packagedRendererIndex());
  } else {
    void win.loadURL(DEV_SERVER_URL);
    win.webContents.openDevTools({ mode: "detach" });
  }

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
          const buf = await fs.readFile(p);
          files.push({
            name: path.basename(p),
            mimeType: mimeFor(p),
            bytes: buf.buffer.slice(
              buf.byteOffset,
              buf.byteOffset + buf.byteLength,
            ) as ArrayBuffer,
            path: p,
          });
        } catch (err) {
          console.error("Failed reading audio file", p, err);
        }
      }
      return files;
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
  // Peaks are stored as JSON files under userData/cache/waveforms/.
  // Renderer sends only the cache key — main decides the actual file path.

  function waveformCacheDir(): string {
    return path.join(app.getPath("userData"), "cache", "waveforms");
  }

  function cacheKeyToFilename(key: string): string {
    // Replace characters that are unsafe in filenames with underscores
    return key.replace(/[^a-zA-Z0-9_\-:.]/g, "_") + ".json";
  }

  async function ensureCacheDir(): Promise<string> {
    const dir = waveformCacheDir();
    await fs.mkdir(dir, { recursive: true });
    return dir;
  }

  ipcMain.handle(
    IpcChannels.WaveformCacheGet,
    async (_event, key: unknown): Promise<WaveformCacheEntryIpc | null> => {
      if (!isValidString(key)) return null;
      try {
        const dir = await ensureCacheDir();
        const filePath = path.join(dir, cacheKeyToFilename(key));
        const raw = await fs.readFile(filePath, "utf-8");
        return JSON.parse(raw) as WaveformCacheEntryIpc;
      } catch {
        return null;
      }
    },
  );

  ipcMain.handle(
    IpcChannels.WaveformCacheSet,
    async (_event, key: unknown, entry: unknown): Promise<void> => {
      if (!isValidString(key) || typeof entry !== "object" || entry === null) return;
      try {
        const dir = await ensureCacheDir();
        const filePath = path.join(dir, cacheKeyToFilename(key));
        await fs.writeFile(filePath, JSON.stringify(entry), "utf-8");
      } catch (e) {
        console.warn("[WaveformCache] write failed:", e);
      }
    },
  );

  ipcMain.handle(
    IpcChannels.WaveformCacheDelete,
    async (_event, key: unknown): Promise<void> => {
      if (!isValidString(key)) return;
      try {
        const dir = waveformCacheDir();
        const filePath = path.join(dir, cacheKeyToFilename(key));
        await fs.unlink(filePath);
      } catch {
        // File may not exist — ignore
      }
    },
  );

  ipcMain.handle(
    IpcChannels.WaveformCacheClear,
    async (): Promise<void> => {
      try {
        const dir = waveformCacheDir();
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
