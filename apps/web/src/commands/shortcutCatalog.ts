/**
 * Shortcut catalog — single source of truth for the Keyboard Shortcuts panel.
 *
 * Merges two sources:
 *  1. Menu items that have an `accelerator` (from flattenMenuItems).
 *  2. Keyboard-only shortcuts wired directly in useKeyboardShortcuts.ts.
 *
 * No shortcut data is duplicated here by hand unless the hook doesn't expose it
 * through a menu item. The keyboard-only list must be kept in sync with the hook.
 */
import { flattenMenuItems } from "../menu/menuHelper";

// ── Types ────────────────────────────────────────────────────────────────────

export type ShortcutCategory =
  | "File"
  | "Edit"
  | "Transport"
  | "Arrangement"
  | "MIDI"
  | "Audio"
  | "Automation"
  | "Mixer"
  | "Window"
  | "Tools"
  | "Project"
  | "View"
  | "General";

export type ShortcutCommand = {
  id: string;
  label: string;
  category: ShortcutCategory;
  description?: string;
  keys: string[];
  source: "menu" | "keyboard";
};

// ── Category mapping ──────────────────────────────────────────────────────────

function categoryFromAction(action: string): ShortcutCategory {
  const ns = action.split(":")[0];
  switch (ns) {
    case "project": return "Project";
    case "file":    return "File";
    case "edit":    return "Edit";
    case "transport": return "Transport";
    case "track":
    case "clip":
    case "marker":  return "Arrangement";
    case "automation": return "Automation";
    case "midi":    return "MIDI";
    case "audio":   return "Audio";
    case "mixer":   return "Mixer";
    case "panel":
    case "layout":
    case "view":
    case "window":  return "Window";
    case "tools":   return "Tools";
    default:        return "General";
  }
}

// ── Keyboard-only shortcuts ───────────────────────────────────────────────────
// These are shortcuts implemented in useKeyboardShortcuts.ts that have NO
// corresponding menu item with an accelerator. Manually kept in sync with
// the hook. Keys use display-friendly strings (arrows as ← → etc.)

const KEYBOARD_ONLY: Omit<ShortcutCommand, "source">[] = [
  // Transport
  { id: "kb:play-pause",        label: "Play / Pause",             category: "Transport",   keys: ["Space"] },
  { id: "kb:stop",              label: "Stop",                     category: "Transport",   keys: ["Enter"] },
  { id: "kb:go-to-start",       label: "Go to Start",              category: "Transport",   keys: ["Home"] },
  { id: "kb:go-to-end",         label: "Go to End",                category: "Transport",   keys: ["End"] },
  { id: "kb:nudge-left",        label: "Nudge Left 1 Beat",        category: "Transport",   keys: ["←"] },
  { id: "kb:nudge-right",       label: "Nudge Right 1 Beat",       category: "Transport",   keys: ["→"] },
  { id: "kb:nudge-left-bar",    label: "Nudge Left 1 Bar",         category: "Transport",   keys: ["Shift+←"] },
  { id: "kb:nudge-right-bar",   label: "Nudge Right 1 Bar",        category: "Transport",   keys: ["Shift+→"] },
  { id: "kb:toggle-loop",       label: "Toggle Loop",              category: "Transport",   keys: ["L"] },
  { id: "kb:toggle-metronome",  label: "Toggle Metronome",         category: "Transport",   keys: ["K"] },
  // View / Zoom
  { id: "kb:zoom-in",           label: "Zoom In",                  category: "View",        keys: ["+"] },
  { id: "kb:zoom-out",          label: "Zoom Out",                 category: "View",        keys: ["-"] },
  { id: "kb:zoom-reset",        label: "Reset Zoom",               category: "View",        keys: ["Ctrl+0"] },
  { id: "kb:toggle-snap",       label: "Toggle Snap to Grid",      category: "View",        keys: ["N"] },
  // Tools
  { id: "kb:tool-pointer",      label: "Pointer Tool",             category: "Tools",       keys: ["V"] },
  { id: "kb:tool-pen",          label: "Pen Tool",                 category: "Tools",       keys: ["P"] },
  { id: "kb:tool-cut",          label: "Cut Tool",                 category: "Tools",       keys: ["C"] },
  { id: "kb:tool-glue",         label: "Glue Tool",                category: "Tools",       keys: ["G"] },
  { id: "kb:tool-time",         label: "Time Stretch Tool",        category: "Tools",       keys: ["T"] },
  { id: "kb:tool-automation",   label: "Automation Tool",          category: "Tools",       keys: ["A"] },
  // Arrangement
  { id: "kb:split-clip",        label: "Split Clip at Playhead",   category: "Arrangement", keys: ["S"] },
  // Edit
  { id: "kb:deselect",          label: "Deselect All",             category: "Edit",        keys: ["Escape"] },
  { id: "kb:delete-selection",  label: "Delete Selection",         category: "Edit",        keys: ["Delete"] },
  // Window / Panels
  { id: "kb:toggle-browser",   label: "Toggle Browser",            category: "Window",      keys: ["B"] },
  { id: "kb:toggle-inspector",  label: "Toggle Inspector",         category: "Window",      keys: ["I"] },
  { id: "kb:toggle-mixer",      label: "Toggle Mixer",             category: "Window",      keys: ["M"] },
  { id: "kb:show-browser",      label: "Show Browser",             category: "Window",      keys: ["Ctrl+1"] },
  { id: "kb:show-inspector",    label: "Show Inspector",           category: "Window",      keys: ["Ctrl+2"] },
  { id: "kb:show-mixer",        label: "Show Mixer",               category: "Window",      keys: ["Ctrl+3"] },
  // General
  { id: "kb:command-palette",   label: "Command Palette",          category: "General",     keys: ["Ctrl+K"] },
];

