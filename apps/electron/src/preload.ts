// Ultra-lean sandboxed preload. Avoid heavy imports and any work beyond
// declaring + freezing the bridge object. Everything here runs on the
// renderer's hot startup path.
import { contextBridge, ipcRenderer } from "electron";
import {
  IpcChannels,
  type ExternalWindowConfig,
  type MessageBoxOptions,
  type MessageBoxResult,
  type OpenDialogResult,
  type PickedAudioFile,
  type SaveDialogResult,
  type WaveformCacheEntryIpc,
  type FolderProjectCreateOptions,
  type FolderProjectCreateResult,
  type FolderImportAudioResult,
  type BrowseFolderResult,
  type GpuFeatureStatus,
} from "./ipc/channels.js";

const invoke = ipcRenderer.invoke.bind(ipcRenderer);
const isMac = process.platform === "darwin";

const fsBridge = Object.freeze({
  pickAudioFiles: (): Promise<PickedAudioFile[]> =>
    invoke(IpcChannels.FsPickAudioFiles),
  readAudioFile: (filePath: string): Promise<PickedAudioFile | null> =>
    invoke(IpcChannels.FsReadAudioFile, filePath),
  statAudioFile: (filePath: string) =>
    invoke(IpcChannels.FsStatAudioFile, filePath),
  revealInFileManager: (filePath: string): Promise<void> =>
    invoke(IpcChannels.FsRevealInFileManager, filePath),
});

const projectBridge = Object.freeze({
  showSaveDialog: (suggestedName?: string): Promise<SaveDialogResult> =>
    invoke(IpcChannels.ProjectSaveDialog, suggestedName),
  showOpenDialog: (): Promise<OpenDialogResult> =>
    invoke(IpcChannels.ProjectOpenDialog),
  read: (filePath: string): Promise<string | null> =>
    invoke(IpcChannels.ProjectRead, filePath),
  write: (filePath: string, contents: string): Promise<boolean> =>
    invoke(IpcChannels.ProjectWrite, filePath, contents),
  // Folder-based project operations
  browseFolderLocation: (): Promise<BrowseFolderResult> =>
    invoke(IpcChannels.ProjectFolderBrowseLocation),
  createFolderProject: (options: FolderProjectCreateOptions): Promise<FolderProjectCreateResult | null> =>
    invoke(IpcChannels.ProjectFolderCreate, options),
  saveFolderProject: (projectRoot: string, contents: string): Promise<boolean> =>
    invoke(IpcChannels.ProjectFolderSave, projectRoot, contents),
  openFolderFile: (filePath: string): Promise<string | null> =>
    invoke(IpcChannels.ProjectFolderOpenFile, filePath),
  importAudioToFolder: (projectRoot: string, sourcePath: string): Promise<FolderImportAudioResult | null> =>
    invoke(IpcChannels.ProjectFolderImportAudio, projectRoot, sourcePath),
});

const dialogBridge = Object.freeze({
  showMessageBox: (options: MessageBoxOptions): Promise<MessageBoxResult> =>
    invoke(IpcChannels.DialogMessageBox, options),
  showErrorBox: (title: string, message: string): Promise<void> =>
    invoke(IpcChannels.DialogErrorBox, title, message),
});

const windowBridge = Object.freeze({
  minimize: (): Promise<void> => invoke(IpcChannels.WindowMinimize),
  toggleMaximize: (): Promise<void> => invoke(IpcChannels.WindowToggleMaximize),
  close: (): Promise<void> => invoke(IpcChannels.WindowClose),
});

const waveformCacheBridge = Object.freeze({
  get: (key: string, projectRoot?: string): Promise<WaveformCacheEntryIpc | null> =>
    invoke(IpcChannels.WaveformCacheGet, key, projectRoot),
  set: (key: string, entry: WaveformCacheEntryIpc, projectRoot?: string): Promise<void> =>
    invoke(IpcChannels.WaveformCacheSet, key, entry, projectRoot),
  delete: (key: string, projectRoot?: string): Promise<void> =>
    invoke(IpcChannels.WaveformCacheDelete, key, projectRoot),
  clear: (): Promise<void> =>
    invoke(IpcChannels.WaveformCacheClear),
});

const windowsBridge = Object.freeze({
  openExternal: (config: ExternalWindowConfig): Promise<string | null> =>
    invoke(IpcChannels.WindowsOpenExternal, config),
  closeExternal: (id: string): Promise<void> =>
    invoke(IpcChannels.WindowsCloseExternal, id),
  focusExternal: (id: string): Promise<void> =>
    invoke(IpcChannels.WindowsFocusExternal, id),
});

const sysBridge = Object.freeze({
  getGpuInfo: (): Promise<GpuFeatureStatus> =>
    invoke(IpcChannels.SysGetGpuInfo),
});

const dawElectron = Object.freeze({
  platform: process.platform,
  frameless: true,
  transparentWindow: true,
  // `titleBarOverlay` in main enables Chromium Window Controls Overlay on
  // Windows / Linux. Hidden titlebar on macOS uses the native traffic lights.
  windowControlsOverlayEnabled: !isMac,
  fs: fsBridge,
  project: projectBridge,
  dialog: dialogBridge,
  window: windowBridge,
  windows: windowsBridge,
  waveformCache: waveformCacheBridge,
  sys: sysBridge,
});

contextBridge.exposeInMainWorld("dawElectron", dawElectron);

export type DawElectronBridge = typeof dawElectron;
