import { create } from "zustand";
import { useUIStore } from "./uiStore";
import type {
  AutomationLane,
  AutomationPoint,
  AutomationTarget,
  DawClip,
  DawFile,
  DawProject,
  DawProjectAsset,
  DawTrack,
  FileId,
  InsertDevice,
  MidiNote,
  TimeSignature,
  TrackAdvanced,
  TrackId,
  TrackMonitorMode,
  TrackPreviewMode,
  TrackRouting,
  TrackSend,
  WaveformStatus,
} from "../types/daw";
import { normalizeProject, normalizeTrack } from "../utils/normalize";

const STORAGE_KEY = "mochi-daw-project";

/** Lightweight peak-level metadata stored in Zustand — NO peak data. */
export type PeakLevelMeta = {
  spp: number;
  peakCount: number;
  channelCount: number;
  sampleRate: number;
  duration: number;
};

type PeakMetaMap = Map<FileId, Map<number, PeakLevelMeta>>;

function defaultProject(): DawProject {
  return normalizeProject({
    id: crypto.randomUUID(),
    name: "Untitled Project",
    version: 1,
    sampleRate: 48000,
    bpm: 120,
    timeSignature: { numerator: 4, denominator: 4 },
    tracks: [],
    files: [],
  });
}

/** Mark project dirty in the UI store. */
function markDirty() {
  if (useUIStore.getState().saveStatus !== "unsaved") {
    useUIStore.getState().setSaveStatus("unsaved");
  }
}

type WaveformStatusMap = Map<FileId, WaveformStatus>;
type WaveformProgressMap = Map<FileId, number>;

