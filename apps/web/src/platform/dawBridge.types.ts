/**
 * Type declarations for the `window.dawElectron` bridge exposed by the
 * Electron preload (`apps/electron/src/preload.ts`).
 *
 * Kept manually in sync with the preload surface. Importing this file
 * augments the global `Window` interface so platform adapters can
 * detect and call the bridge without `any`.
 */

export type DawBridgePlatform =
  | "aix"
  | "darwin"
  | "freebsd"
  | "linux"
  | "openbsd"
  | "sunos"
  | "win32"
  | (string & {});

export type DawBridgePickedAudioFile = {
  name: string;
  mimeType: string;
  bytes: ArrayBuffer;
  path: string;
  size: number;
  lastModified: number;
};

export type DawBridgeAudioFileStat = Omit<DawBridgePickedAudioFile, "bytes">;

export type DawBridgeWavPeakResult = {
  fileId: string;
  sampleRate: number;
  channelCount: number;
  duration: number;
  samplesPerPeak: number;
  peakCount: number;
  peaks: number[];
};

export type DawBridgeBrowserRootEntry = {
  id: string;
  name: string;
  path: string;
  kind: "factory" | "factory-folder" | "drive" | "folder";
};

export type DawBridgeBrowserFileEntry = {
  name: string;
  path: string;
  kind: "folder" | "audio" | "file";
  size?: number;
  lastModified?: number;
  mimeType?: string;
};

export type DawBridgeBrowserIndexStatus = {
  rootPath: string;
  dbPath: string;
  status: "idle" | "indexing" | "done" | "error";
  scannedDirs: number;
  scannedFiles: number;
  audioFiles: number;
  currentPath?: string;
  error?: string;
  startedAt?: number;
  updatedAt?: number;
  finishedAt?: number;
};

export type DawBridgeMessageBoxKind =
  | "none"
  | "info"
  | "error"
  | "question"
  | "warning";

export type DawBridgeMessageBoxOptions = {
  type?: DawBridgeMessageBoxKind;
  title?: string;
  message: string;
  detail?: string;
  buttons?: string[];
  defaultId?: number;
  cancelId?: number;
};

export type DawBridgeMessageBoxResult = {
  response: number;
};

export type DawBridgeSaveDialogResult = {
  canceled: boolean;
  path?: string;
};

export type DawBridgeOpenDialogResult = {
  canceled: boolean;
  path?: string;
};

export interface DawBridgeFs {
  pickAudioFiles(): Promise<DawBridgePickedAudioFile[]>;
  readAudioFile(path: string): Promise<DawBridgePickedAudioFile | null>;
  statAudioFile(path: string): Promise<DawBridgeAudioFileStat | null>;
  generateWavPeaks(path: string, fileId: string, samplesPerPeak: number): Promise<DawBridgeWavPeakResult | null>;
  browserRoots(): Promise<DawBridgeBrowserRootEntry[]>;
  browserListDir(path: string): Promise<DawBridgeBrowserFileEntry[]>;
  ensureFactoryLibrary(): Promise<DawBridgeBrowserRootEntry[]>;
  browserIndexStart(path: string): Promise<DawBridgeBrowserIndexStatus>;
  browserIndexStatus(paths?: string[]): Promise<DawBridgeBrowserIndexStatus[]>;
  getPathForFile(file: File): string;
  revealInFileManager(path: string): Promise<void>;
}

export type DawBridgeFolderCreateOptions = {
  name: string;
  location: string;
};

export type DawBridgeFolderCreateResult = {
  projectRoot: string;
  projectFilePath: string;
};

export type DawBridgeFolderImportResult = {
  relativePath: string;
  absolutePath: string;
  name: string;
  size: number;
  lastModified: number;
};

export type DawBridgeBrowseFolderResult = {
  canceled: boolean;
  folderPath?: string;
};

export interface DawBridgeProject {
  showSaveDialog(suggestedName?: string): Promise<DawBridgeSaveDialogResult>;
  showOpenDialog(): Promise<DawBridgeOpenDialogResult>;
  read(path: string): Promise<string | null>;
  write(path: string, contents: string): Promise<boolean>;
  // Folder project operations
  browseFolderLocation(): Promise<DawBridgeBrowseFolderResult>;
  createFolderProject(options: DawBridgeFolderCreateOptions): Promise<DawBridgeFolderCreateResult | null>;
  saveFolderProject(projectRoot: string, contents: string): Promise<boolean>;
  openFolderFile(filePath: string): Promise<string | null>;
  importAudioToFolder(projectRoot: string, sourcePath: string): Promise<DawBridgeFolderImportResult | null>;
}

