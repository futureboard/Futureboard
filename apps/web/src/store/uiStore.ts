import { create } from "zustand";
import type { ClipId, TrackId } from "../types/daw";
import { MIXER_HEIGHT } from "../theme";

type UIStore = {
  pixelsPerSecond: number;
  scrollX: number;
  selectedClipId: ClipId | null;
  selectedTrackId: TrackId | null;
  masterVolume: number;
  inspectorOpen: boolean;
  mixerOpen: boolean;
  snapToGrid: boolean;
  loopEnabled: boolean;
  loopStart: number;
  loopEnd: number;
  // Mixer layout
  mixerHeight: number;
  mixerChannelWidth: number;
  mixerFlexLayout: boolean;
  setPixelsPerSecond: (v: number) => void;
  setScrollX: (v: number) => void;
  setSelectedClipId: (id: ClipId | null) => void;
  setSelectedTrackId: (id: TrackId | null) => void;
  setMasterVolume: (v: number) => void;
  toggleInspector: () => void;
  toggleMixer: () => void;
  toggleSnapToGrid: () => void;
  toggleLoop: () => void;
  setLoopStart: (seconds: number) => void;
  setLoopEnd: (seconds: number) => void;
  setMixerHeight: (h: number) => void;
  setMixerChannelWidth: (w: number) => void;
  toggleMixerFlexLayout: () => void;
  // cross-track clip drag
  draggingClipTargetIdx: number | null;
  setDraggingClipTargetIdx: (idx: number | null) => void;
};

export const useUIStore = create<UIStore>((set) => ({
  pixelsPerSecond: 100,
  scrollX: 0,
  selectedClipId: null,
  selectedTrackId: null,
  masterVolume: 1,
  inspectorOpen: true,
  mixerOpen: true,
  snapToGrid: true,
  loopEnabled: false,
  loopStart: 0,
  loopEnd: 4,
  mixerHeight: MIXER_HEIGHT,
  mixerChannelWidth: 80,
  mixerFlexLayout: false,
  setPixelsPerSecond: (pixelsPerSecond) => set({ pixelsPerSecond }),
  setScrollX: (scrollX) => set({ scrollX }),
  setSelectedClipId: (selectedClipId) => set({ selectedClipId }),
  setSelectedTrackId: (selectedTrackId) => set({ selectedTrackId }),
  setMasterVolume: (masterVolume) => set({ masterVolume }),
  toggleInspector: () => set((s) => ({ inspectorOpen: !s.inspectorOpen })),
  toggleMixer: () => set((s) => ({ mixerOpen: !s.mixerOpen })),
  toggleSnapToGrid: () => set((s) => ({ snapToGrid: !s.snapToGrid })),
  toggleLoop: () => set((s) => ({ loopEnabled: !s.loopEnabled })),
  setLoopStart: (loopStart) => set({ loopStart }),
  setLoopEnd: (loopEnd) => set({ loopEnd }),
  setMixerHeight: (mixerHeight) => set({ mixerHeight: Math.max(160, Math.min(520, mixerHeight)) }),
  setMixerChannelWidth: (mixerChannelWidth) => set({ mixerChannelWidth: Math.max(60, Math.min(180, mixerChannelWidth)) }),
  toggleMixerFlexLayout: () => set((s) => ({ mixerFlexLayout: !s.mixerFlexLayout })),
  draggingClipTargetIdx: null,
  setDraggingClipTargetIdx: (draggingClipTargetIdx) => set({ draggingClipTargetIdx }),
}));
