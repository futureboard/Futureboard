/**
 * Native audio engine types.
 * Shared by detection, the NativeSphereAudioEngineAdapter, and the Settings UI.
 *
 * SphereDirectAudioEngine is the Rust desktop/native audio backend.
 * It runs as a separate process and is only available in the Electron client.
 */

// ── Backend identity ──────────────────────────────────────────────────────────

export type AudioEngineBackend = "web-audio" | "native-sphere-direct";

export type AudioEngineBackendStatus = {
  backend:      AudioEngineBackend;
  available:    boolean;
  running:      boolean;
  version?:     string;
  /** Human-readable reason when unavailable. */
  reason?:      string;
  sampleRate?:  number;
  bufferSize?:  number;
  inputDevice?: string;
  outputDevice?: string;
};

// ── Device info ───────────────────────────────────────────────────────────────

export type SphereAudioDeviceInfo = {
  id:                string;
  name:              string;
  kind:              "input" | "output" | (string & {});
  channels:          number;
  defaultSampleRate: number;
  isDefault:         boolean;
  backend:           string;
};

// ── Runtime status from the native engine ─────────────────────────────────────

export type SphereAudioStatus = {
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
  cpuLoad?:         number;  // 0–1
  xrunCount?:       number;
};

// ── Meter / transport snapshots ───────────────────────────────────────────────

export type StereoMeterLevel = { left: number; right: number };

export type MeterSnapshot = {
  tracks:    Record<string, StereoMeterLevel> | Array<StereoMeterLevel & { trackId?: string; id?: string }>;
  master:    StereoMeterLevel;
  timestamp: number;
};

export type EngineTransportState = {
  playing:         boolean;
  positionSeconds: number;
  bpm:             number;
};

// ── Project snapshot for native engine ───────────────────────────────────────
// Passed to the native process via IPC.  Media is always sent as file paths
// (never as raw buffers) so the IPC channel stays fast.

export type EngineInsertSnapshot = {
  id:      string;
  type:    string;
  enabled: boolean;
  params:  Record<string, number | string | boolean>;
};

export type EngineSendSnapshot = {
  id:            string;
  returnTrackId: string;
  level:         number;
  enabled:       boolean;
};

export type EngineTrackSnapshot = {
  id:            string;
  type:          string;
  volume:        number;
  pan:           number;
  muted:         boolean;
  solo:          boolean;
  armed:         boolean;
  previewMode:   string;
  outputTrackId: string | null;
  inserts:       EngineInsertSnapshot[];
  sends:         EngineSendSnapshot[];
};

export type EngineFadeSnapshot = {
  inDuration:  number;
  outDuration: number;
  inCurve:     "linear" | "exponential";
  outCurve:    "linear" | "exponential";
};

export type EngineClipAudioProcess = {
  speedRatio:     number;
  pitchSemitones: number;
  preservePitch:  boolean;
  mode:           string;
  quality:        string;
};

export type EngineClipSnapshot = {
  id:           string;
  trackId:      string;
  assetId:      string;
  relativePath?: string | null;
  /**
   * Absolute path to the media file.
   * Only populated in Electron folder-project mode.
   * Web mode leaves this null — native engine is not used on web.
   */
  mediaPath:    string | null;
  startBeat:    number;
  durationBeats: number;
  offsetSeconds: number;
  gain:          number;
  fades:         EngineFadeSnapshot | null;
  audioProcess:  EngineClipAudioProcess | null;
};

export type EngineAssetSnapshot = {
  id: string;
  type: string;
  name: string;
  relativePath: string;
  missing?: boolean;
};

export type EngineRoutingSnapshot = {
  masterOutputDevice: string | null;
  sampleRate:         number;
  bufferSize:         number;
};

export type EngineProjectSnapshot = {
  projectId:     string;
  projectRoot:   string | null;
  bpm:           number;
  timeSignature: [number, number];
  sampleRate:    number;
  tracks:        EngineTrackSnapshot[];
  clips:         EngineClipSnapshot[];
  assets?:       EngineAssetSnapshot[];
  files?:        Array<{
    id: string;
    name: string;
    originalFileName?: string;
    storageProvider?: string;
    relativePath?: string | null;
    cacheKey?: string | null;
    storageKey?: string | null;
  }>;
  routing:       EngineRoutingSnapshot;
};

// ── Device open config ────────────────────────────────────────────────────────

export type SphereDeviceOpenConfig = {
  inputDeviceId?:  string;
  outputDeviceId?: string;
  sampleRate?:     number;
  bufferSize?:     number;
};
