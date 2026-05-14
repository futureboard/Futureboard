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
};

export type ClipType = "audio" | "midi";

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
};

/** Returns the effective clip type, defaulting to "audio" for pre-existing clips. */
export function clipType(clip: DawClip): ClipType {
  return clip.type ?? (clip.fileId ? "audio" : "midi");
}

export type DawFile = {
  id: FileId;
  name: string;
  mimeType: string;
  duration: number;
  sampleRate: number;
  channels: number;
  storageKey?: string;
  localObjectUrl?: string;
};

export type WaveformPeaks = {
  samplesPerPeak: number;
  channelCount: number;
  /** Interleaved min/max per peak, per channel: [ch0_min, ch0_max, ch1_min, ch1_max, ...] */
  peaks: Float32Array;
  /** Source audio sample rate (needed for clip-offset math). */
  sampleRate?: number;
  /** Source audio total duration in seconds. */
  duration?: number;
};

export type WaveformStatus = "idle" | "loading" | "ready" | "error";
