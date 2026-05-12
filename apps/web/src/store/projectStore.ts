import { create } from "zustand";
import type { DawClip, DawFile, DawProject, DawTrack, FileId, TrackId, WaveformPeaks } from "../types/daw";

const STORAGE_KEY = "mochi-daw-project";

function defaultProject(): DawProject {
  return {
    id: crypto.randomUUID(),
    name: "Untitled Project",
    version: 1,
    sampleRate: 48000,
    bpm: 120,
    tracks: [],
    files: [],
  };
}

type PeakCache = Map<FileId, WaveformPeaks>;

type ProjectStore = {
  project: DawProject;
  peakCache: PeakCache;

  setProjectName: (name: string) => void;
  setBpm: (bpm: number) => void;
  addTrack: (track: DawTrack) => void;
  removeTrack: (trackId: TrackId) => void;
  setTrackName: (trackId: TrackId, name: string) => void;
  setTrackVolume: (trackId: TrackId, volume: number) => void;
  setTrackPan: (trackId: TrackId, pan: number) => void;
  setTrackMute: (trackId: TrackId, muted: boolean) => void;
  setTrackSolo: (trackId: TrackId, solo: boolean) => void;
  setTrackArmed: (trackId: TrackId, armed: boolean) => void;
  addClip: (trackId: TrackId, clip: DawClip) => void;
  moveClip: (clipId: string, trackId: TrackId, startTime: number) => void;
  removeClip: (clipId: string) => void;
  addFile: (file: DawFile) => void;
  setPeaks: (fileId: FileId, peaks: WaveformPeaks) => void;
  saveLocal: () => void;
  loadLocal: () => void;
};

export const useProjectStore = create<ProjectStore>((set, get) => ({
  project: defaultProject(),
  peakCache: new Map(),

  setProjectName: (name) =>
    set((s) => ({ project: { ...s.project, name } })),

  setBpm: (bpm) =>
    set((s) => ({ project: { ...s.project, bpm } })),

  addTrack: (track) =>
    set((s) => ({ project: { ...s.project, tracks: [...s.project.tracks, track] } })),

  removeTrack: (trackId) =>
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.filter((t) => t.id !== trackId),
      },
    })),

  setTrackName: (trackId, name) =>
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) => t.id === trackId ? { ...t, name } : t),
      },
    })),

  setTrackVolume: (trackId, volume) =>
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) => t.id === trackId ? { ...t, volume } : t),
      },
    })),

  setTrackPan: (trackId, pan) =>
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) => t.id === trackId ? { ...t, pan } : t),
      },
    })),

  setTrackMute: (trackId, muted) =>
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) => t.id === trackId ? { ...t, muted } : t),
      },
    })),

  setTrackSolo: (trackId, solo) =>
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) => t.id === trackId ? { ...t, solo } : t),
      },
    })),

  setTrackArmed: (trackId, armed) =>
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) => t.id === trackId ? { ...t, armed } : t),
      },
    })),

  addClip: (trackId, clip) =>
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) =>
          t.id === trackId ? { ...t, clips: [...t.clips, clip] } : t
        ),
      },
    })),

  moveClip: (clipId, _trackId, startTime) =>
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) => ({
          ...t,
          clips: t.clips.map((c) => c.id === clipId ? { ...c, startTime } : c),
        })),
      },
    })),

  removeClip: (clipId) =>
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) => ({
          ...t,
          clips: t.clips.filter((c) => c.id !== clipId),
        })),
      },
    })),

  addFile: (file) =>
    set((s) => ({ project: { ...s.project, files: [...s.project.files, file] } })),

  setPeaks: (fileId, peaks) =>
    set((s) => {
      const next = new Map(s.peakCache);
      next.set(fileId, peaks);
      return { peakCache: next };
    }),

  saveLocal: () => {
    const { project } = get();
    const serializable = {
      ...project,
      files: project.files.map(({ localObjectUrl: _, ...f }) => f),
    };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(serializable));
  },

  loadLocal: () => {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return;
    try {
      const project = JSON.parse(raw) as DawProject;
      set({ project });
    } catch {
      // corrupt — ignore
    }
  },
}));
