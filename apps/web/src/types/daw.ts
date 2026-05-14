export type ProjectId = string;
export type TrackId = string;
export type ClipId = string;
export type FileId = string;

export type TimeSignature = {
  numerator: number;
  denominator: number;
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
};

export type TrackType = "audio" | "midi" | "instrument" | "plugin" | "bus" | "return" | "group";

export type TrackInsert = {
  id: string;
  name: string;
  bypassed: boolean;
};

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
  inserts?: TrackInsert[];
  sends?: TrackSend[];
  /** Output routing target: "master" or a bus/group track ID. Defaults to "master". */
  output?: string;
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
