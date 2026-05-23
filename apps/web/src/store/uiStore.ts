import { create } from "zustand";
import type { ClipId, TrackId } from "../types/daw";
import type { AppMenuItem } from "../menu/menuItems";
import type { SnapDivision } from "../utils/musicalTime";
import { MIXER_HEIGHT, BROWSER_WIDTH, INSPECTOR_WIDTH } from "../theme";

export type PanelDock = "left" | "right" | "bottom" | "float";
export type BottomPanelTab = "mixer" | "editor" | "effect-editor";
export type ArrangementTool =
  | "pointer"
  | "pen"
  | "cut"
  | "glue"
  | "mute"
  | "time"
  | "automation";
export type PanelSizing = "fixed" | "flex";

export type PanelLayout = {
  id: string;
  visible: boolean;
  dock: PanelDock;
  sizing: PanelSizing;
  size: number;
  minSize: number;
  maxSize: number;
};

export type DOMRectLike = { x: number; y: number; width: number; height: number; left: number; top: number; right: number; bottom: number };

export type MarqueeSelectionState = {
  active: boolean;
  pointerId: number;
  startClientX: number;
  startClientY: number;
  currentClientX: number;
  currentClientY: number;
  rect: DOMRectLike;
  affectedClipIds: string[];
  affectedTrackIds: string[];
};

type UIStore = {
  pixelsPerSecond: number;
  scrollX: number;
  selectedClipIds: ClipId[];
  selectedTrackId: TrackId | null;
  selectedTrackIds: TrackId[];
  selectedMixerTrackId: TrackId | "master" | null;
  focusedPanel: "timeline" | "mixer" | "browser" | "inspector" | null;
  masterVolume: number;
  panels: Record<string, PanelLayout>;
  setPanelLayout: (id: string, layout: Partial<PanelLayout>) => void;
  togglePanel: (id: string) => void;
  applyWorkspaceLayout: (layoutName: string) => void;
  snapToGrid: boolean;
  arrangementGridDivision: SnapDivision;
  loopEnabled: boolean;
  loopStart: number;
  loopEnd: number;
  // Mixer layout
  mixerChannelWidth: number;
  mixerFlexLayout: boolean;
  // Bottom workspace tabs
  bottomPanelTab: BottomPanelTab;
  setBottomPanelTab: (tab: BottomPanelTab) => void;
  setPixelsPerSecond: (v: number) => void;
  setScrollX: (v: number) => void;
  setSelectedClipIds: (ids: ClipId[]) => void;
  toggleClipSelection: (id: ClipId) => void;
  setSelectedTrackId: (id: TrackId | null) => void;
  setSelectedTrackIds: (ids: TrackId[]) => void;
  toggleTrackInSelection: (id: TrackId) => void;
  setSelectedMixerTrackId: (id: TrackId | "master" | null) => void;
  setFocusedPanel: (panel: UIStore["focusedPanel"]) => void;
  setMasterVolume: (v: number) => void;
  toggleSnapToGrid: () => void;
  setArrangementGridDivision: (division: SnapDivision) => void;
  toggleLoop: () => void;
  setLoopStart: (seconds: number) => void;
  setLoopEnd: (seconds: number) => void;
  setMixerChannelWidth: (w: number) => void;
  toggleMixerFlexLayout: () => void;
  // Arrangement editing tool
  currentTool: ArrangementTool;
  setCurrentTool: (tool: ArrangementTool) => void;
  // Browser file selection (used by pen tool to create audio clips)
  selectedBrowserFileId: string | null;
  setSelectedBrowserFileId: (id: string | null) => void;
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
  // Project save status — driven by App-level save/dirty detection
  saveStatus: "saved" | "unsaved" | "saving" | "error";
  setSaveStatus: (status: UIStore["saveStatus"]) => void;
  // Marquee Selection gesture
  marqueeSelection: MarqueeSelectionState | null;
  setMarqueeSelection: (marqueeSelection: MarqueeSelectionState | null) => void;
  // Track area virtualization scroll state
  scrollY: number;
  setScrollY: (v: number) => void;
  trackAreaHeight: number;
  setTrackAreaHeight: (h: number) => void;
};

