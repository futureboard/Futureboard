export type ProjectId = string;
export type TrackId = string;
export type ClipId = string;
export type FileId = string;

export type DawProject = {
  id: ProjectId;
  name: string;
  version: number;
  sampleRate: number;
  bpm: number;
  tracks: DawTrack[];
  files: DawFile[];
};

export type DawTrack = {
  id: TrackId;
  name: string;
  type: "audio";
  color: string;
  volume: number;
  pan: number;
  muted: boolean;
  solo: boolean;
  armed: boolean;
  clips: DawClip[];
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
