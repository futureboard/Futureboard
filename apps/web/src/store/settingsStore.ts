import { create } from "zustand";

const STORAGE_KEY = "futureboard.appSettings.v1";

export type StartupBehavior = "wizard" | "newProject" | "lastProject";
export type PreferredEngine = "auto" | "wasm" | "webAudio" | "native-sphere-direct";
export type PreferredBufferSize = 64 | 128 | 256 | 512 | 1024;

/** DAUx OS-level audio backend selection. */
export type DauxBackend = "wasapi" | "wasapi-exclusive" | "mme" | "coreaudio" | "alsa";

/** Audio engine sample rate override. "device-default" lets the driver choose. */
export type AudioSampleRate = "device-default" | 44100 | 48000 | 96000;

/** Top-level engine kind visible in the UI (derived from runtime, not user-selectable). */
export type AudioEngineKind = "daux" | "wasm";

export type ExtraFolderSetting = {
  id: string;
  name: string;
  path: string;
  enabled: boolean;
  addedAt: number;
};

export type GraphicRenderingMode = "auto" | "force" | "software";
export type VisualFrameRate = 45 | 60 | 120 | "unlimited";

export type AppSettings = {
  startupBehavior: StartupBehavior;
  autoSave: boolean;
  autoSaveIntervalMin: number;
  preferredEngine: PreferredEngine;
  preferredBufferSize: PreferredBufferSize;
  /** DAUx OS backend (Electron only). Defaults to the platform-appropriate backend. */
  dauxBackend?: DauxBackend;
  /** Audio engine sample rate (Electron / DAUx only). */
  audioSampleRate: AudioSampleRate;
  /** Extra user folders shown in Browser/Library and indexed by Electron. */
  extraFolders: ExtraFolderSetting[];
  compactUI: boolean;
  enableDevTools: boolean;
  /** GPU vs software rendering (Electron only). Requires restart. Persisted to settings.json. */
  graphicRenderingMode: GraphicRenderingMode;
  /** Visual/UI refresh cap for meters, playhead, timeline overlays, and diagnostics. */
  visualFrameRate: VisualFrameRate;
};

const DEFAULTS: AppSettings = {
  startupBehavior: "wizard",
  autoSave: true,
  autoSaveIntervalMin: 5,
  preferredEngine: "auto",
  preferredBufferSize: 256,
  dauxBackend: undefined,   // resolved at runtime per-platform
  audioSampleRate: "device-default",
  extraFolders: [],
  compactUI: false,
  enableDevTools: false,
  graphicRenderingMode: "auto",
  visualFrameRate: 60,
};

function normalizeVisualFrameRate(raw: unknown): VisualFrameRate {
  return raw === 45 || raw === 60 || raw === 120 || raw === "unlimited"
    ? raw
    : DEFAULTS.visualFrameRate;
}

function normalizeExtraFolders(raw: unknown): ExtraFolderSetting[] {
  if (!Array.isArray(raw)) return [];
  const seen = new Set<string>();
  const out: ExtraFolderSetting[] = [];
  for (const item of raw) {
    if (!item || typeof item !== "object") continue;
    const obj = item as Partial<ExtraFolderSetting>;
    const path = typeof obj.path === "string" ? obj.path.trim() : "";
    if (!path || seen.has(path)) continue;
    seen.add(path);
    const name = typeof obj.name === "string" && obj.name.trim()
      ? obj.name.trim()
      : path.replace(/\\/g, "/").split("/").filter(Boolean).pop() ?? path;
    out.push({
      id: typeof obj.id === "string" && obj.id ? obj.id : `extra:${path}`,
      name,
      path,
      enabled: obj.enabled !== false,
      addedAt: typeof obj.addedAt === "number" ? obj.addedAt : Date.now(),
    });
  }
  return out;
}

function loadFromStorage(): AppSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return { ...DEFAULTS };
    const parsed = JSON.parse(raw) as Partial<AppSettings>;
    return {
      ...DEFAULTS,
      ...parsed,
      extraFolders: normalizeExtraFolders(parsed.extraFolders),
      visualFrameRate: normalizeVisualFrameRate(parsed.visualFrameRate),
    };
  } catch {
    return { ...DEFAULTS };
  }
}

function saveToStorage(s: AppSettings) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(s));
  } catch {
    // ignore quota errors
  }
}

type SettingsStore = AppSettings & {
  applySettings: (patch: Partial<AppSettings>) => void;
  resetToDefaults: () => void;
};

export const useSettingsStore = create<SettingsStore>((set) => ({
  ...loadFromStorage(),

  applySettings(patch) {
    set((s) => {
      const next: AppSettings = {
        startupBehavior:      patch.startupBehavior      ?? s.startupBehavior,
        autoSave:             patch.autoSave             ?? s.autoSave,
        autoSaveIntervalMin:  patch.autoSaveIntervalMin  ?? s.autoSaveIntervalMin,
        preferredEngine:      patch.preferredEngine      ?? s.preferredEngine,
        preferredBufferSize:  patch.preferredBufferSize  ?? s.preferredBufferSize,
        dauxBackend:          patch.dauxBackend          ?? s.dauxBackend,
        audioSampleRate:      patch.audioSampleRate      ?? s.audioSampleRate,
        extraFolders:         patch.extraFolders         ? normalizeExtraFolders(patch.extraFolders) : s.extraFolders,
        compactUI:            patch.compactUI            ?? s.compactUI,
        enableDevTools:       patch.enableDevTools       ?? s.enableDevTools,
        graphicRenderingMode: patch.graphicRenderingMode ?? s.graphicRenderingMode,
        visualFrameRate:      patch.visualFrameRate      ?? s.visualFrameRate,
      };
      saveToStorage(next);
      return next;
    });
  },

  resetToDefaults() {
    saveToStorage(DEFAULTS);
    set({ ...DEFAULTS });
  },
}));

export { DEFAULTS as APP_SETTINGS_DEFAULTS };
