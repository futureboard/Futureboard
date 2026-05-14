import { create } from "zustand";
import type { DawClip, DawFile, DawProject, DawTrack, FileId, MidiNote, TimeSignature, TrackId, WaveformPeaks, WaveformStatus } from "../types/daw";

const STORAGE_KEY = "mochi-daw-project";

function defaultProject(): DawProject {
  return {
    id: crypto.randomUUID(),
    name: "Untitled Project",
    version: 1,
    sampleRate: 48000,
    bpm: 120,
    timeSignature: { numerator: 4, denominator: 4 },
    tracks: [],
    files: [],
  };
}

type PeakCache = Map<FileId, WaveformPeaks>;
type WaveformStatusMap = Map<FileId, WaveformStatus>;

type ProjectStore = {
  project: DawProject;
  peakCache: PeakCache;
  waveformStatus: WaveformStatusMap;

  setProjectName: (name: string) => void;
  setBpm: (bpm: number) => void;
  setTimeSignature: (timeSig: TimeSignature) => void;
  addTrack: (track: DawTrack) => void;
  removeTrack: (trackId: TrackId) => void;
  setTrackName: (trackId: TrackId, name: string) => void;
  setTrackVolume: (trackId: TrackId, volume: number) => void;
  setTrackPan: (trackId: TrackId, pan: number) => void;
  setTrackMute: (trackId: TrackId, muted: boolean) => void;
  setTrackSolo: (trackId: TrackId, solo: boolean) => void;
  setTrackArmed: (trackId: TrackId, armed: boolean) => void;
  setTrackColor: (trackId: TrackId, color: string) => void;
  reorderTracks: (activeTrackId: TrackId, overTrackId: TrackId) => void;
  addClip: (trackId: TrackId, clip: DawClip) => void;
  moveClip: (clipId: string, trackId: TrackId, startTime: number) => void;
  resizeClip: (clipId: string, trackId: TrackId, startTime: number, offset: number, duration: number) => void;
  updateClip: (clipId: string, updates: Partial<DawClip>) => void;
  removeClip: (clipId: string) => void;
  deleteClips: (clipIds: string[]) => void;
  duplicateClips: (clipIds: string[]) => void;
  splitClip: (clipId: string, time: number) => void;
  addMidiNotes: (clipId: string, notes: MidiNote[]) => void;
  updateMidiNotes: (clipId: string, updates: Array<Partial<MidiNote> & { id: string }>) => void;
  removeMidiNotes: (clipId: string, noteIds: string[]) => void;
  addFile: (file: DawFile) => void;
  moveClipToTrack: (clipId: string, toTrackId: TrackId, startTime: number) => void;
  setPeaks: (fileId: FileId, peaks: WaveformPeaks) => void;
  setWaveformStatus: (fileId: FileId, status: WaveformStatus) => void;
  saveLocal: () => void;
  loadLocal: () => void;
};