export type DawBridgeGpuMode = "auto" | "force" | "software";

export type DawBridgeGpuFeatureStatus = {
  hardwareAccelerationEnabled: boolean;
  gpuMode: DawBridgeGpuMode;
  features: Record<string, string>;
  gpuDescription: string | null;
  electronVersion: string;
  chromeVersion: string;
};

/** Settings persisted to futureboard-settings.json (Electron only). */
export type DawBridgeElectronSettings = {
  graphicRenderingMode: "auto" | "force" | "software";
};

export interface DawBridgeSys {
  getGpuInfo(): Promise<DawBridgeGpuFeatureStatus>;
  readElectronSettings(): Promise<DawBridgeElectronSettings>;
  writeElectronSettings(settings: DawBridgeElectronSettings): Promise<void>;
  /** Returns the OS path for the default projects folder (Documents/Futureboard Studio/Projects). */
  getDefaultProjectsPath(): Promise<string>;
}

export type DawBridgeAudioPluginKind = "effect" | "instrument";

export type DawBridgeAudioPluginRegistryEntry = {
  id: string;
  name: string;
  vendor: string;
  format: "VST3" | "CLAP" | (string & {});
  category: string;
  rawCategory?: string;
  subCategories?: string;
  kind: DawBridgeAudioPluginKind;
  path: string;
  classId?: string;
  version?: string;
  sdkMetadataLoaded: boolean;
  presetPath: string;
  scannedAt: number;
};

export type DawBridgeAudioPluginHostStatus = {
  available: boolean;
  backend: string;
  message: string;
  dbPath: string;
  presetRoot: string;
  defaultScanPaths: string[];
};

export type DawBridgeAudioPluginScanResult = {
  status: DawBridgeAudioPluginHostStatus;
  plugins: DawBridgeAudioPluginRegistryEntry[];
  scannedPaths: string[];
  generatedPresets: number;
  failed: Array<{ path: string; error: string }>;
};

export type DawBridgeAudioPluginScanProgressEvent =
  | {
      type: "started";
      status: DawBridgeAudioPluginHostStatus;
      scannedPaths: string[];
    }
  | {
      type: "plugin";
      plugin: DawBridgeAudioPluginRegistryEntry;
      generatedPresets: number;
    }
  | {
      type: "folder";
      path: string;
      discovered: number;
    }
  | {
      type: "failed";
      path: string;
      error: string;
    }
  | {
      type: "complete";
      result: DawBridgeAudioPluginScanResult;
    };

export type DawBridgePluginEditorWindowOpenOptions = {
  windowId: string;
  title: string;
  subtitle?: string;
  width?: number;
  height?: number;
  pluginPath?: string;
  classId?: string;
  format?: string;
};

export interface DawBridgePluginHost {
  getStatus(): Promise<DawBridgeAudioPluginHostStatus>;
  listPlugins(): Promise<DawBridgeAudioPluginRegistryEntry[]>;
  scanVst3(paths?: string[]): Promise<DawBridgeAudioPluginScanResult>;
  onScanProgress(callback: (event: DawBridgeAudioPluginScanProgressEvent) => void): () => void;
  revealPreset(pluginId: string): Promise<void>;
  openEditorWindow(options: DawBridgePluginEditorWindowOpenOptions): Promise<number | null>;
  openEditorForPath(pluginPath: string): Promise<number | null>;
  closeEditorWindow(handle: number): Promise<void>;
  focusEditorWindow?(handle: number): Promise<void>;
  resizeEditorWindow?(handle: number, width: number, height: number): Promise<void>;
}

export interface DawBridgePeakChunk {
  read(fileId: string, spp: number, chunkIndex: number, projectRoot: string): Promise<ArrayBuffer | null>;
  write(fileId: string, spp: number, chunkIndex: number, data: ArrayBuffer, projectRoot: string): Promise<void>;
}

