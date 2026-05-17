// Ultra-lean sandboxed preload. Avoid heavy imports and any work beyond
// declaring + freezing the bridge object. Everything here runs on the
// renderer's hot startup path.
import { contextBridge, ipcRenderer, webUtils } from "electron";
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
  type SphereDeviceOpenConfig,
  type SphereTransportState,
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
  getPathForFile: (file: File): string =>
    webUtils.getPathForFile(file),
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

/**
 * sphereAudioBridge — safe IPC surface for SphereDirectAudioEngine.
 * Only exposed in the Electron client; the renderer detects its presence via
 * `window.dawElectron?.sphereAudio`.  All methods are async and handle
 * main-process errors as rejected promises.
 */
const sphereAudioBridge = Object.freeze({
  getStatus:          ()                                            => invoke(IpcChannels.SphereAudioGetStatus),
  getVersion:         ()                                            => invoke(IpcChannels.SphereAudioGetVersion),
  listInputDevices:   ()                                            => invoke(IpcChannels.SphereAudioListInputDevices),
  listOutputDevices:  ()                                            => invoke(IpcChannels.SphereAudioListOutputDevices),
  openDevice:         (config: SphereDeviceOpenConfig)              => invoke(IpcChannels.SphereAudioOpenDevice, config),
  closeDevice:        ()                                            => invoke(IpcChannels.SphereAudioCloseDevice),
  start:              ()                                            => invoke(IpcChannels.SphereAudioStart),
  stop:               ()                                            => invoke(IpcChannels.SphereAudioStop),
  setTestTone:        (enabled: boolean, frequency: number)          => invoke(IpcChannels.SphereAudioSetTestTone, enabled, frequency),
  setTransportState:  (state: SphereTransportState)                 => invoke(IpcChannels.SphereAudioSetTransport, state),
  getTransportState:  ()                                            => invoke(IpcChannels.SphereAudioGetTransport),
  updateTrackParam:   (trackId: string, paramId: string, value: unknown)                           => invoke(IpcChannels.SphereAudioUpdateTrackParam, trackId, paramId, value),
  updateInsertParam:  (trackId: string, insertId: string, paramId: string, value: unknown)         => invoke(IpcChannels.SphereAudioUpdateInsertParam, trackId, insertId, paramId, value),
  loadProject:        (snapshot: unknown)                           => invoke(IpcChannels.SphereAudioLoadProject, snapshot),
  updateClip:         (clipId: string, patch: unknown)              => invoke(IpcChannels.SphereAudioUpdateClip, clipId, patch),
  getMeters:          ()                                            => invoke(IpcChannels.SphereAudioGetMeters),
  getDebugInfo:       ()                                            => invoke(IpcChannels.SphereAudioGetDebugInfo),
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
  /** SphereDirectAudioEngine native backend bridge. Presence indicates Electron client. */
  sphereAudio: sphereAudioBridge,
});

contextBridge.exposeInMainWorld("dawElectron", dawElectron);

export type DawElectronBridge = typeof dawElectron;
