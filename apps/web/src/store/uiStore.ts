import { create } from "zustand";
import type { ClipId, TrackId } from "../types/daw";

type UIStore = {
  pixelsPerSecond: number;
  scrollX: number;
  selectedClipId: ClipId | null;
  selectedTrackId: TrackId | null;
  masterVolume: number;
  inspectorOpen: boolean;
  mixerOpen: boolean;
  setPixelsPerSecond: (v: number) => void;
  setScrollX: (v: number) => void;
  setSelectedClipId: (id: ClipId | null) => void;
  setSelectedTrackId: (id: TrackId | null) => void;
  setMasterVolume: (v: number) => void;
  toggleInspector: () => void;
  toggleMixer: () => void;
};

export const useUIStore = create<UIStore>((set) => ({
  pixelsPerSecond: 100,
  scrollX: 0,
  selectedClipId: null,
  selectedTrackId: null,
  masterVolume: 1,
  inspectorOpen: true,
  mixerOpen: true,
  setPixelsPerSecond: (pixelsPerSecond) => set({ pixelsPerSecond }),
  setScrollX: (scrollX) => set({ scrollX }),
  setSelectedClipId: (selectedClipId) => set({ selectedClipId }),
  setSelectedTrackId: (selectedTrackId) => set({ selectedTrackId }),
  setMasterVolume: (masterVolume) => set({ masterVolume }),
  toggleInspector: () => set((s) => ({ inspectorOpen: !s.inspectorOpen })),
  toggleMixer: () => set((s) => ({ mixerOpen: !s.mixerOpen })),
}));