type ProjectStore = {
  project: DawProject;
  /** Peak level metadata per file — keyed by spp. Contains NO peak data. */
  peakMeta: PeakMetaMap;
  waveformStatus: WaveformStatusMap;
  waveformProgress: WaveformProgressMap;

  // ── Project-level ──────────────────────────────────────────────────────────
  createNewProject: (overrides?: Partial<DawProject>) => void;
  loadProject: (project: DawProject) => void;
  resetProject: () => void;
  setProjectName: (name: string) => void;
  setBpm: (bpm: number) => void;
  setTimeSignature: (timeSig: TimeSignature) => void;
  updateProjectSettings: (patch: Partial<Pick<DawProject, "bpm" | "timeSignature" | "sampleRate" | "name">>) => void;

  // ── Tracks ─────────────────────────────────────────────────────────────────
  addTrack: (track: DawTrack) => void;
  batchImportTracks: (tracks: DawTrack[], clips: Array<{ trackId: string; clip: DawClip }>) => void;
  removeTrack: (trackId: TrackId) => void;
  setTrackName: (trackId: TrackId, name: string) => void;
  setTrackVolume: (trackId: TrackId, volume: number) => void;
  setTrackPan: (trackId: TrackId, pan: number) => void;
  setTrackMute: (trackId: TrackId, muted: boolean) => void;
  setTrackSolo: (trackId: TrackId, solo: boolean) => void;
  setTrackArmed: (trackId: TrackId, armed: boolean) => void;
  setTrackMonitorMode: (trackId: TrackId, mode: TrackMonitorMode) => void;
  setTrackPreviewMode: (trackId: TrackId, mode: TrackPreviewMode) => void;
  setTrackColor: (trackId: TrackId, color: string) => void;
  setTrackOutput: (trackId: TrackId, output: string) => void;
  setTrackHeight: (trackId: TrackId, height: number | undefined) => void;
  collapseTrack: (trackId: TrackId, collapsed: boolean) => void;
  reorderTracks: (activeTrackId: TrackId, overTrackId: TrackId) => void;
  updateTrackRouting: (trackId: TrackId, patch: Partial<TrackRouting>) => void;
  updateTrackAdvanced: (trackId: TrackId, patch: Partial<TrackAdvanced>) => void;

  // ── Track sends ────────────────────────────────────────────────────────────
  addTrackSend: (trackId: TrackId, send: TrackSend) => void;
  removeTrackSend: (trackId: TrackId, sendId: string) => void;
  updateTrackSend: (trackId: TrackId, sendId: string, updates: Partial<TrackSend>) => void;

  // ── Insert devices ─────────────────────────────────────────────────────────
  addInsertDevice: (trackId: TrackId, device: InsertDevice) => void;
  removeInsertDevice: (trackId: TrackId, deviceId: string) => void;
  toggleInsertDevice: (trackId: TrackId, deviceId: string) => void;
  updateInsertDeviceParams: (trackId: TrackId, deviceId: string, params: Record<string, number | string | boolean>) => void;
  reorderInsertDevices: (trackId: TrackId, fromIndex: number, toIndex: number) => void;

  // ── Clips ──────────────────────────────────────────────────────────────────
  addClip: (trackId: TrackId, clip: DawClip) => void;
  moveClip: (clipId: string, trackId: TrackId, startTime: number) => void;
  resizeClip: (clipId: string, trackId: TrackId, startTime: number, offset: number, duration: number) => void;
  updateClip: (clipId: string, updates: Partial<DawClip>) => void;
  removeClip: (clipId: string) => void;
  deleteClips: (clipIds: string[]) => void;
  duplicateClips: (clipIds: string[]) => void;
  splitClip: (clipId: string, time: number) => void;
  moveClipToTrack: (clipId: string, toTrackId: TrackId, startTime: number) => void;

  // ── MIDI notes ─────────────────────────────────────────────────────────────
  addMidiNotes: (clipId: string, notes: MidiNote[]) => void;
  updateMidiNotes: (clipId: string, updates: Array<Partial<MidiNote> & { id: string }>) => void;
  removeMidiNotes: (clipId: string, noteIds: string[]) => void;

  // ── Files / assets ─────────────────────────────────────────────────────────
  addFile: (file: DawFile) => void;
  updateFile: (fileId: FileId, updates: Partial<DawFile>) => void;
  removeFile: (fileId: FileId) => void;

  // ── Project asset manifest (Electron folder-project assets) ────────────────
  addAsset: (asset: DawProjectAsset) => void;
  updateAsset: (assetId: string, updates: Partial<DawProjectAsset>) => void;
  removeAsset: (assetId: string) => void;

  // ── Waveform metadata (non-dirty — actual peak data lives in peakChunkCache) ─
  /** Register metadata for one peak resolution level. Peak data is NOT stored here. */
  setPeakMeta: (fileId: FileId, meta: PeakLevelMeta) => void;
  setWaveformStatus: (fileId: FileId, status: WaveformStatus) => void;
  setWaveformProgress: (fileId: FileId, progress: number) => void;

  // ── Automation lanes ───────────────────────────────────────────────────────
  addAutomationLane: (trackId: TrackId, target: AutomationTarget) => AutomationLane;
  removeAutomationLane: (trackId: TrackId, laneId: string) => void;
  toggleAutomationLaneVisible: (trackId: TrackId, laneId: string) => void;
  clearAutomationLane: (trackId: TrackId, laneId: string) => void;

  // ── Automation points ──────────────────────────────────────────────────────
  addAutomationPoint: (trackId: TrackId, laneId: string, point: AutomationPoint) => void;
  updateAutomationPoint: (trackId: TrackId, laneId: string, pointId: string, patch: Partial<AutomationPoint>) => void;
  removeAutomationPoint: (trackId: TrackId, laneId: string, pointId: string) => void;
  removeAutomationPoints: (trackId: TrackId, laneId: string, pointIds: string[]) => void;

  // ── Persistence ────────────────────────────────────────────────────────────
  saveLocal: () => void;
  loadLocal: () => void;
};

