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

export type TrackType = "audio" | "midi" | "plugin" | "bus";

export type TrackInsert = {
  id: string;
  name: string;
  bypassed: boolean;
};

export type TrackSend = {
  id: string;
  name: string;
  /** send level 0–1 (1 = 0 dB) */
  level: number;
};

export type DawTrack = {
  id: TrackId;
  name: string;
  type: TrackType;
  color: string;
  volume: number;
  pan: number;
  muted: boolean;
  solo: boolean;
  armed: boolean;
  clips: DawClip[];
  inserts?: TrackInsert[];
  sends?: TrackSend[];
};

export type DawClip = {
  id: ClipId;
  name: string;
  fileId: FileId;
  trackId: TrackId;
  startTime: number;
  offset: number;
  duration: number;
  gain: number;
};

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
  peaks: Float32Array;
};
