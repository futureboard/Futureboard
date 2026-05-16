/**
 * deviceStore — runtime audio/MIDI device state.
 *
 * This is NOT project state. Devices are enumerated at runtime and updated
 * whenever permissions change or devices are plugged/unplugged. Project state
 * stores only a device id/name for display; if missing, "Missing device" is shown.
 */
import { create } from "zustand";

// ── Types ─────────────────────────────────────────────────────────────────────

export type AudioDeviceInfo = {
  id: string;
  name: string;
  kind: "audioinput" | "audiooutput";
  isDefault: boolean;
};

export type MidiDeviceInfo = {
  id: string;
  name: string;
  kind: "input" | "output";
  state: "connected" | "disconnected";
};

export type AudioPermissionState = "unknown" | "granted" | "denied" | "prompting";
export type MidiPermissionState = "unknown" | "granted" | "denied" | "unsupported" | "prompting";

// ── Store ─────────────────────────────────────────────────────────────────────

type DeviceStore = {
  audioPermission: AudioPermissionState;
  audioInputs: AudioDeviceInfo[];
  audioOutputs: AudioDeviceInfo[];

  midiPermission: MidiPermissionState;
  midiInputs: MidiDeviceInfo[];
  midiOutputs: MidiDeviceInfo[];

  setAudioPermission: (state: AudioPermissionState) => void;
  setAudioDevices: (inputs: AudioDeviceInfo[], outputs: AudioDeviceInfo[]) => void;
  setMidiPermission: (state: MidiPermissionState) => void;
  setMidiDevices: (inputs: MidiDeviceInfo[], outputs: MidiDeviceInfo[]) => void;
};

export const useDeviceStore = create<DeviceStore>((set) => ({
  audioPermission: "unknown",
  audioInputs: [],
  audioOutputs: [],

  midiPermission: "unknown",
  midiInputs: [],
  midiOutputs: [],

  setAudioPermission: (audioPermission) => set({ audioPermission }),
  setAudioDevices: (audioInputs, audioOutputs) => set({ audioInputs, audioOutputs }),
  setMidiPermission: (midiPermission) => set({ midiPermission }),
  setMidiDevices: (midiInputs, midiOutputs) => set({ midiInputs, midiOutputs }),
}));

// ── Selectors ─────────────────────────────────────────────────────────────────

/** Label shown in the UI for a stored device id (may be missing/unplugged). */
export function resolveAudioInputLabel(
  deviceId: string | undefined,
  inputs: AudioDeviceInfo[]
): string {
  if (!deviceId || deviceId === "none") return "None";
  if (deviceId === "system-audio") return "System Input";
  const found = inputs.find((d) => d.id === deviceId);
  return found ? found.name : "Missing device";
}

export function resolveAudioOutputLabel(
  deviceId: string | undefined,
  outputs: AudioDeviceInfo[]
): string {
  if (!deviceId || deviceId === "none") return "None";
  if (deviceId === "master") return "Master";
  const found = outputs.find((d) => d.id === deviceId);
  return found ? found.name : "Missing device";
}

export function resolveMidiInputLabel(
  deviceId: string | undefined,
  inputs: MidiDeviceInfo[]
): string {
  if (!deviceId || deviceId === "none") return "None";
  const found = inputs.find((d) => d.id === deviceId);
  return found ? found.name : "Missing device";
}
