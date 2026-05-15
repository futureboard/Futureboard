/**
 * Unified selection types and selectors — Phase 1.
 *
 * Architecture:
 *   - SelectionState is the canonical shape for all selection data.
 *   - buildSelectionState() bridges from existing uiStore fields to this shape.
 *   - getActiveSelectionContext() returns the priority-ordered "what is selected"
 *     answer that commands, menus, Inspector, and Status Bar should consume.
 *   - Phase 2+: selectionState may become primary store state; for now it is
 *     derived on demand from uiStore snapshots.
 *
 * Phase scope:
 *   Phase 1 (this file): types, helpers, read-only selectors.
 *   Phase 2: mixer / insert-device selection sync.
 *   Phase 3: MIDI note selection migration.
 *   Phase 4: automation point selection.
 *   Phase 5: remove deprecated duplicate fields.
 */

import type { DawProject } from "../types/daw";

// ── Domain ─────────────────────────────────────────────────────────────────

export type SelectionDomain =
  | "arrangement"
  | "midi-editor"
  | "audio-editor"
  | "automation"
  | "mixer"
  | "effect-editor"
  | "browser"
  | "inspector"
  | "menu"
  | "dialog"
  | "none";

/** Maps uiStore.focusedPanel to SelectionDomain. */
export function focusedPanelToDomain(panel: string | null | undefined): SelectionDomain {
  switch (panel) {
    case "timeline":  return "arrangement";
    case "mixer":     return "mixer";
    case "browser":   return "browser";
    case "inspector": return "inspector";
    default:          return "none";
  }
}

// ── Mode ───────────────────────────────────────────────────────────────────

export type SelectionMode = "replace" | "add" | "toggle" | "range" | "clear";

/** Derive SelectionMode from a pointer or keyboard event snapshot. */
export function getSelectionMode(event: {
  shiftKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
}): SelectionMode {
  if (event.shiftKey) return "range";
  if (event.ctrlKey || event.metaKey) return "toggle";
  return "replace";
}

export const isAdditiveSelection = (e: { ctrlKey: boolean; metaKey: boolean }): boolean =>
  e.ctrlKey || e.metaKey;

export const isToggleSelection = isAdditiveSelection;

export const isRangeSelection = (e: { shiftKey: boolean }): boolean => e.shiftKey;

// ── Sub-types ──────────────────────────────────────────────────────────────

export type SelectedMidiNotes = {
  clipId: string;
  noteIds: string[];
};

export type SelectedAutomationPoints = {
  trackId: string;
  laneId: string;
  pointIds: string[];
};

export type SelectedInsertDevice = {
  trackId: string;
  deviceId: string;
};

export type TimelineRangeSelection = {
  startBeat: number;
  endBeat: number;
  trackIds?: string[];
};

export type SelectionRef =
  | { kind: "track"; trackId: string }
  | { kind: "clip"; clipId: string; trackId?: string }
  | { kind: "midi-note"; clipId: string; noteId: string }
  | { kind: "automation-point"; trackId: string; laneId: string; pointId: string }
  | { kind: "insert-device"; trackId: string; deviceId: string }
  | { kind: "browser-asset"; assetId: string };

// ── Unified selection state ────────────────────────────────────────────────

export type SelectionState = {
  /** Which UI surface holds keyboard / command focus. */
  focusedDomain: SelectionDomain;
  /** Selected track IDs (arrangement / mixer). Phase 1: at most one. */
  trackIds: string[];
  /** Selected clip IDs (arrangement). */
  clipIds: string[];
  /** MIDI note selection — scoped to a single clip. Phase 3: fully wired. */
  midi: SelectedMidiNotes | null;
  /** Automation point selection — scoped to a single lane. Phase 4. */
  automation: SelectedAutomationPoints | null;
  /** Automation clip IDs (treated like regular clips for now). */
  automationClipIds: string[];
  /** Insert device selection — { trackId, deviceId }. Phase 2. */
  insertDevice: SelectedInsertDevice | null;
  /** Browser asset IDs. */
  browserAssetIds: string[];
  /** Loop / range selection across the timeline. */
  timelineRange: TimelineRangeSelection | null;
  /** Time-range selection inside an audio / MIDI editor. */
  editorRange: TimelineRangeSelection | null;
  /** Anchor point for range selections (Shift+click start). */
  anchor: SelectionRef | null;
  /** Most recently interacted-with item (for Inspector / Status Bar). */
  lastTouched: SelectionRef | null;
};

