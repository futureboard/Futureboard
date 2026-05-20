import type { DawProject } from "../types/daw";
import type {
  DialogAdapter,
  FileSystemAdapter,
  FolderProjectAdapter,
  MessageBoxOptions,
  PluginHostAdapter,
  Platform,
  PlatformCapabilities,
  ProjectStorageAdapter,
  SaveProjectOptions,
  SaveProjectResult,
  WindowAdapter,
} from "./platform.types";

const STORAGE_KEY = "mochi-daw-project";
const AUDIO_ACCEPT = "audio/wav,audio/mpeg,audio/mp3,.wav,.mp3";

const capabilities: PlatformCapabilities = {
  kind: "web",
  filesystem: false,
  persistentLocalProjects: false,
  nativeDialogs: false,
  nativeWindowControls: false,
  nativeAudioEngine: false,
  nativePlugins: false,
  webAudio: typeof window !== "undefined" && "AudioContext" in window,
  cloudSync: false,
  osFilePaths: false,
};

const pluginHost: PluginHostAdapter = {
  isSupported: false,
  async getStatus() {
    return {
      available: false,
      backend: "web",
      message: "Native audio plug-in scanning is available only in the Electron client.",
      dbPath: "",
      presetRoot: "",
      defaultScanPaths: [],
    };
  },
  async listPlugins() {
    return [];
  },
  async scanVst3() {
    const status = await pluginHost.getStatus();
    return { status, plugins: [], scannedPaths: [], generatedPresets: 0, failed: [] };
  },
  onScanProgress() {
    return () => { /* no-op on web */ };
  },
  async revealPreset() {
    throw new Error("Native audio plug-in presets are not available on web");
  },
};

/**
 * Creates a transient hidden `<input type="file">`, awaits a single user
 * interaction, and resolves with the selected files (or an empty array on
 * cancel). The element is removed from the DOM once the user has chosen.
 */
function pickAudioFilesViaHiddenInput(): Promise<File[]> {
  return new Promise((resolve) => {
    if (typeof document === "undefined") {
      resolve([]);
      return;
    }
    const input = document.createElement("input");
    input.type = "file";
    input.accept = AUDIO_ACCEPT;
    input.multiple = true;
    input.style.position = "fixed";
    input.style.left = "-9999px";
    input.style.top = "-9999px";
    input.style.opacity = "0";

    let settled = false;
    const finish = (files: File[]) => {
      if (settled) return;
      settled = true;
      input.remove();
      resolve(files);
    };

    input.addEventListener("change", () => {
      const files = input.files ? Array.from(input.files) : [];
      finish(files);
    });
    // Best-effort cancel detection (supported in modern browsers).
    input.addEventListener("cancel", () => finish([]));

    document.body.appendChild(input);
    input.click();
  });
}

const fileSystem: FileSystemAdapter = {
  pickAudioFiles: pickAudioFilesViaHiddenInput,
  async readAudioFile(_path: string): Promise<File | null> {
    return null;
  },
  async statAudioFile(_path: string): Promise<{ size: number; lastModified: number; name: string; mimeType: string } | null> {
    return null;
  },
  async generateWavPeaks(_path: string, _fileId: string, _samplesPerPeak: number) {
    return null;
  },
  async browserRoots() {
    return [];
  },
  async browserListDir(_path: string) {
    return [];
  },
  async ensureFactoryLibrary() {
    return [];
  },
  async browserIndexStart(path: string) {
    return {
      rootPath: path,
      dbPath: "",
      status: "idle" as const,
      scannedDirs: 0,
      scannedFiles: 0,
      audioFiles: 0,
    };
  },
  async browserIndexStatus(_paths?: string[]) {
    return [];
  },
  getNativePathForFile(_file: File): string | null {
    return null;
  },
  async revealInFileManager(_path: string): Promise<void> {
    throw new Error("revealInFileManager is not supported on web");
  },
};

function serializeProject(project: DawProject): unknown {
  return {
    ...project,
      files: project.files.map((file) => ({
        id: file.id,
        name: file.name,
        mimeType: file.mimeType,
        size: file.size,
        lastModified: file.lastModified,
        hash: file.hash,
        originalFileName: file.originalFileName,
        duration: file.duration,
        sampleRate: file.sampleRate,
        channels: file.channels,
        storageProvider: file.storageProvider,
        cacheKey: file.cacheKey,
        waveformCacheKeys: file.waveformCacheKeys,
        storageKey: file.storageKey,
      })),
  };
}

const projectStorage: ProjectStorageAdapter = {
  async saveProject(
    project: DawProject,
    _opts?: SaveProjectOptions,
  ): Promise<SaveProjectResult | null> {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(serializeProject(project)));
      return {};
    } catch {
      return null;
    }
  },
  async openProject(): Promise<DawProject | null> {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    try {
      return JSON.parse(raw) as DawProject;
    } catch {
      return null;
    }
  },
};

const dialog: DialogAdapter = {
  async showMessageBox(opts: MessageBoxOptions) {
    const lines: string[] = [];
    if (opts.title) lines.push(opts.title);
    lines.push(opts.message);
    if (opts.detail) lines.push(opts.detail);
    const cancelResponse = opts.cancelId ?? Math.max(0, (opts.buttons?.length ?? 1) - 1);
    if (typeof window !== "undefined") {
      if (opts.buttons && opts.buttons.length > 1) {
        return { response: window.confirm(lines.join("\n\n")) ? (opts.defaultId ?? 0) : cancelResponse };
      }
      window.alert(lines.join("\n\n"));
    }
    return { response: opts.defaultId ?? 0 };
  },
  async showErrorBox(title: string, message: string): Promise<void> {
    if (typeof window !== "undefined") {
      window.alert(`${title}\n\n${message}`);
    }
  },
};

const folderProject: FolderProjectAdapter = {
  isSupported: false,
  getProjectRoot: () => null,
  setProjectRoot: () => { /* no-op on web */ },
  getProjectFilePath: () => null,
  browseLocation: async () => null,
  createProject: async () => null,
  importAudio: async () => null,
  openByPath: async () => null,
  getDefaultProjectsPath: async () => "",
};

const windowAdapter: WindowAdapter = {
  minimize() {
    /* no-op on web */
  },
  toggleMaximize() {
    /* no-op on web */
  },
  close() {
    /* no-op on web */
  },
};

export const webPlatform: Platform = {
  kind: "web",
  capabilities,
  fileSystem,
  projectStorage,
  dialog,
  window: windowAdapter,
  folderProject,
  pluginHost,
};
