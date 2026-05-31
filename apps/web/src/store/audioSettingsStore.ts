import { create } from "zustand";

const STORAGE_KEY = "futureboard.audioSettings.v1";

export type AudioDeviceSettings = {
  /** Global audio input device ID (null = system default). */
  audioInputDeviceId: string | null;
  /** Global audio output device ID (null = system default). */
  audioOutputDeviceId: string | null;
  /** Reported input channel count for the selected device. */
  audioInputChannelCount: number;
  /** Reported output channel count for the selected device. */
  audioOutputChannelCount: number;
  /**
   * Explicitly enabled MIDI input device IDs.
   * Empty array means all connected inputs are treated as enabled.
   */
  midiEnabledInputIds: string[];
  /** Explicitly enabled MIDI output device IDs. */
  midiEnabledOutputIds: string[];
};

const DEFAULTS: AudioDeviceSettings = {
  audioInputDeviceId: null,
  audioOutputDeviceId: null,
  audioInputChannelCount: 2,
  audioOutputChannelCount: 2,
  midiEnabledInputIds: [],
  midiEnabledOutputIds: [],
};

function load(): AudioDeviceSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return { ...DEFAULTS };
    return { ...DEFAULTS, ...(JSON.parse(raw) as Partial<AudioDeviceSettings>) };
  } catch {
    return { ...DEFAULTS };
  }
}

function save(s: AudioDeviceSettings) {
  try { localStorage.setItem(STORAGE_KEY, JSON.stringify(s)); } catch {
    // Ignore unavailable storage.
  }
}

type AudioSettingsStore = AudioDeviceSettings & {
  setAudioInputDevice: (id: string | null) => void;
  setAudioOutputDevice: (id: string | null) => void;
  setAudioInputChannelCount: (n: number) => void;
  setAudioOutputChannelCount: (n: number) => void;
  toggleMidiInput: (id: string) => void;
  toggleMidiOutput: (id: string) => void;
  enableAllMidiInputs: () => void;
};

export const useAudioSettingsStore = create<AudioSettingsStore>((set) => ({
  ...load(),

  setAudioInputDevice(id) {
    set((s) => { const n = { ...s, audioInputDeviceId: id }; save(n); return n; });
  },
  setAudioOutputDevice(id) {
    set((s) => { const n = { ...s, audioOutputDeviceId: id }; save(n); return n; });
  },
  setAudioInputChannelCount(count) {
    set((s) => { const n = { ...s, audioInputChannelCount: count }; save(n); return n; });
  },
  setAudioOutputChannelCount(count) {
    set((s) => { const n = { ...s, audioOutputChannelCount: count }; save(n); return n; });
  },
  toggleMidiInput(id) {
    set((s) => {
      const ids = s.midiEnabledInputIds.includes(id)
        ? s.midiEnabledInputIds.filter((x) => x !== id)
        : [...s.midiEnabledInputIds, id];
      const n = { ...s, midiEnabledInputIds: ids };
      save(n);
      return n;
    });
  },
  toggleMidiOutput(id) {
    set((s) => {
      const ids = s.midiEnabledOutputIds.includes(id)
        ? s.midiEnabledOutputIds.filter((x) => x !== id)
        : [...s.midiEnabledOutputIds, id];
      const n = { ...s, midiEnabledOutputIds: ids };
      save(n);
      return n;
    });
  },
  enableAllMidiInputs() {
    set((s) => { const n = { ...s, midiEnabledInputIds: [] }; save(n); return n; });
  },
}));
