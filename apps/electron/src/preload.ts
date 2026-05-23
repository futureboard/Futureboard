// Ultra-lean sandboxed preload. Avoid heavy imports and any work beyond
// declaring + freezing the bridge object. Everything here runs on the
// renderer's hot startup path.
import { contextBridge, ipcRenderer, webUtils, type IpcRendererEvent } from "electron";
import {
  IpcChannels,
  type ExternalWindowConfig,
  type FloatingWindowMixerUpdateRequest,
  type FloatingWindowOpenRequest,
  type MessageBoxOptions,
  type MessageBoxResult,
  type OpenDialogResult,
  type PickedAudioFile,
  type SaveDialogResult,
  type WaveformCacheEntryIpc,
  type WavPeakResult,
  type FolderProjectCreateOptions,
  type FolderProjectCreateResult,
  type FolderImportAudioResult,
  type BrowseFolderResult,
  type BrowserFileEntry,
  type BrowserIndexStatus,
  type BrowserRootEntry,
  type GpuFeatureStatus,
  type ElectronPersistedSettings,
  type AudioPluginHostStatus,
  type AudioPluginRegistryEntry,
  type AudioPluginScanProgressEvent,
  type AudioPluginScanResult,
  type PluginEditorWindowOpenOptions,
  type SphereDeviceOpenConfig,
  type SphereTransportState,
  type SphereDauxConfig,
  type SphereStartRecordingConfig,
  type SphereRecordingResult,
  type SphereRecordingStatus,
} from "./ipc/channels.js";

const invoke = ipcRenderer.invoke.bind(ipcRenderer);
const isMac = process.platform === "darwin";
const APP_COMMAND_CHANNEL = "app-command";

// Buffer app-command events that arrive before the renderer registers its
// onCommand listener (e.g. the startup file-open arg from tryOpenFileArg fires
// during ready-to-show, which can precede App.tsx's useEffect mount).
const _earlyCommands: string[] = [];
let _earlyAppCmdListener: ((_e: IpcRendererEvent, cmd: unknown) => void) | null =
  (_e: IpcRendererEvent, cmd: unknown) => {
    if (typeof cmd === "string") _earlyCommands.push(cmd);
  };
ipcRenderer.on(APP_COMMAND_CHANNEL, _earlyAppCmdListener);

const fsBridge = Object.freeze({
  pickAudioFiles: (): Promise<PickedAudioFile[]> =>
    invoke(IpcChannels.FsPickAudioFiles),
  readAudioFile: (filePath: string): Promise<PickedAudioFile | null> =>
    invoke(IpcChannels.FsReadAudioFile, filePath),
  statAudioFile: (filePath: string) =>
    invoke(IpcChannels.FsStatAudioFile, filePath),
  generateWavPeaks: (filePath: string, fileId: string, samplesPerPeak: number): Promise<WavPeakResult | null> =>
    invoke(IpcChannels.FsGenerateWavPeaks, filePath, fileId, samplesPerPeak),
  browserRoots: (): Promise<BrowserRootEntry[]> =>
    invoke(IpcChannels.FsBrowserRoots),
  browserListDir: (dirPath: string): Promise<BrowserFileEntry[]> =>
    invoke(IpcChannels.FsBrowserListDir, dirPath),
  ensureFactoryLibrary: (): Promise<BrowserRootEntry[]> =>
    invoke(IpcChannels.FsEnsureFactoryLibrary),
  browserIndexStart: (rootPath: string): Promise<BrowserIndexStatus> =>
    invoke(IpcChannels.FsBrowserIndexStart, rootPath),
  browserIndexStatus: (rootPaths?: string[]): Promise<BrowserIndexStatus[]> =>
    invoke(IpcChannels.FsBrowserIndexStatus, rootPaths),
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
  forceClose: (): Promise<void> => invoke(IpcChannels.WindowForceClose),
});

const peakChunkBridge = Object.freeze({
  read: (fileId: string, spp: number, chunkIndex: number, projectRoot: string): Promise<ArrayBuffer | null> =>
    invoke(IpcChannels.PeakChunkRead, fileId, spp, chunkIndex, projectRoot),
  write: (fileId: string, spp: number, chunkIndex: number, data: ArrayBuffer, projectRoot: string): Promise<void> =>
    invoke(IpcChannels.PeakChunkWrite, fileId, spp, chunkIndex, data, projectRoot),
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
  readElectronSettings: (): Promise<ElectronPersistedSettings> =>
    invoke(IpcChannels.SysReadElectronSettings),
  writeElectronSettings: (settings: ElectronPersistedSettings): Promise<void> =>
    invoke(IpcChannels.SysWriteElectronSettings, settings),
  getDefaultProjectsPath: (): Promise<string> =>
    invoke(IpcChannels.SysGetDefaultProjectsPath),
});