// ── Bridge from uiStore ────────────────────────────────────────────────────

/**
 * Build a SelectionState snapshot from the uiStore field subset.
 * Call this wherever you need to pass SelectionState to a selector;
 * do NOT call getState() inside render — read fields via hooks first.
 */
export function buildSelectionState(uiSnapshot: {
  focusedPanel?: string | null;
  selectedTrackId?: string | null;
  selectedClipIds?: string[];
  selectedBrowserFileId?: string | null;
  // Phase 2+: insertDevice, midi, automation wired here.
}): SelectionState {
  return {
    focusedDomain: focusedPanelToDomain(uiSnapshot.focusedPanel),
    trackIds: uiSnapshot.selectedTrackId ? [uiSnapshot.selectedTrackId] : [],
    clipIds: uiSnapshot.selectedClipIds ?? [],
    midi: null,               // Phase 3
    automation: null,         // Phase 4
    automationClipIds: [],
    insertDevice: null,       // Phase 2
    browserAssetIds: uiSnapshot.selectedBrowserFileId ? [uiSnapshot.selectedBrowserFileId] : [],
    timelineRange: null,
    editorRange: null,
    anchor: null,
    lastTouched: null,
  };
}

// ── Active selection context ───────────────────────────────────────────────

export type ActiveSelectionContext =
  | { kind: "midi-notes";         clipId: string; noteIds: string[] }
  | { kind: "automation-points";  trackId: string; laneId: string; pointIds: string[] }
  | { kind: "insert-device";      trackId: string; deviceId: string }
  | { kind: "clips";              clipIds: string[] }
  | { kind: "tracks";             trackIds: string[] }
  | { kind: "browser-assets";     assetIds: string[] }
  | { kind: "timeline-range";     range: TimelineRangeSelection }
  | { kind: "none" };

/**
 * Returns the active selection context using spec §7 priority order:
 *
 *  1. MIDI notes      — only when focusedDomain = "midi-editor"
 *  2. Automation pts  — only when focusedDomain = "automation"
 *  3. Insert device   — only when focusedDomain = "effect-editor"
 *  4. Clips           — available in arrangement / any non-special domain
 *  5. Tracks
 *  6. Browser assets
 *  7. Timeline range
 *  8. None
 *
 * Commands (delete, duplicate, select-all) and menus should call this
 * instead of independently inspecting individual store fields.
 */
export function getActiveSelectionContext(selection: SelectionState): ActiveSelectionContext {
  const { focusedDomain: d } = selection;

  if (d === "midi-editor" && selection.midi && selection.midi.noteIds.length > 0) {
    return { kind: "midi-notes", clipId: selection.midi.clipId, noteIds: selection.midi.noteIds };
  }

  if (d === "automation" && selection.automation && selection.automation.pointIds.length > 0) {
    return {
      kind: "automation-points",
      trackId: selection.automation.trackId,
      laneId:  selection.automation.laneId,
      pointIds: selection.automation.pointIds,
    };
  }

  if (d === "effect-editor" && selection.insertDevice) {
    return {
      kind:     "insert-device",
      trackId:  selection.insertDevice.trackId,
      deviceId: selection.insertDevice.deviceId,
    };
  }

  // Clips: available in arrangement or when domain is unspecified ("none").
  // Excluded from browser/inspector focus to avoid accidental cross-domain edits.
  if (selection.clipIds.length > 0 && d !== "browser" && d !== "inspector") {
    return { kind: "clips", clipIds: selection.clipIds };
  }

  if (selection.trackIds.length > 0 && d !== "browser" && d !== "inspector") {
    return { kind: "tracks", trackIds: selection.trackIds };
  }

  if (selection.browserAssetIds.length > 0) {
    return { kind: "browser-assets", assetIds: selection.browserAssetIds };
  }

  if (selection.timelineRange) {
    return { kind: "timeline-range", range: selection.timelineRange };
  }

  return { kind: "none" };
}

// ── Can-do predicates (used by menus and actionRunner) ─────────────────────

export function canDeleteSelection(selection: SelectionState): boolean {
  const ctx = getActiveSelectionContext(selection);
  switch (ctx.kind) {
    case "clips":
    case "tracks":
    case "midi-notes":
    case "automation-points":
    case "insert-device":
      return true;
    default:
      return false;
  }
}

