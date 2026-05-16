export type ProjectId = string;
export type TrackId = string;
export type ClipId = string;
export type FileId = string;

export type TimeSignature = {
  numerator: number;
  denominator: number;
};

export type ProjectLoop = {
  enabled: boolean;
  startTime: number;
  endTime: number;
};

export type ProjectMarker = {
  id: string;
  time: number;
  label: string;
  color?: string;
};

export type DawProject = {
  id: ProjectId;
  name: string;
  version: number;
  sampleRate: number;
  bpm: number;
  timeSignature: TimeSignature;
  tracks: DawTrack[];
  files: DawFile[];
  masterTrackId?: TrackId;
  loop?: ProjectLoop;
  markers?: ProjectMarker[];
};

export type TrackType = "audio" | "midi" | "instrument" | "plugin" | "bus" | "return" | "group" | "master";

// Snap division for grid snapping
export type SnapDivision =
  | "off"
  | "1bar"
  | "1/2"
  | "1/4"
  | "1/8"
  | "1/16"
  | "1/32"
  | "1/64";

// Insert device types supported by WebAudio first pass
export type InsertDeviceType =
  | "eq"
  | "compressor"
  | "delay"
  | "reverb"
  | "saturator"
  | "limiter"
  | "gain"
  | "custom";

export type InsertDevice = {
  id: string;
  type: InsertDeviceType | string;
  name: string;
  /** false = bypassed/disabled */
  enabled: boolean;
  order: number;
  params: Record<string, number | string | boolean>;
};

/** @deprecated Use InsertDevice */
export type TrackInsert = InsertDevice;

export type TrackSend = {
  id: string;
  name: string;
  /** Target bus/return track ID to receive this send's audio. */
  targetTrackId: string;
  /** Send level 0–1 (1 = 0 dB). */
  level: number;
  enabled?: boolean;
  preFader?: boolean;
};

export type DawTrack = {
  id: TrackId;
  name: string;
  type: TrackType;
  color: string;
  channelCount: number;
  volume: number;
  pan: number;
  muted: boolean;
  solo: boolean;
  armed: boolean;
  clips: DawClip[];
  inserts?: InsertDevice[];
  sends?: TrackSend[];
  /** Output routing target: "master" or a bus/group track ID. Defaults to "master". */
  output?: string;
  /** Display height in pixels (overrides TRACK_HEIGHT default). */
  height?: number;
  /** Whether the track lane is collapsed to minimum height. */
  collapsed?: boolean;
  /** Automation lanes attached to this track. */
  automationLanes?: AutomationLane[];
};

export type ClipType = "audio" | "midi";

export type AudioProcessQuality = "draft" | "balanced" | "high";

/**
 * Processing mode for pitch/time stretching.
 * Each mode routes to a different DSP algorithm or configuration.
 */
export type AudioPitchMode =
  | "resample"    // tape-speed: no pitch preservation, speedRatio changes pitch+duration
  | "monophonic"  // WSOLA — first-pass; will get PSOLA later
  | "polyphonic"  // WSOLA with cross-correlation grain search (default)
  | "percussive"  // short-grain OLA, transient-friendly
  | "granular";   // classic OLA, designed for texture/sound-design

export type AudioClipProcess = {
  speedRatio: number;
  pitchSemitones: number;
  preservePitch: boolean;
  /** @default "polyphonic" */
  mode: AudioPitchMode;
  quality: AudioProcessQuality;
};

export type MidiNote = {
  id: string;
  pitch: number;    // 0–127
  start: number;    // seconds from clip start
  duration: number; // seconds
  velocity: number; // 1–127
};

export type DawClip = {
  id: ClipId;
  name: string;
  type?: ClipType;  // defaults to "audio" for backwards-compat
  fileId: FileId;
  notes?: MidiNote[];
  trackId: TrackId;
  startTime: number;
  offset: number;
  duration: number;
  gain: number;
  fadeIn?: number;
  fadeOut?: number;
  color?: string;
  muted?: boolean;
  locked?: boolean;
  audioProcess?: AudioClipProcess;
};

/** Returns the effective clip type, defaulting to "audio" for pre-existing clips. */
export function clipType(clip: DawClip): ClipType {
  return clip.type ?? (clip.fileId ? "audio" : "midi");
}

export type DawFile = {
  id: FileId;
  name: string;
  mimeType: string;
  size?: number;
  lastModified?: number;
  originalFileName?: string;
  duration: number;
  sampleRate: number;
  channels: number;
  storageProvider?: "indexeddb" | "opfs" | "file-handle" | "missing";
  cacheKey?: string;
  waveformCacheKeys?: string[];
  storageKey?: string;
  localObjectUrl?: string;
};

export type WaveformPeaks = {
  fileId?: FileId;
  channel?: number;
  samplesPerPeak: number;
  channelCount: number;
  /** Interleaved min/max per peak, per channel: [ch0_min, ch0_max, ch1_min, ch1_max, ...] */
  peaks: Float32Array | Int16Array;
  peakCount?: number;
  version?: number;
  /** Source audio sample rate (needed for clip-offset math). */
  sampleRate?: number;
  /** Source audio total duration in seconds. */
  duration?: number;
};

export type WaveformStatus = "idle" | "loading" | "ready" | "error" | "missing";

// ── Automation ────────────────────────────────────────────────────────────────

export type AutomationTargetKind =
  | "track-volume"
  | "track-pan"
  | "track-mute"
  | "track-send"
  | "clip-gain"
  | "device-param"
  | "master-volume"
  | "transport-bpm";

export type AutomationCurveType = "linear" | "hold" | "smooth";

export type AutomationPoint = {
  id: string;
  /** Position in quarter-note beats from project start. */
  beat: number;
  /** Real parameter value (not normalized). */
  value: number;
  curve?: AutomationCurveType;
  selected?: boolean;
};

export type AutomationTarget = {
  id: string;
  kind: AutomationTargetKind;
  trackId?: TrackId;
  sendId?: string;
  deviceId?: string;
  paramId?: string;
  label: string;
  unit?: string;
  min: number;
  max: number;
  defaultValue: number;
  displayScale?: "linear" | "db" | "percent" | "pan";
};

export type AutomationLane = {
  id: string;
  trackId: TrackId;
  target: AutomationTarget;
  visible: boolean;
  /** Display height in pixels. */
  height: number;
  points: AutomationPoint[];
};

export type AutomationClip = {
  id: string;
  trackId: TrackId;
  target: AutomationTarget;
  startBeat: number;
  durationBeats: number;
  muted: boolean;
  points: AutomationPoint[];
  name?: string;
  color?: string;
};