export const useUIStore = create<UIStore>((set) => ({
  pixelsPerSecond: 100,
  scrollX: 0,
  selectedClipIds: [],
  selectedTrackId: null,
  selectedTrackIds: [],
  selectedMixerTrackId: null,
  focusedPanel: "timeline",
  masterVolume: 1,
  panels: {
    browser: { id: "browser", visible: true, dock: "left", sizing: "fixed", size: BROWSER_WIDTH, minSize: 200, maxSize: 400 },
    inspector: { id: "inspector", visible: true, dock: "right", sizing: "fixed", size: INSPECTOR_WIDTH, minSize: 240, maxSize: 500 },
    mixer: { id: "mixer", visible: true, dock: "bottom", sizing: "fixed", size: MIXER_HEIGHT, minSize: 160, maxSize: 600 },
  },
  setPanelLayout: (id, layout) => set((s) => ({
    panels: { ...s.panels, [id]: { ...s.panels[id], ...layout } }
  })),
  togglePanel: (id) => set((s) => {
    const p = s.panels[id];
    if (!p) return s;
    return { panels: { ...s.panels, [id]: { ...p, visible: !p.visible } } };
  }),
  applyWorkspaceLayout: (layoutName) => set((s) => {
    const p = { ...s.panels };
    // Reset defaults first
    Object.keys(p).forEach(k => p[k].visible = true);
    if (layoutName === "Editing") {
      if (p.mixer) p.mixer.visible = false;
    } else if (layoutName === "Mixing") {
      if (p.browser) p.browser.visible = false;
      if (p.mixer) { p.mixer.visible = true; p.mixer.size = 360; }
    } else if (layoutName === "Sound Design") {
      if (p.browser) p.browser.visible = true;
      if (p.inspector) p.inspector.visible = true;
      if (p.mixer) p.mixer.visible = true;
    } else if (layoutName === "Minimal") {
      if (p.browser) p.browser.visible = false;
      if (p.inspector) p.inspector.visible = false;
      if (p.mixer) p.mixer.visible = false;
    } else if (layoutName === "Laptop") {
      if (p.browser) p.browser.visible = false;
    }
    return { panels: p };
  }),
  snapToGrid: true,
  arrangementGridDivision: "auto",
  loopEnabled: false,
  loopStart: 0,
  loopEnd: 4,
  mixerHeight: MIXER_HEIGHT,
  mixerChannelWidth: 88,
  mixerFlexLayout: false,
  bottomPanelTab: "mixer",
  setBottomPanelTab: (bottomPanelTab) => set({ bottomPanelTab }),
  setPixelsPerSecond: (pixelsPerSecond) => set({ pixelsPerSecond }),
  setScrollX: (scrollX) => set({ scrollX }),
  setSelectedClipIds: (selectedClipIds) => set({ selectedClipIds }),
  toggleClipSelection: (id) => set((s) => ({
    selectedClipIds: s.selectedClipIds.includes(id)
      ? s.selectedClipIds.filter((x) => x !== id)
      : [...s.selectedClipIds, id]
  })),
  setSelectedTrackId: (selectedTrackId) => set({ selectedTrackId }),
  setSelectedTrackIds: (selectedTrackIds) => set({ selectedTrackIds }),
  toggleTrackInSelection: (id) => set((s) => ({
    selectedTrackIds: s.selectedTrackIds.includes(id)
      ? s.selectedTrackIds.filter((x) => x !== id)
      : [...s.selectedTrackIds, id],
  })),
  setSelectedMixerTrackId: (selectedMixerTrackId) => set({ selectedMixerTrackId }),
  setFocusedPanel: (focusedPanel) => set({ focusedPanel }),
  setMasterVolume: (masterVolume) => set({ masterVolume }),
  toggleSnapToGrid: () => set((s) => ({ snapToGrid: !s.snapToGrid })),
  setArrangementGridDivision: (arrangementGridDivision) => set({ arrangementGridDivision }),
  toggleLoop: () => set((s) => ({ loopEnabled: !s.loopEnabled })),
  setLoopStart: (loopStart) => set({ loopStart }),
  setLoopEnd: (loopEnd) => set({ loopEnd }),
  setMixerChannelWidth: (mixerChannelWidth) => set({ mixerChannelWidth: Math.max(72, Math.min(180, mixerChannelWidth)) }),
  toggleMixerFlexLayout: () => set((s) => ({ mixerFlexLayout: !s.mixerFlexLayout })),
  currentTool: "pointer",
  setCurrentTool: (currentTool) => set({ currentTool }),
  selectedBrowserFileId: null,
  setSelectedBrowserFileId: (selectedBrowserFileId) => set({ selectedBrowserFileId }),
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
  saveStatus: "saved",
  setSaveStatus: (saveStatus) => set({ saveStatus }),
  marqueeSelection: null,
  setMarqueeSelection: (marqueeSelection) => set({ marqueeSelection }),
  scrollY: 0,
  setScrollY: (scrollY) => set({ scrollY }),
  trackAreaHeight: 600,
  setTrackAreaHeight: (trackAreaHeight) => set({ trackAreaHeight }),
}));
