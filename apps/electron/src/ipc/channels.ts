/**
 * IPC channel constants + request/response types.
 * Shared between Electron main (`main.ts`) and renderer preload (`preload.ts`).
 */

export const IpcChannels = {
  FsPickAudioFiles: "daw:fs:pickAudioFiles",
  FsReadAudioFile: "daw:fs:readAudioFile",
  FsStatAudioFile: "daw:fs:statAudioFile",
  FsRevealInFileManager: "daw:fs:revealInFileManager",

  ProjectSaveDialog: "daw:project:saveDialog",
  ProjectOpenDialog: "daw:project:openDialog",
  ProjectRead: "daw:project:read",
  ProjectWrite: "daw:project:write",

  DialogMessageBox: "daw:dialog:messageBox",
  DialogErrorBox: "daw:dialog:errorBox",

  WindowMinimize: "daw:window:minimize",
  WindowToggleMaximize: "daw:window:toggleMaximize",
  WindowClose: "daw:window:close",

  // External floating windows (Electron only)
  WindowsOpenExternal: "daw:windows:openExternal",
  WindowsCloseExternal: "daw:windows:closeExternal",
  WindowsFocusExternal: "daw:windows:focusExternal",

  // Waveform peak cache (Electron only — persists to userData/cache/waveforms)
  WaveformCacheGet: "daw:waveformCache:get",
  WaveformCacheSet: "daw:waveformCache:set",
  WaveformCacheDelete: "daw:waveformCache:delete",
  WaveformCacheClear: "daw:waveformCache:clear",

  // System / diagnostics (Electron only)
  SysGetGpuInfo: "daw:sys:getGpuInfo",

  // Folder-based project operations (Electron only)
  ProjectFolderBrowseLocation: "daw:project:folderBrowseLocation",
  ProjectFolderCreate: "daw:project:folderCreate",
  ProjectFolderSave: "daw:project:folderSave",
  ProjectFolderOpenFile: "daw:project:folderOpenFile",
  ProjectFolderImportAudio: "daw:project:folderImportAudio",
  FsEnsureProjectFolders: "daw:fs:ensureProjectFolders",

  // SphereDirectAudioEngine — native Rust audio backend (Electron only)
  SphereAudioGetStatus:        "daw:sphere:getStatus",
  SphereAudioGetVersion:       "daw:sphere:getVersion",
  SphereAudioListInputDevices: "daw:sphere:listInputDevices",
  SphereAudioListOutputDevices:"daw:sphere:listOutputDevices",
  SphereAudioOpenDevice:       "daw:sphere:openDevice",
  SphereAudioCloseDevice:      "daw:sphere:closeDevice",
  SphereAudioStart:            "daw:sphere:start",
  SphereAudioStop:             "daw:sphere:stop",
  SphereAudioSetTestTone:      "daw:sphere:setTestTone",
  SphereAudioSetTransport:     "daw:sphere:setTransportState",
  SphereAudioGetTransport:     "daw:sphere:getTransportState",
  SphereAudioUpdateTrackParam: "daw:sphere:updateTrackParam",
  SphereAudioUpdateInsertParam:"daw:sphere:updateInsertParam",
  SphereAudioLoadProject:      "daw:sphere:loadProject",
  SphereAudioUpdateClip:       "daw:sphere:updateClip",
  SphereAudioGetMeters:        "daw:sphere:getMeters",
  SphereAudioGetDebugInfo:     "daw:sphere:getDebugInfo",
} as const;

export type IpcChannel = (typeof IpcChannels)[keyof typeof IpcChannels];

export type PickedAudioFile = {
  name: string;
  mimeType: string;
  bytes: ArrayBuffer;
  path: string;
  size: number;
  lastModified: number;
};

export type AudioFileStat = {
  name: string;
  mimeType: string;
  path: string;
  size: number;
  lastModified: number;
};

export type MessageBoxOptions = {
  type?: "none" | "info" | "error" | "question" | "warning";
  title?: string;
  message: string;
  detail?: string;
  buttons?: string[];
};

export type MessageBoxResult = {
  response: number;
};

export type SaveDialogResult = {
  canceled: boolean;
  path?: string;
};

export type OpenDialogResult = {
  canceled: boolean;
  path?: string;
};

export type WaveformCacheEntryIpc = {
  version: number;
  fileId: string;
  fileName?: string;
  fileSize?: number;
  fileLastModified?: number;
  sampleRate: number;
  channelCount: number;
  duration: number;
  samplesPerPeak: number;
  peakCount: number;
  createdAt: number;
  peaks: number[];
};

export type FolderProjectCreateOptions = {
  name: string;
  location: string;
};

export type FolderProjectCreateResult = {
  projectRoot: string;
  projectFilePath: string;
};

export type FolderImportAudioResult = {
  relativePath: string;
  absolutePath: string;
  name: string;
  size: number;
  lastModified: number;
};

export type BrowseFolderResult = {
  canceled: boolean;
  folderPath?: string;
};

export type GpuFeatureStatus = {
  hardwareAccelerationEnabled: boolean;
  features: Record<string, string>;
  gpuDescription: string | null;
  electronVersion: string;
  chromeVersion: string;
};

export type ExternalWindowConfig = {
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
};

// ── SphereDirectAudioEngine IPC types ─────────────────────────────────────────

export type SphereAudioStatus = {
  available:         boolean;
  running:      boolean;
  streamOpen:        boolean;
  transportPlaying:  boolean;
  positionSeconds:   number;
  version:      string;
  backendName?:      string;
  sampleRate:   number;
  bufferSize:   number;
  inputDevice:  string | null;
  outputDevice: string | null;
  lastError?:        string | null;
  cpuLoad:      number;
  xrunCount:    number;
};

export type SphereAudioDeviceInfo = {
  id:                string;
  name:              string;
  kind:              "input" | "output" | (string & {});
  channels:          number;
  defaultSampleRate: number;
  isDefault:         boolean;
  backend:           string;
};

export type SphereDeviceOpenConfig = {
  inputDeviceId?:  string;
  outputDeviceId?: string;
  sampleRate?:     number;
  bufferSize?:     number;
};

export type SphereTransportState = {
  playing?:         boolean;
  positionSeconds?: number;
  loop?:            boolean;
  loopStart?:       number;
  loopEnd?:         number;
};

export type SphereMeterSnapshot = {
  tracks:    Record<string, { left: number; right: number }>;
  master:    { left: number; right: number };
  timestamp: number;
};
