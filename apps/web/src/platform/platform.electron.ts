import "./dawBridge.types";
import type { DawProject } from "../types/daw";
import type { DawElectronBridge } from "./dawBridge.types";
import type {
  DialogAdapter,
  FileSystemAdapter,
  FolderImportAudioResult,
  FolderProjectAdapter,
  MessageBoxOptions,
  Platform,
  PlatformCapabilities,
  ProjectStorageAdapter,
  SaveProjectOptions,
  SaveProjectResult,
  WindowAdapter,
} from "./platform.types";
import { setElectronWaveformCacheProjectRoot } from "../engine/waveformCache";

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
  async statAudioFile(path: string): Promise<{ size: number; lastModified: number; name: string; mimeType: string } | null> {
    return bridge().fs.statAudioFile(path);
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

// Renderer-side project path tracking. We don't expose these in `DawProject`
// itself; they are purely runtime state for save/open operations.
let lastProjectPath: string | undefined;
// Non-null when the current project is a folder-based project (.mochiproj inside a folder).
let currentProjectRoot: string | null = null;
// Timestamp written when the project file was first created (folder mode only).
let folderProjectCreatedAt: number | null = null;

/** Resolve a relative path (forward-slash) against a project root (native path). */
function joinProjectPath(root: string, relPath: string): string {
  const sep = root.includes("\\") ? "\\" : "/";
  return `${root}${sep}${relPath.replace(/\//g, sep)}`;
}

/** Derive the project root from a .mochiproj file path. */
function rootFromFilePath(filePath: string): string {
  const lastSlash = Math.max(filePath.lastIndexOf("/"), filePath.lastIndexOf("\\"));
  return filePath.substring(0, lastSlash);
}

function serializeProject(project: DawProject): object {
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
      relativePath: file.relativePath,
    })),
  };
}

function serializeFolderProject(project: DawProject): object {
  const now = Date.now();
  return {
    schemaVersion: 1,
    createdAt: folderProjectCreatedAt ?? now,
    updatedAt: now,
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
      storageProvider: "project-folder" as const,
      relativePath: file.relativePath,
      waveformCacheKeys: file.waveformCacheKeys,
      // cacheKey and storageKey are absolute paths — do not persist them
    })),
  };
}

function setProjectRootInternal(root: string | null): void {
  currentProjectRoot = root;
  setElectronWaveformCacheProjectRoot(root);
}

function resolveOpenedProject(
  parsed: DawProject & { schemaVersion?: number; createdAt?: number; updatedAt?: number },
  filePath: string,
): DawProject {
  const root = rootFromFilePath(filePath);
  setProjectRootInternal(root);
  lastProjectPath = filePath;
  if (parsed.createdAt != null) folderProjectCreatedAt = parsed.createdAt;

  return {
    ...parsed,
    files: (parsed.files ?? []).map((file) => {
      if (file.relativePath) {
        const absPath = joinProjectPath(root, file.relativePath);
        return {
          ...file,
          storageProvider: "project-folder" as const,
          cacheKey: absPath,
          storageKey: absPath,
          localObjectUrl: undefined,
        };
      }
      return file;
    }),
  };
}

const projectStorage: ProjectStorageAdapter = {
  async saveProject(
    project: DawProject,
    opts?: SaveProjectOptions,
  ): Promise<SaveProjectResult | null> {
    const b = bridge();

    // Folder project mode: save atomically inside the project root
    if (currentProjectRoot && !opts?.saveAs) {
      const ok = await b.project.saveFolderProject(
        currentProjectRoot,
        JSON.stringify(serializeFolderProject(project), null, 2),
      );
      if (!ok) return null;
      return { path: lastProjectPath, projectRoot: currentProjectRoot };
    }

    // Save As for folder project: ask for new location then create folder project
    if (currentProjectRoot && opts?.saveAs) {
      const browseResult = await b.project.browseFolderLocation();
      if (browseResult.canceled || !browseResult.folderPath) return null;
      const createResult = await b.project.createFolderProject({
        name: project.name,
        location: browseResult.folderPath,
      });
      if (!createResult) return null;
      setProjectRootInternal(createResult.projectRoot);
      lastProjectPath = createResult.projectFilePath;
      folderProjectCreatedAt = Date.now();
      const ok = await b.project.saveFolderProject(
        currentProjectRoot,
        JSON.stringify(serializeFolderProject(project), null, 2),
      );
      if (!ok) return null;
      return { path: lastProjectPath, projectRoot: currentProjectRoot };
    }

    // Legacy mode: show save dialog
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
      const parsed = JSON.parse(raw) as DawProject & { schemaVersion?: number; createdAt?: number };
      if (parsed.schemaVersion != null || (parsed.files ?? []).some((f) => f.relativePath)) {
        return resolveOpenedProject(parsed, result.path);
      }
      setProjectRootInternal(null);
      lastProjectPath = result.path;
      folderProjectCreatedAt = null;
      return parsed;
    } catch {
      return null;
    }
  },
};

const folderProject: FolderProjectAdapter = {
  isSupported: true,

  getProjectRoot(): string | null {
    return currentProjectRoot;
  },

  setProjectRoot(root: string | null): void {
    setProjectRootInternal(root);
  },

  getProjectFilePath(): string | null {
    return lastProjectPath ?? null;
  },

  async browseLocation(): Promise<string | null> {
    const result = await bridge().project.browseFolderLocation();
    if (result.canceled || !result.folderPath) return null;
    return result.folderPath;
  },

  async createProject(opts: { name: string; location: string }): Promise<{ projectRoot: string; projectFilePath: string } | null> {
    const result = await bridge().project.createFolderProject(opts);
    if (!result) return null;
    setProjectRootInternal(result.projectRoot);
    lastProjectPath = result.projectFilePath;
    folderProjectCreatedAt = Date.now();
    return result;
  },

  async importAudio(sourcePath: string): Promise<FolderImportAudioResult | null> {
    if (!currentProjectRoot) return null;
    return bridge().project.importAudioToFolder(currentProjectRoot, sourcePath);
  },

  async openByPath(filePath: string): Promise<DawProject | null> {
    const raw = await bridge().project.openFolderFile(filePath);
    if (raw == null) return null;
    try {
      const parsed = JSON.parse(raw) as DawProject & { schemaVersion?: number; createdAt?: number };
      return resolveOpenedProject(parsed, filePath);
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
  folderProject,
};