// ── Main export ───────────────────────────────────────────────────────────────

let _cached: ShortcutCommand[] | null = null;

export function getShortcutCommands(): ShortcutCommand[] {
  if (_cached) return _cached;

  // Source 1: menu items with an accelerator
  const menuCommands: ShortcutCommand[] = flattenMenuItems()
    .filter((item) => item.accelerator && item.action && item.action !== "noop")
    .map((item) => ({
      id: item.action,
      label: item.label,
      category: categoryFromAction(item.action),
      description: item.description,
      keys: [item.accelerator!],
      source: "menu" as const,
    }));

  // Deduplicate: if a menu command already exists with same id, skip keyboard-only
  const menuActionIds = new Set(menuCommands.map((c) => c.id));

  const kbCommands: ShortcutCommand[] = KEYBOARD_ONLY
    .filter((kb) => !menuActionIds.has(kb.id))
    .map((kb) => ({ ...kb, source: "keyboard" as const }));

  const all = [...menuCommands, ...kbCommands].sort((a, b) => {
    if (a.category !== b.category) return a.category.localeCompare(b.category);
    return a.label.localeCompare(b.label);
  });

  _cached = all;
  return all;
}

/** Detect conflicts: two commands with the same key chord. */
export type ShortcutConflict = {
  key: string;
  commandIds: string[];
};

export function detectShortcutConflicts(commands: ShortcutCommand[]): ShortcutConflict[] {
  const byKey = new Map<string, string[]>();
  for (const cmd of commands) {
    for (const k of cmd.keys) {
      const norm = k.toLowerCase();
      if (!byKey.has(norm)) byKey.set(norm, []);
      byKey.get(norm)!.push(cmd.id);
    }
  }
  const conflicts: ShortcutConflict[] = [];
  for (const [key, ids] of byKey) {
    if (ids.length > 1) conflicts.push({ key, commandIds: ids });
  }
  return conflicts;
}

export const ALL_SHORTCUT_CATEGORIES: ShortcutCategory[] = [
  "File", "Edit", "Transport", "Arrangement", "MIDI", "Audio",
  "Automation", "Mixer", "Window", "Tools", "Project", "View", "General",
];