export const useProjectStore = create<ProjectStore>((set, get) => ({
  project: defaultProject(),
  peakCache: new Map(),
  waveformStatus: new Map(),

  setProjectName: (name) =>
    set((s) => ({ project: { ...s.project, name } })),

  setBpm: (bpm) =>
    set((s) => ({ project: { ...s.project, bpm } })),

  setTimeSignature: (timeSignature) =>
    set((s) => ({ project: { ...s.project, timeSignature } })),

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

  setTrackColor: (trackId, color) =>
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) => t.id === trackId ? { ...t, color } : t),
      },
    })),

  reorderTracks: (activeTrackId, overTrackId) =>
    set((s) => {
      const tracks = s.project.tracks;
      const oldIndex = tracks.findIndex((t) => t.id === activeTrackId);
      const newIndex = tracks.findIndex((t) => t.id === overTrackId);
      if (oldIndex < 0 || newIndex < 0 || oldIndex === newIndex) return s;
      const next = tracks.slice();
      const [moved] = next.splice(oldIndex, 1);
      next.splice(newIndex, 0, moved);
      return { project: { ...s.project, tracks: next } };
    }),

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

  resizeClip: (clipId, _trackId, startTime, offset, duration) =>
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) => ({
          ...t,
          clips: t.clips.map((c) => c.id === clipId ? { ...c, startTime, offset, duration } : c),
        })),
      },
    })),

  updateClip: (clipId, updates) =>
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) => ({
          ...t,
          clips: t.clips.map((c) => c.id === clipId ? { ...c, ...updates } : c),
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

  deleteClips: (clipIds) =>
    set((s) => {
      const ids = new Set(clipIds);
      return {
        project: {
          ...s.project,
          tracks: s.project.tracks.map((t) => ({
            ...t,
            clips: t.clips.filter((c) => !ids.has(c.id)),
          })),
        },
      };
    }),

  duplicateClips: (clipIds) =>
    set((s) => {
      const ids = new Set(clipIds);
      return {
        project: {
          ...s.project,
          tracks: s.project.tracks.map((t) => {
            const newClips: DawClip[] = [];
            for (const c of t.clips) {
              if (ids.has(c.id)) {
                newClips.push({ ...c, id: crypto.randomUUID(), startTime: c.startTime + c.duration });
              }
            }
            return { ...t, clips: [...t.clips, ...newClips] };
          }),
        },
      };
    }),

  splitClip: (clipId, time) =>
    set((s) => {
      return {
        project: {
          ...s.project,
          tracks: s.project.tracks.map((t) => {
            const idx = t.clips.findIndex((c) => c.id === clipId);
            if (idx === -1) return t;
            const c = t.clips[idx];
            if (time <= c.startTime || time >= c.startTime + c.duration) return t; // Cannot split outside bounds
            
            const firstDuration = time - c.startTime;
            const c1: DawClip = { ...c, duration: firstDuration };
            const c2: DawClip = {
              ...c,
              id: crypto.randomUUID(),
              startTime: time,
              offset: c.offset + firstDuration,
              duration: c.duration - firstDuration,
            };
            
            const newClips = [...t.clips];
            newClips.splice(idx, 1, c1, c2);
            return { ...t, clips: newClips };
          }),
        },
      };
    }),

  addMidiNotes: (clipId, notes) =>
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) => ({
          ...t,
          clips: t.clips.map((c) =>
            c.id === clipId ? { ...c, notes: [...(c.notes ?? []), ...notes] } : c
          ),
        })),
      },
    })),

  updateMidiNotes: (clipId, updates) => {
    const map = new Map(updates.map((u) => [u.id, u]));
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) => ({
          ...t,
          clips: t.clips.map((c) =>
            c.id === clipId
              ? { ...c, notes: (c.notes ?? []).map((n) => map.has(n.id) ? { ...n, ...map.get(n.id) } : n) }
              : c
          ),
        })),
      },
    }));
  },

  removeMidiNotes: (clipId, noteIds) => {
    const ids = new Set(noteIds);
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) => ({
          ...t,
          clips: t.clips.map((c) =>
            c.id === clipId ? { ...c, notes: (c.notes ?? []).filter((n) => !ids.has(n.id)) } : c
          ),
        })),
      },
    }));
  },

  addFile: (file) =>
    set((s) => ({ project: { ...s.project, files: [...s.project.files, file] } })),

  moveClipToTrack: (clipId, toTrackId, startTime) =>
    set((s) => {
      let clip: DawClip | undefined;
      const tracks = s.project.tracks.map((t) => {
        const found = t.clips.find((c) => c.id === clipId);
        if (found) { clip = found; return { ...t, clips: t.clips.filter((c) => c.id !== clipId) }; }
        return t;
      });
      if (!clip) return s;
      const moved: DawClip = { ...clip, trackId: toTrackId, startTime };
      return {
        project: {
          ...s.project,
          tracks: tracks.map((t) =>
            t.id === toTrackId ? { ...t, clips: [...t.clips, moved] } : t
          ),
        },
      };
    }),

  setPeaks: (fileId, peaks) =>
    set((s) => {
      const next = new Map(s.peakCache);
      next.set(fileId, peaks);
      const status = new Map(s.waveformStatus);
      status.set(fileId, "ready");
      return { peakCache: next, waveformStatus: status };
    }),

  setWaveformStatus: (fileId, status) =>
    set((s) => {
      const next = new Map(s.waveformStatus);
      next.set(fileId, status);
      return { waveformStatus: next };
    }),

  saveLocal: () => {
    const { project } = get();
    const serializable = {
      ...project,
      files: project.files.map((file) => ({
        id: file.id,
        name: file.name,
        mimeType: file.mimeType,
        duration: file.duration,
        sampleRate: file.sampleRate,
        channels: file.channels,
        storageKey: file.storageKey,
      })),
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