const pluginHostBridge = Object.freeze({
  getStatus: (): Promise<AudioPluginHostStatus> =>
    invoke(IpcChannels.PluginHostGetStatus),
  listPlugins: (): Promise<AudioPluginRegistryEntry[]> =>
    invoke(IpcChannels.PluginHostListPlugins),
  scanVst3: (paths?: string[]): Promise<AudioPluginScanResult> =>
    invoke(IpcChannels.PluginHostScanVst3, paths),
  onScanProgress: (callback: (event: AudioPluginScanProgressEvent) => void): (() => void) => {
    const listener = (_event: IpcRendererEvent, payload: unknown) => {
      callback(payload as AudioPluginScanProgressEvent);
    };
    ipcRenderer.on(IpcChannels.PluginHostScanProgress, listener);
    return () => ipcRenderer.removeListener(IpcChannels.PluginHostScanProgress, listener);
  },
  revealPreset: (pluginId: string): Promise<void> =>
    invoke(IpcChannels.PluginHostRevealPreset, pluginId),
  openEditorWindow: (options: PluginEditorWindowOpenOptions): Promise<number | null> =>
    invoke(IpcChannels.PluginHostOpenEditorWindow, options),
  openEditorForPath: (pluginPath: string): Promise<number | null> =>
    invoke(IpcChannels.PluginHostOpenEditorForPath, pluginPath),
  closeEditorWindow: (handle: number): Promise<void> =>
    invoke(IpcChannels.PluginHostCloseEditorWindow, handle),
  focusEditorWindow: (handle: number): Promise<void> =>
    invoke(IpcChannels.PluginHostFocusEditorWindow, handle),
  resizeEditorWindow: (handle: number, width: number, height: number): Promise<void> =>
    invoke(IpcChannels.PluginHostResizeEditorWindow, { handle, width, height }),
});

const floatingWindowBridge = Object.freeze({
  open:  (req: FloatingWindowOpenRequest): Promise<boolean> => invoke(IpcChannels.FloatingWindowOpen, req),
  close: (id: string): Promise<void>                        => invoke(IpcChannels.FloatingWindowClose, id),
  focus: (id: string): Promise<void>                        => invoke(IpcChannels.FloatingWindowFocus, id),
  updateMixer: (req: FloatingWindowMixerUpdateRequest): Promise<void> =>
    invoke(IpcChannels.FloatingWindowMixerUpdate, req),
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
  openInsertEditor:   (options: { trackId: string; insertId: string; windowId: string; title: string; width?: number; height?: number }) =>
    invoke(IpcChannels.SphereAudioOpenInsertEditor, options) as Promise<number | null>,
  closeInsertEditor:  (trackId: string, insertId: string) => invoke(IpcChannels.SphereAudioCloseInsertEditor, trackId, insertId) as Promise<void>,
  focusInsertEditor:  (trackId: string, insertId: string) => invoke(IpcChannels.SphereAudioFocusInsertEditor, trackId, insertId) as Promise<boolean>,
  loadProject:        (snapshot: unknown)                           => invoke(IpcChannels.SphereAudioLoadProject, snapshot),
  updateClip:         (clipId: string, patch: unknown)              => invoke(IpcChannels.SphereAudioUpdateClip, clipId, patch),
  getMeters:          ()                                            => invoke(IpcChannels.SphereAudioGetMeters),
  getDebugInfo:       ()                                            => invoke(IpcChannels.SphereAudioGetDebugInfo),
  // DAUx backend selection
  listDauxBackends:   ()                                            => invoke(IpcChannels.SphereAudioListDauxBackends),
  openDaux:           (config: SphereDauxConfig)                    => invoke(IpcChannels.SphereAudioOpenDaux, config),
  openDauxSafe:       (config: SphereDauxConfig)                    => invoke(IpcChannels.SphereAudioOpenDauxSafe, config),
  getDauxStatus:      ()                                            => invoke(IpcChannels.SphereAudioGetDauxStatus),
  // Recording
  startRecording:     (config: SphereStartRecordingConfig)          => invoke(IpcChannels.SphereAudioStartRecording, config) as Promise<void>,
  stopRecording:      ()                                            => invoke(IpcChannels.SphereAudioStopRecording) as Promise<SphereRecordingResult[]>,
  getRecordingStatus: ()                                            => invoke(IpcChannels.SphereAudioGetRecordingStatus) as Promise<SphereRecordingStatus>,
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
  peakChunk: peakChunkBridge,
  waveformCache: waveformCacheBridge,
  sys: sysBridge,
  pluginHost: pluginHostBridge,
  /** SphereDirectAudioEngine native backend bridge. Presence indicates Electron client. */
  sphereAudio: sphereAudioBridge,
  /** Native floating window runtime (Rust/egui binary). */
  floatingWindow: floatingWindowBridge,
});

contextBridge.exposeInMainWorld("dawElectron", dawElectron);
contextBridge.exposeInMainWorld("futureboard", Object.freeze({
  commands: Object.freeze({
    onCommand: (callback: (commandId: string) => void): (() => void) => {
      // Remove the early-buffer listener and drain any buffered commands.
      if (_earlyAppCmdListener) {
        ipcRenderer.removeListener(APP_COMMAND_CHANNEL, _earlyAppCmdListener);
        _earlyAppCmdListener = null;
        for (const cmd of _earlyCommands.splice(0)) {
          // Defer via setTimeout so the command runs after the useEffect
          // that registered this handler has fully returned.
          setTimeout(() => callback(cmd), 0);
        }
      }
      const listener = (_event: IpcRendererEvent, commandId: unknown) => {
        if (typeof commandId === "string") callback(commandId);
      };
      ipcRenderer.on(APP_COMMAND_CHANNEL, listener);
      return () => ipcRenderer.removeListener(APP_COMMAND_CHANNEL, listener);
    },
  }),
}));

export type DawElectronBridge = typeof dawElectron;