export function canDuplicateSelection(selection: SelectionState): boolean {
  const ctx = getActiveSelectionContext(selection);
  return ctx.kind === "clips" || ctx.kind === "midi-notes";
}

export function canCopySelection(selection: SelectionState): boolean {
  return canDuplicateSelection(selection);
}

export function hasSelection(selection: SelectionState): boolean {
  return getActiveSelectionContext(selection).kind !== "none";
}

export function hasClipSelection(selection: SelectionState): boolean {
  return selection.clipIds.length > 0;
}

export function hasNoteSelection(selection: SelectionState): boolean {
  return (selection.midi?.noteIds.length ?? 0) > 0;
}

export function hasDeviceSelection(selection: SelectionState): boolean {
  return selection.insertDevice !== null;
}

// ── Project-aware selectors ────────────────────────────────────────────────

export function getPrimarySelectedTrack(project: DawProject, selection: SelectionState) {
  if (!selection.trackIds.length) return null;
  return project.tracks.find((t) => t.id === selection.trackIds[0]) ?? null;
}

export function getPrimarySelectedClip(project: DawProject, selection: SelectionState) {
  if (!selection.clipIds.length) return null;
  return (
    project.tracks.flatMap((t) => t.clips).find((c) => c.id === selection.clipIds[0]) ?? null
  );
}

export function getSelectedTracks(project: DawProject, selection: SelectionState) {
  const ids = new Set(selection.trackIds);
  return project.tracks.filter((t) => ids.has(t.id));
}

export function getSelectedClips(project: DawProject, selection: SelectionState) {
  const ids = new Set(selection.clipIds);
  return project.tracks.flatMap((t) => t.clips).filter((c) => ids.has(c.id));
}

/**
 * Human-readable summary for the Status Bar.
 *
 * Examples:
 *   "Audio Clip · Drums"
 *   "3 clips"
 *   "Audio Track 1"
 *   "7 MIDI notes"
 *   ""   (no selection → caller renders fallback)
 */
export function getSelectionSummary(project: DawProject, selection: SelectionState): string {
  const ctx = getActiveSelectionContext(selection);

  switch (ctx.kind) {
    case "midi-notes":
      return ctx.noteIds.length === 1 ? "1 MIDI note" : `${ctx.noteIds.length} MIDI notes`;

    case "automation-points":
      return ctx.pointIds.length === 1
        ? "1 automation point"
        : `${ctx.pointIds.length} automation points`;

    case "insert-device":
      return "Device selected";

    case "clips": {
      if (ctx.clipIds.length === 1) {
        const clip = project.tracks
          .flatMap((t) => t.clips)
          .find((c) => c.id === ctx.clipIds[0]);
        if (clip) return `${clip.type === "midi" ? "MIDI" : "Audio"} Clip · ${clip.name}`;
        return "1 clip";
      }
      return `${ctx.clipIds.length} clips`;
    }

    case "tracks": {
      if (ctx.trackIds.length === 1) {
        const track = project.tracks.find((t) => t.id === ctx.trackIds[0]);
        if (track) return track.name;
      }
      return `${ctx.trackIds.length} tracks`;
    }

    case "browser-assets":
      return ctx.assetIds.length === 1 ? "1 file" : `${ctx.assetIds.length} files`;

    case "timeline-range":
      return "Range selected";

    default:
      return "";
  }
}

// ── Cleanup ────────────────────────────────────────────────────────────────

/**
 * Remove stale selection references after project mutations (delete, load).
 * Call after: deleting tracks/clips/devices, loading a new project.
 */
export function cleanupSelection(project: DawProject, selection: SelectionState): SelectionState {
  const trackSet = new Set(project.tracks.map((t) => t.id));
  const clipSet  = new Set(project.tracks.flatMap((t) => t.clips.map((c) => c.id)));

  const trackIds = selection.trackIds.filter((id) => trackSet.has(id));
  const clipIds  = selection.clipIds.filter((id) => clipSet.has(id));

  const insertDevice =
    selection.insertDevice && trackSet.has(selection.insertDevice.trackId)
      ? selection.insertDevice
      : null;

  const midi =
    selection.midi && clipSet.has(selection.midi.clipId) ? selection.midi : null;

  return { ...selection, trackIds, clipIds, insertDevice, midi };
}
