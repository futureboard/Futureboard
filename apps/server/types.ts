export type ProjectRow = {
  id: string;
  name: string;
  version: number;
  sample_rate: number;
  bpm: number;
  data: string; // JSON blob of full project
  created_at: number;
  updated_at: number;
};

export type DawProject = {
  id: string;
  name: string;
  version: number;
  sampleRate: number;
  bpm: number;
  tracks: DawTrack[];
  files: DawFile[];
};

export type DawTrack = {
  id: string;
  name: string;
  type: "audio";
  volume: number;
  pan: number;
  muted: boolean;
  solo: boolean;
  clips: DawClip[];
};

export type DawClip = {
  id: string;
  name: string;
  fileId: string;
  trackId: string;
  startTime: number;
  offset: number;
  duration: number;
  gain: number;
};

export type DawFile = {
  id: string;
  name: string;
  mimeType: string;
  duration: number;
  sampleRate: number;
  channels: number;
  storageKey?: string;
  localObjectUrl?: string;
};

export type FileRow = {
  id: string;
  project_id: string;
  name: string;
  mime_type: string;
  duration: number;
  sample_rate: number;
  channels: number;
  storage_key: string;
  created_at: number;
};

export type ApiProject = {
  id: string;
  name: string;
  version: number;
  sampleRate: number;
  bpm: number;
  data: unknown;
  createdAt: number;
  updatedAt: number;
};

export type ApiFile = {
  id: string;
  projectId: string;
  name: string;
  mimeType: string;
  duration: number;
  sampleRate: number;
  channels: number;
  storageKey: string;
  createdAt: number;
};

export type RouteHandler = (
  req: Request,
  params: Record<string, string>
) => Response | Promise<Response>;
