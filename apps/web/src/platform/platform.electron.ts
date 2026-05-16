import "./dawBridge.types";
import type { DawProject } from "../types/daw";
import type { DawElectronBridge } from "./dawBridge.types";
import type {
  DialogAdapter,
  FileSystemAdapter,
  MessageBoxOptions,
  Platform,
  PlatformCapabilities,
  ProjectStorageAdapter,
  SaveProjectOptions,
  SaveProjectResult,
  WindowAdapter,
} from "./platform.types";

function bridge(): DawElectronBridge {
  const b = typeof window !== "undefined" ? window.dawElectron : undefined;
  if (!b) {
    throw new Error(
      "electronPlatform invoked but window.dawElectron is missing. " +
        "The Electron preload script did not load.",
    );
  }
  return b;
}

const capabilities: PlatformCapabilities = {
  kind: "electron",
  filesystem: true,
  persistentLocalProjects: true,
  nativeDialogs: true,
  nativeWindowControls: true,
  nativeAudioEngine: false,
  nativePlugins: false,
  webAudio: typeof window !== "undefined" && "AudioContext" in window,
  cloudSync: false,
  osFilePaths: true,
};

const fileSystem: FileSystemAdapter = {
  async pickAudioFiles(): Promise<File[]> {
    const picked = await bridge().fs.pickAudioFiles();
    return picked.map(fileFromPickedAudio);
  },
  async readAudioFile(path: string): Promise<File | null> {
    const picked = await bridge().fs.readAudioFile(path);
    return picked ? fileFromPickedAudio(picked) : null;
  },
  async revealInFileManager(path: string): Promise<void> {
    await bridge().fs.revealInFileManager(path);
  },
};

function fileFromPickedAudio(
  picked: Awaited<ReturnType<DawElectronBridge["fs"]["pickAudioFiles"]>>[number],
): File {
  const file = new File([picked.bytes], picked.name, {
    type: picked.mimeType,
    lastModified: picked.lastModified,
  });
  Object.defineProperty(file, "__futureboardPath", {
    value: picked.path,
    enumerable: false,
    configurable: false,
  });
  return file;
}

// Renderer-side project path tracking. We don't expose this in `DawProject`
// itself; it's purely so a subsequent "Save" (without "Save As") can skip
// the dialog. Cleared when a different project is opened.
let lastProjectPath: string | undefined;

function serializeProject(project: DawProject): DawProject {
  return {
    ...project,
    files: project.files.map((file) => ({
      id: file.id,
      name: file.name,
      mimeType: file.mimeType,
      size: file.size,
      lastModified: file.lastModified,
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
    opts?: SaveProjectOptions,
  ): Promise<SaveProjectResult | null> {
    const b = bridge();
    let targetPath = lastProjectPath;
    if (!targetPath || opts?.saveAs) {
      const result = await b.project.showSaveDialog(`${project.name}.mochiproj`);
      if (result.canceled || !result.path) return null;
      targetPath = result.path;
    }
    const ok = await b.project.write(targetPath, JSON.stringify(serializeProject(project), null, 2));
    if (!ok) return null;
    lastProjectPath = targetPath;
    return { path: targetPath };
  },
  async openProject(): Promise<DawProject | null> {
    const b = bridge();
    const result = await b.project.showOpenDialog();
    if (result.canceled || !result.path) return null;
    const raw = await b.project.read(result.path);
    if (raw == null) return null;
    try {
      const project = JSON.parse(raw) as DawProject;
      lastProjectPath = result.path;
      return project;
    } catch {
      return null;
    }
  },
};

const dialog: DialogAdapter = {
  async showMessageBox(opts: MessageBoxOptions): Promise<void> {
    await bridge().dialog.showMessageBox({
      type: opts.type,
      title: opts.title,
      message: opts.message,
      detail: opts.detail,
      buttons: opts.buttons,
    });
  },
  async showErrorBox(title: string, message: string): Promise<void> {
    await bridge().dialog.showErrorBox(title, message);
  },
};

const windowAdapter: WindowAdapter = {
  minimize() {
    void bridge().window.minimize();
  },
  toggleMaximize() {
    void bridge().window.toggleMaximize();
  },
  close() {
    void bridge().window.close();
  },
};

export const electronPlatform: Platform = {
  kind: "electron",
  capabilities,
  fileSystem,
  projectStorage,
  dialog,
  window: windowAdapter,
};
