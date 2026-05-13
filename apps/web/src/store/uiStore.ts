import { create } from "zustand";
import type { ClipId, TrackId } from "../types/daw";
import type { AppMenuItem } from "../menu/menuItems";
import { MIXER_HEIGHT } from "../theme";

type UIStore = {
  pixelsPerSecond: number;
  scrollX: number;
  selectedClipIds: ClipId[];
  selectedTrackId: TrackId | null;
  selectedMixerTrackId: TrackId | "master" | null;
  focusedPanel: "timeline" | "mixer" | "browser" | "inspector" | null;
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
  setSelectedClipIds: (ids: ClipId[]) => void;
  toggleClipSelection: (id: ClipId) => void;
  setSelectedTrackId: (id: TrackId | null) => void;
  setSelectedMixerTrackId: (id: TrackId | "master" | null) => void;
  setFocusedPanel: (panel: UIStore["focusedPanel"]) => void;
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
  // command palette
  commandPaletteOpen: boolean;
  setCommandPaletteOpen: (open: boolean) => void;
  toggleCommandPalette: () => void;
  // context menu
  contextMenuOpen: boolean;
  contextMenuPosition: { x: number; y: number };
  contextMenuItems: AppMenuItem[];
  setContextMenu: (open: boolean, position?: { x: number; y: number }, items?: AppMenuItem[]) => void;
};

export const useUIStore = create<UIStore>((set) => ({
  pixelsPerSecond: 100,
  scrollX: 0,
  selectedClipIds: [],
  selectedTrackId: null,
  selectedMixerTrackId: null,
  focusedPanel: "timeline",
  masterVolume: 1,
  inspectorOpen: true,
  mixerOpen: true,
  snapToGrid: true,
  loopEnabled: false,
  loopStart: 0,
  loopEnd: 4,
  mixerHeight: MIXER_HEIGHT,
  mixerChannelWidth: 88,
  mixerFlexLayout: false,
  setPixelsPerSecond: (pixelsPerSecond) => set({ pixelsPerSecond }),
  setScrollX: (scrollX) => set({ scrollX }),
  setSelectedClipIds: (selectedClipIds) => set({ selectedClipIds }),
  toggleClipSelection: (id) => set((s) => ({
    selectedClipIds: s.selectedClipIds.includes(id)
      ? s.selectedClipIds.filter((x) => x !== id)
      : [...s.selectedClipIds, id]
  })),
  setSelectedTrackId: (selectedTrackId) => set({ selectedTrackId }),
  setSelectedMixerTrackId: (selectedMixerTrackId) => set({ selectedMixerTrackId }),
  setFocusedPanel: (focusedPanel) => set({ focusedPanel }),
  setMasterVolume: (masterVolume) => set({ masterVolume }),
  toggleInspector: () => set((s) => ({ inspectorOpen: !s.inspectorOpen })),
  toggleMixer: () => set((s) => ({ mixerOpen: !s.mixerOpen })),
  toggleSnapToGrid: () => set((s) => ({ snapToGrid: !s.snapToGrid })),
  toggleLoop: () => set((s) => ({ loopEnabled: !s.loopEnabled })),
  setLoopStart: (loopStart) => set({ loopStart }),
  setLoopEnd: (loopEnd) => set({ loopEnd }),
  setMixerHeight: (mixerHeight) => set({ mixerHeight: Math.max(160, Math.min(520, mixerHeight)) }),
  setMixerChannelWidth: (mixerChannelWidth) => set({ mixerChannelWidth: Math.max(72, Math.min(180, mixerChannelWidth)) }),
  toggleMixerFlexLayout: () => set((s) => ({ mixerFlexLayout: !s.mixerFlexLayout })),
  draggingClipTargetIdx: null,
  setDraggingClipTargetIdx: (draggingClipTargetIdx) => set({ draggingClipTargetIdx }),
  commandPaletteOpen: false,
  setCommandPaletteOpen: (commandPaletteOpen) => set({ commandPaletteOpen }),
  toggleCommandPalette: () => set((s) => ({ commandPaletteOpen: !s.commandPaletteOpen })),
  contextMenuOpen: false,
  contextMenuPosition: { x: 0, y: 0 },
  contextMenuItems: [],
  setContextMenu: (open, position, items) => set((s) => ({
    contextMenuOpen: open,
    contextMenuPosition: position ?? s.contextMenuPosition,
    contextMenuItems: items ?? s.contextMenuItems,
  })),
}));