export interface DawBridgeDialog {
  showMessageBox(
    options: DawBridgeMessageBoxOptions,
  ): Promise<DawBridgeMessageBoxResult>;
  showErrorBox(title: string, message: string): Promise<void>;
}

export interface DawBridgeWindow {
  minimize(): Promise<void>;
  toggleMaximize(): Promise<void>;
  close(): Promise<void>;
  forceClose(): Promise<void>;
}

export interface DawBridgeExternalWindows {
  openExternal(config: {
    id?: string;
    title: string;
    contentType: string;
    payload?: Record<string, unknown>;
    width: number;
    height: number;
    minWidth?: number;
    minHeight?: number;
    alwaysOnTop?: boolean;
    frame?: boolean;
    transparent?: boolean;
    resizable?: boolean;
    maximizable?: boolean;
  }): Promise<string | null>;
  closeExternal(id: string): Promise<void>;
  focusExternal(id: string): Promise<void>;
}

// ── SphereDirectAudioEngine bridge ────────────────────────────────────────────

export type DawBridgeSphereDeviceOpenConfig = {
  inputDeviceId?:  string;
  outputDeviceId?: string;
  sampleRate?:     number;
  bufferSize?:     number;
};

export type DawBridgeSphereTransportState = {
  playing?:         boolean;
  positionSeconds?: number;
  loop?:            boolean;
  loopStart?:       number;
  loopEnd?:         number;
};

export type DawBridgeSphereAudioStatus = {
  available:        boolean;
  running:          boolean;
  streamOpen:       boolean;
  transportPlaying: boolean;
  positionSeconds:  number;
  version:          string;
  backendName?:     string;
  sampleRate:       number;
  bufferSize:       number;
  inputDevice:      string | null;
  outputDevice:     string | null;
  lastError?:       string | null;
  cpuLoad?:         number;
  xrunCount?:       number;
};

export type DawBridgeSphereDeviceInfo = {
  id:                string;
  name:              string;
  kind:              "input" | "output" | (string & {});
  channels:          number;
  defaultSampleRate: number;
  isDefault:         boolean;
  backend:           string;
};

export type DawBridgeSphereMeterSnapshot = {
  tracks:    Record<string, { left: number; right: number }>;
  master:    { left: number; right: number };
  timestamp: number;
};

export type DawBridgeSphereDebugInfo = {
  projectId:       string | null;
  loadedTracks:    number;
  loadedClips:     number;
  readyClips:      number;
  isPlaying:       boolean;
  positionSeconds: number;
  hasSolo:         boolean;
  clipSummaries:   string[];
  insertSummaries: string[];
};

// ── DAUx backend selection types ──────────────────────────────────────────────

export type DawBridgeDauxBackendInfo = {
  /** Machine-readable id: "auto" | "wasapi-shared" | "wasapi-exclusive" | "coreaudio" | "alsa" | "mme" */
  id:          string;
  name:        string;
  available:   boolean;
  isDefault:   boolean;
  description: string;
};

export type DawBridgeDauxConfig = {
  /** Backend id from DawBridgeDauxBackendInfo.id */
  backendId:       string;
  /** Output device name/id — omit for system default */
  outputDeviceId?: string;
  /** Sample rate in Hz — omit for device default */
  sampleRate?:     number;
  /** Buffer size in frames — omit for driver default */
  bufferSize?:     number;
  /** Enable MMCSS "Pro Audio" thread priority (Windows only) */
  mmcssPriority?:  boolean;
  /** Use larger buffer for glitch-prone systems */
  safeMode?:       boolean;
};

export type DawBridgeDauxStatus = {
  backendId:           string;
  backendName:         string;
  outputDevice:        string | null;
  sampleRate:          number;
  bufferSize:          number;
  /** Estimated output latency in milliseconds */
  estimatedLatencyMs:  number;
  /** Number of underruns/glitches since stream open */
  glitchCount:         number;
  /** MMCSS priority active on audio thread (Windows only) */
  mmcssActive:         boolean;
  /** Last backend error (e.g. WASAPI Exclusive failed reason). Null when engine is healthy. */
  lastError?:          string | null;
};

/**
 * SphereDirectAudioEngine preload bridge.
 * Present only in the Electron client.  Renderer code must check for its
 * existence before calling any method.
 */