export const useProjectStore = create<ProjectStore>((set, get) => ({
  project: defaultProject(),
  peakMeta: new Map(),
  waveformStatus: new Map(),
  waveformProgress: new Map(),

  // ── Project-level ──────────────────────────────────────────────────────────

  createNewProject: (overrides) => {
    set({ project: defaultProject(), peakMeta: new Map(), waveformStatus: new Map(), waveformProgress: new Map() });
    if (overrides) set((s) => ({ project: { ...s.project, ...overrides } }));
  },

  loadProject: (project) => {
    set({ project: normalizeProject(project as Partial<DawProject>), peakMeta: new Map(), waveformStatus: new Map(), waveformProgress: new Map() });
  },

  resetProject: () => {
    set({ project: defaultProject(), peakMeta: new Map(), waveformStatus: new Map(), waveformProgress: new Map() });
  },

  setProjectName: (name) => {
    set((s) => ({ project: { ...s.project, name } }));
    markDirty();
  },

  setBpm: (bpm) => {
    set((s) => ({ project: { ...s.project, bpm: Math.max(20, Math.min(300, bpm)) } }));
    markDirty();
  },

  setTimeSignature: (timeSignature) => {
    set((s) => ({ project: { ...s.project, timeSignature } }));
    markDirty();
  },

  updateProjectSettings: (patch) => {
    set((s) => ({ project: { ...s.project, ...patch } }));
    markDirty();
  },

  // ── Tracks ─────────────────────────────────────────────────────────────────

  addTrack: (track) => {
    set((s) => ({ project: { ...s.project, tracks: [...s.project.tracks, normalizeTrack(track)] } }));
    markDirty();
  },

  batchImportTracks: (tracks, clips) => {
    if (tracks.length === 0 && clips.length === 0) return;
    set((s) => {
      const allTracks = [...s.project.tracks, ...tracks.map(normalizeTrack)];
      const clipsByTrack = new Map<string, DawClip[]>();
      for (const { trackId, clip } of clips) {
        const arr = clipsByTrack.get(trackId);
        if (arr) arr.push(clip);
        else clipsByTrack.set(trackId, [clip]);
      }
      return {
        project: {
          ...s.project,
          tracks: allTracks.map((t) => {
            const extra = clipsByTrack.get(t.id);
            return extra ? { ...t, clips: [...t.clips, ...extra] } : t;
          }),
        },
      };
    });
    markDirty();
  },

  removeTrack: (trackId) => {
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks
          .filter((t) => t.id !== trackId)
          .map((t) => ({
            ...t,
            output: t.output === trackId ? "master" : t.output,
            sends: t.sends?.filter((send) => send.targetTrackId !== trackId),
          })),
      },
    }));
    markDirty();
  },

  setTrackName: (trackId, name) => {
    set((s) => ({
      project: { ...s.project, tracks: s.project.tracks.map((t) => t.id === trackId ? { ...t, name } : t) },
    }));
    markDirty();
  },

  setTrackVolume: (trackId, volume) => {
    set((s) => ({
      project: { ...s.project, tracks: s.project.tracks.map((t) => t.id === trackId ? { ...t, volume } : t) },
    }));
    markDirty();
  },

  setTrackPan: (trackId, pan) => {
    set((s) => ({
      project: { ...s.project, tracks: s.project.tracks.map((t) => t.id === trackId ? { ...t, pan } : t) },
    }));
    markDirty();
  },

  setTrackMute: (trackId, muted) => {
    set((s) => ({
      project: { ...s.project, tracks: s.project.tracks.map((t) => t.id === trackId ? { ...t, muted } : t) },
    }));
    markDirty();
  },

  setTrackSolo: (trackId, solo) => {
    set((s) => ({
      project: { ...s.project, tracks: s.project.tracks.map((t) => t.id === trackId ? { ...t, solo } : t) },
    }));
    markDirty();
  },

  setTrackArmed: (trackId, armed) => {
    set((s) => ({
      project: { ...s.project, tracks: s.project.tracks.map((t) => t.id === trackId ? { ...t, armed } : t) },
    }));
    markDirty();
  },

  setTrackMonitorMode: (trackId, mode) => {
    set((s) => ({
      project: { ...s.project, tracks: s.project.tracks.map((t) => t.id === trackId ? { ...t, monitorMode: mode } : t) },
    }));
    markDirty();
  },

  setTrackPreviewMode: (trackId, mode) => {
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) =>
          t.id === trackId
            ? { ...t, monitor: { ...(t.monitor ?? { previewMode: "stereo" }), previewMode: mode } }
            : t
        ),
      },
    }));
    markDirty();
  },

  setTrackColor: (trackId, color) => {
    set((s) => ({
      project: { ...s.project, tracks: s.project.tracks.map((t) => t.id === trackId ? { ...t, color } : t) },
    }));
    markDirty();
  },

  setTrackOutput: (trackId, output) => {
    set((s) => ({
      project: { ...s.project, tracks: s.project.tracks.map((t) => t.id === trackId ? { ...t, output } : t) },
    }));
    markDirty();
  },

  setTrackHeight: (trackId, height) => {
    set((s) => ({
      project: { ...s.project, tracks: s.project.tracks.map((t) => t.id === trackId ? { ...t, height } : t) },
    }));
  },

  collapseTrack: (trackId, collapsed) => {
    set((s) => ({
      project: { ...s.project, tracks: s.project.tracks.map((t) => t.id === trackId ? { ...t, collapsed } : t) },
    }));
  },

  reorderTracks: (activeTrackId, overTrackId) => {
    set((s) => {
      const tracks = s.project.tracks;
      const oldIndex = tracks.findIndex((t) => t.id === activeTrackId);
      const newIndex = tracks.findIndex((t) => t.id === overTrackId);
      if (oldIndex < 0 || newIndex < 0 || oldIndex === newIndex) return s;
      const next = tracks.slice();
      const [moved] = next.splice(oldIndex, 1);
      next.splice(newIndex, 0, moved);
      return { project: { ...s.project, tracks: next } };
    });
    markDirty();
  },

  updateTrackRouting: (trackId, patch) => {
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) =>
          t.id === trackId ? { ...t, routing: { ...(t.routing ?? {}), ...patch } as TrackRouting } : t
        ),
      },
    }));
    markDirty();
  },

  updateTrackAdvanced: (trackId, patch) => {
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) =>
          t.id === trackId ? { ...t, advanced: { ...(t.advanced ?? {}), ...patch } as TrackAdvanced } : t
        ),
      },
    }));
    markDirty();
  },

  // ── Track sends ────────────────────────────────────────────────────────────

  addTrackSend: (trackId, send) => {
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) =>
          t.id === trackId ? { ...t, sends: [...(t.sends ?? []), send] } : t
        ),
      },
    }));
    markDirty();
  },

  removeTrackSend: (trackId, sendId) => {
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) =>
          t.id === trackId ? { ...t, sends: (t.sends ?? []).filter((s) => s.id !== sendId) } : t
        ),
      },
    }));
    markDirty();
  },

  updateTrackSend: (trackId, sendId, updates) => {
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) =>
          t.id === trackId
            ? { ...t, sends: (t.sends ?? []).map((send) => send.id === sendId ? { ...send, ...updates } : send) }
            : t
        ),
      },
    }));
    markDirty();
  },

  // ── Insert devices ─────────────────────────────────────────────────────────

  addInsertDevice: (trackId, device) => {
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) => {
          if (t.id !== trackId) return t;
          const inserts = t.inserts ?? [];
          const order = inserts.length;
          return { ...t, inserts: [...inserts, { ...device, order }] };
        }),
      },
    }));
    markDirty();
  },

  removeInsertDevice: (trackId, deviceId) => {
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) => {
          if (t.id !== trackId) return t;
          const filtered = (t.inserts ?? []).filter((ins) => ins.id !== deviceId);
          return { ...t, inserts: filtered.map((ins, i) => ({ ...ins, order: i })) };
        }),
      },
    }));
    markDirty();
  },

  toggleInsertDevice: (trackId, deviceId) => {
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) =>
          t.id === trackId
            ? {
                ...t,
                inserts: (t.inserts ?? []).map((ins) =>
                  ins.id === deviceId ? { ...ins, enabled: !ins.enabled } : ins
                ),
              }
            : t
        ),
      },
    }));
    markDirty();
  },

  updateInsertDeviceParams: (trackId, deviceId, params) => {
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) =>
          t.id === trackId
            ? {
                ...t,
                inserts: (t.inserts ?? []).map((ins) =>
                  ins.id === deviceId ? { ...ins, params: { ...ins.params, ...params } } : ins
                ),
              }
            : t
        ),
      },
    }));
    markDirty();
  },

  reorderInsertDevices: (trackId, fromIndex, toIndex) => {
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) => {
          if (t.id !== trackId) return t;
          const inserts = (t.inserts ?? []).slice();
          const [moved] = inserts.splice(fromIndex, 1);
          inserts.splice(toIndex, 0, moved);
          return { ...t, inserts: inserts.map((ins, i) => ({ ...ins, order: i })) };
        }),
      },
    }));
    markDirty();
  },

  // ── Clips ──────────────────────────────────────────────────────────────────

  addClip: (trackId, clip) => {
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) =>
          t.id === trackId ? { ...t, clips: [...t.clips, clip] } : t
        ),
      },
    }));
    markDirty();
  },

  moveClip: (clipId, _trackId, startTime) => {
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) => ({
          ...t,
          clips: t.clips.map((c) => c.id === clipId ? { ...c, startTime } : c),
        })),
      },
    }));
    markDirty();
  },

  resizeClip: (clipId, _trackId, startTime, offset, duration) => {
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) => ({
          ...t,
          clips: t.clips.map((c) => c.id === clipId ? { ...c, startTime, offset, duration } : c),
        })),
      },
    }));
    markDirty();
  },

  updateClip: (clipId, updates) => {
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) => ({
          ...t,
          clips: t.clips.map((c) => c.id === clipId ? { ...c, ...updates } : c),
        })),
      },
    }));
    markDirty();
  },

  removeClip: (clipId) => {
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) => ({
          ...t,
          clips: t.clips.filter((c) => c.id !== clipId),
        })),
      },
    }));
    markDirty();
  },

  deleteClips: (clipIds) => {
    const ids = new Set(clipIds);
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) => ({
          ...t,
          clips: t.clips.filter((c) => !ids.has(c.id)),
        })),
      },
    }));
    markDirty();
  },

  duplicateClips: (clipIds) => {
    const ids = new Set(clipIds);
    set((s) => ({
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
    }));
    markDirty();
  },

  splitClip: (clipId, time) => {
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) => {
          const idx = t.clips.findIndex((c) => c.id === clipId);
          if (idx === -1) return t;
          const c = t.clips[idx];
          if (time <= c.startTime || time >= c.startTime + c.duration) return t;
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
    }));
    markDirty();
  },

  moveClipToTrack: (clipId, toTrackId, startTime) => {
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
    });
    markDirty();
  },

  // ── MIDI notes ─────────────────────────────────────────────────────────────

  addMidiNotes: (clipId, notes) => {
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
    }));
    markDirty();
  },

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
    markDirty();
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
    markDirty();
  },

  // ── Files / assets ─────────────────────────────────────────────────────────

  addFile: (file) => {
    set((s) => ({ project: { ...s.project, files: [...s.project.files, file] } }));
    markDirty();
  },

  updateFile: (fileId, updates) => {
    set((s) => ({
      project: {
        ...s.project,
        files: s.project.files.map((file) =>
          file.id === fileId ? { ...file, ...updates } : file
        ),
      },
    }));
    markDirty();
  },

  removeFile: (fileId) => {
    set((s) => ({ project: { ...s.project, files: s.project.files.filter((f) => f.id !== fileId) } }));
    markDirty();
  },

  // ── Project asset manifest ─────────────────────────────────────────────────

  addAsset: (asset) => {
    set((s) => {
      // Deduplicate: if same id already present, skip (idempotent)
      const existing = (s.project.assets ?? []).some((a) => a.id === asset.id);
      if (existing) return s;
      return { project: { ...s.project, assets: [...(s.project.assets ?? []), asset] } };
    });
    markDirty();
  },

  updateAsset: (assetId, updates) => {
    set((s) => ({
      project: {
        ...s.project,
        assets: (s.project.assets ?? []).map((a) =>
          a.id === assetId ? { ...a, ...updates } : a
        ),
      },
    }));
    markDirty();
  },

  removeAsset: (assetId) => {
    set((s) => ({
      project: {
        ...s.project,
        assets: (s.project.assets ?? []).filter((a) => a.id !== assetId),
      },
    }));
    markDirty();
  },

  // ── Automation lanes ───────────────────────────────────────────────────────

  addAutomationLane: (trackId, target) => {
    const lane: AutomationLane = {
      id: crypto.randomUUID(),
      trackId,
      target,
      visible: true,
      height: 72,
      points: [],
    };
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) =>
          t.id === trackId
            ? { ...t, automationLanes: [...(t.automationLanes ?? []), lane] }
            : t
        ),
      },
    }));
    markDirty();
    return lane;
  },

  removeAutomationLane: (trackId, laneId) => {
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) =>
          t.id === trackId
            ? { ...t, automationLanes: (t.automationLanes ?? []).filter((l) => l.id !== laneId) }
            : t
        ),
      },
    }));
    markDirty();
  },

  toggleAutomationLaneVisible: (trackId, laneId) => {
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) =>
          t.id === trackId
            ? {
                ...t,
                automationLanes: (t.automationLanes ?? []).map((l) =>
                  l.id === laneId ? { ...l, visible: !l.visible } : l
                ),
              }
            : t
        ),
      },
    }));
  },

  clearAutomationLane: (trackId, laneId) => {
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) =>
          t.id === trackId
            ? {
                ...t,
                automationLanes: (t.automationLanes ?? []).map((l) =>
                  l.id === laneId ? { ...l, points: [] } : l
                ),
              }
            : t
        ),
      },
    }));
    markDirty();
  },

  // ── Automation points ──────────────────────────────────────────────────────

  addAutomationPoint: (trackId, laneId, point) => {
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) =>
          t.id === trackId
            ? {
                ...t,
                automationLanes: (t.automationLanes ?? []).map((l) =>
                  l.id === laneId ? { ...l, points: [...l.points, point] } : l
                ),
              }
            : t
        ),
      },
    }));
    markDirty();
  },

  updateAutomationPoint: (trackId, laneId, pointId, patch) => {
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) =>
          t.id === trackId
            ? {
                ...t,
                automationLanes: (t.automationLanes ?? []).map((l) =>
                  l.id === laneId
                    ? {
                        ...l,
                        points: l.points.map((p) =>
                          p.id === pointId ? { ...p, ...patch } : p
                        ),
                      }
                    : l
                ),
              }
            : t
        ),
      },
    }));
    markDirty();
  },

  removeAutomationPoint: (trackId, laneId, pointId) => {
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) =>
          t.id === trackId
            ? {
                ...t,
                automationLanes: (t.automationLanes ?? []).map((l) =>
                  l.id === laneId
                    ? { ...l, points: l.points.filter((p) => p.id !== pointId) }
                    : l
                ),
              }
            : t
        ),
      },
    }));
    markDirty();
  },

  removeAutomationPoints: (trackId, laneId, pointIds) => {
    const ids = new Set(pointIds);
    set((s) => ({
      project: {
        ...s.project,
        tracks: s.project.tracks.map((t) =>
          t.id === trackId
            ? {
                ...t,
                automationLanes: (t.automationLanes ?? []).map((l) =>
                  l.id === laneId
                    ? { ...l, points: l.points.filter((p) => !ids.has(p.id)) }
                    : l
                ),
              }
            : t
        ),
      },
    }));
    markDirty();
  },

  // ── Waveform metadata ──────────────────────────────────────────────────────

  setPeakMeta: (fileId, meta) =>
    set((s) => {
      const nextMeta = new Map(s.peakMeta);
      const fileLevels = new Map(nextMeta.get(fileId) ?? []);
      fileLevels.set(meta.spp, meta);
      nextMeta.set(fileId, fileLevels);
      const status = new Map(s.waveformStatus);
      status.set(fileId, "ready");
      const progress = new Map(s.waveformProgress);
      progress.set(fileId, 1);
      return { peakMeta: nextMeta, waveformStatus: status, waveformProgress: progress };
    }),

  setWaveformStatus: (fileId, status) =>
    set((s) => {
      const next = new Map(s.waveformStatus);
      next.set(fileId, status);
      return { waveformStatus: next };
    }),

  setWaveformProgress: (fileId, progress) =>
    set((s) => {
      const next = new Map(s.waveformProgress);
      next.set(fileId, Math.max(0, Math.min(1, progress)));
      return { waveformProgress: next };
    }),

  // ── Persistence ────────────────────────────────────────────────────────────

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
        size: file.size,
        lastModified: file.lastModified,
        hash: file.hash,
        originalFileName: file.originalFileName,
        storageProvider: file.storageProvider,
        cacheKey: file.cacheKey,
        waveformCacheKeys: file.waveformCacheKeys,
        storageKey: file.storageKey,
        relativePath: file.relativePath,
      })),
      // Assets are persisted to localStorage so the Browser panel survives
      // page refresh (folder project path restored on next native open).
      assets: (project.assets ?? []).map((a) => ({
        id: a.id,
        type: a.type,
        name: a.name,
        originalName: a.originalName,
        relativePath: a.relativePath,
        size: a.size,
        hash: a.hash,
        durationSeconds: a.durationSeconds,
        sampleRate: a.sampleRate,
        channels: a.channels,
        mimeType: a.mimeType,
        createdAt: a.createdAt,
        updatedAt: a.updatedAt,
        // missing is runtime-only; will be re-evaluated on next open
      })),
    };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(serializable));
  },

  loadLocal: () => {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return;
    try {
      const project = JSON.parse(raw) as DawProject;
      set({ project: normalizeProject(project as Partial<DawProject>), peakMeta: new Map(), waveformStatus: new Map(), waveformProgress: new Map() });
    } catch {
      // corrupt — ignore
    }
  },
}));
