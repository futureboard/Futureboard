import { create } from "zustand";

export type AudioBackendKind = "web-audio" | "rust-wasm" | "sphere-native";
export type AudioRuntime = "web" | "electron";
export type AudioBackendRequest = "auto" | "force-web" | "force-native";

export type AudioBackendState = {
  runtime: AudioRuntime;
  requested: AudioBackendRequest;
  active: AudioBackendKind | null;
  available: {
    webAudio: boolean;
    rustWasm: boolean;
    sphereNative: boolean;
  };
  initialized: boolean;
  healthy: boolean;
  error?: string;
  fallbackReason?: string;
  contextState?: string;
  device?: string;
  version?: string;
};

type AudioBackendStore = AudioBackendState & {
  setRuntime: (runtime: AudioRuntime) => void;
  setRequested: (requested: AudioBackendRequest) => void;
  setAvailability: (available: Partial<AudioBackendState["available"]>) => void;
  setActive: (active: AudioBackendKind | null, patch?: Partial<AudioBackendState>) => void;
  setHealth: (healthy: boolean, error?: string) => void;
};

const DEFAULT_AVAILABILITY: AudioBackendState["available"] = {
  webAudio: false,
  rustWasm: false,
  sphereNative: false,
};

export const useAudioBackendStore = create<AudioBackendStore>((set) => ({
  runtime: "web",
  requested: "auto",
  active: null,
  available: DEFAULT_AVAILABILITY,
  initialized: false,
  healthy: false,

  setRuntime: (runtime) => set({ runtime }),
  setRequested: (requested) => set({ requested }),
  setAvailability: (available) =>
    set((state) => ({ available: { ...state.available, ...available } })),
  setActive: (active, patch) =>
    set({
      active,
      initialized: active !== null,
      healthy: active !== null && !patch?.error,
      error: undefined,
      fallbackReason: undefined,
      ...patch,
    }),
  setHealth: (healthy, error) => set({ healthy, error }),
}));