export interface DawBridgeSphereAudio {
  getStatus():                                                               Promise<DawBridgeSphereAudioStatus>;
  getVersion():                                                              Promise<string>;
  listInputDevices():                                                        Promise<DawBridgeSphereDeviceInfo[]>;
  listOutputDevices():                                                       Promise<DawBridgeSphereDeviceInfo[]>;
  openDevice(config: DawBridgeSphereDeviceOpenConfig):                       Promise<void>;
  closeDevice():                                                             Promise<void>;
  start():                                                                   Promise<void>;
  stop():                                                                    Promise<void>;
  setTestTone(enabled: boolean, frequency: number):                          Promise<void>;
  setTransportState(state: DawBridgeSphereTransportState):                   Promise<void>;
  getTransportState():                                                       Promise<{ playing: boolean; positionSeconds: number }>;
  updateTrackParam(trackId: string, paramId: string, value: unknown):        Promise<void>;
  updateInsertParam(trackId: string, insertId: string, paramId: string, value: unknown): Promise<void>;
  openInsertEditor(options: {
    trackId: string;
    insertId: string;
    windowId: string;
    title: string;
    width?: number;
    height?: number;
  }):                                                                        Promise<number | null>;
  closeInsertEditor(trackId: string, insertId: string):                      Promise<void>;
  focusInsertEditor(trackId: string, insertId: string):                      Promise<boolean>;
  loadProject(snapshot: unknown):                                            Promise<void>;
  updateClip(clipId: string, patch: unknown):                                Promise<void>;
  getMeters():                                                               Promise<DawBridgeSphereMeterSnapshot>;
  getDebugInfo():                                                            Promise<DawBridgeSphereDebugInfo>;
  // DAUx backend selection
  listDauxBackends():                                                        Promise<DawBridgeDauxBackendInfo[]>;
  openDaux(config: DawBridgeDauxConfig):                                     Promise<void>;
  /** Safe variant: restores previous backend if new config fails. */
  openDauxSafe(config: DawBridgeDauxConfig):                                 Promise<void>;
  getDauxStatus():                                                           Promise<DawBridgeDauxStatus>;
}

export type FloatingWindowKind = "Mixer" | "Midi" | "Analyzer" | "PluginEditorPlaceholder";

export interface DawBridgeFloatingWindow {
  open(req: { id: string; kind: FloatingWindowKind; title: string; alwaysOnTop?: boolean }): Promise<boolean>;
  close(id: string): Promise<void>;
  focus(id: string): Promise<void>;
  updateMixer(req: {
    tracks: Array<{
      id: string;
      name: string;
      color: string;
      volume: number;
      pan: number;
      mute: boolean;
      solo: boolean;
      armed: boolean;
      meterL?: number;
      meterR?: number;
    }>;
    master: {
      volume: number;
      meterL?: number;
      meterR?: number;
    };
  }): Promise<void>;
}

export interface FutureboardCommandBridge {
  onCommand(callback: (commandId: string) => void): () => void;
}

export interface FutureboardBridge {
  commands: FutureboardCommandBridge;
}

export interface DawElectronBridge {
  /** Legacy/back-compat surface preserved for existing renderer consumers. */
  platform: DawBridgePlatform;
  frameless: boolean;
  transparentWindow: boolean;
  windowControlsOverlayEnabled: boolean;

  fs: DawBridgeFs;
  project: DawBridgeProject;
  dialog: DawBridgeDialog;
  window: DawBridgeWindow;
  windows: DawBridgeExternalWindows;
  sys: DawBridgeSys;
  pluginHost?: DawBridgePluginHost;

  /** Binary peak chunk files (Project/Cache/Peaks/). Present only in Electron client. */
  peakChunk?: DawBridgePeakChunk;
  /** SphereDirectAudioEngine native backend. Present only in Electron client. */
  sphereAudio: DawBridgeSphereAudio;
  /** Native floating window runtime (Rust/egui binary). Present only in Electron client. */
  floatingWindow?: DawBridgeFloatingWindow;
}

declare global {
  interface Window {
    dawElectron?: DawElectronBridge;
    futureboard?: FutureboardBridge;
  }
}

export {};
