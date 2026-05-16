/**
 * Normalization helpers — fill in default values for older project state
 * so UI and engine code never crash on missing fields.
 */
import type { DawProject, DawTrack, DawClip, InsertDevice, TrackSend, ProjectLoop, ProjectMarker, AutomationLane, AutomationPoint, AudioClipProcess, TrackRouting, TrackInputRouting, TrackOutputRouting, TrackAdvanced } from "../types/daw";

export function normalizeLoop(raw: Partial<ProjectLoop> | undefined): ProjectLoop | undefined {
  if (!raw) return undefined;
  const startTime = Math.max(0, raw.startTime ?? 0);
  const endTime = Math.max(startTime, raw.endTime ?? startTime);
  return {
    enabled: raw.enabled ?? false,
    startTime,
    endTime,
  };
}

export function normalizeMarker(raw: Partial<ProjectMarker>): ProjectMarker {
  return {
    id: raw.id ?? crypto.randomUUID(),
    time: Math.max(0, raw.time ?? 0),
    label: raw.label ?? "Marker",
    color: raw.color,
  };
}

export function normalizeInsertDevice(raw: Partial<InsertDevice>, index: number): InsertDevice {
  return {
    id: raw.id ?? crypto.randomUUID(),
    type: raw.type ?? "custom",
    name: raw.name ?? "Insert",
    // handle old 'bypassed' field: bypassed=true → enabled=false
    enabled: raw.enabled !== undefined ? raw.enabled : !(raw as Record<string, unknown>)["bypassed"],
    order: raw.order !== undefined ? raw.order : index,
    params: raw.params ?? {},
  };
}

export function normalizeSend(raw: Partial<TrackSend>): TrackSend {
  return {
    id: raw.id ?? crypto.randomUUID(),
    name: raw.name ?? "Send",
    targetTrackId: raw.targetTrackId ?? "",
    level: raw.level ?? 1,
    enabled: raw.enabled !== false,
    preFader: raw.preFader ?? false,
  };
}

export function normalizeAutomationPoint(raw: Partial<AutomationPoint>): AutomationPoint {
  return {
    id: raw.id ?? crypto.randomUUID(),
    beat: Math.max(0, raw.beat ?? 0),
    value: raw.value ?? 0,
    curve: raw.curve ?? "linear",
    selected: false,
  };
}

export function normalizeAutomationLane(raw: Partial<AutomationLane>): AutomationLane {
  return {
    id: raw.id ?? crypto.randomUUID(),
    trackId: raw.trackId ?? "",
    target: raw.target ?? {
      id: "",
      kind: "track-volume",
      label: "Volume",
      min: 0,
      max: 1,
      defaultValue: 1,
      displayScale: "linear",
    },
    visible: raw.visible !== false,
    height: raw.height ?? 72,
    points: (raw.points ?? []).map((p) => normalizeAutomationPoint(p as Partial<AutomationPoint>)),
  };
}

export const DEFAULT_TRACK_ADVANCED: TrackAdvanced = {
  latencyMs: 0,
  delayMs: 0,
  semitone: 0,
  phaseInvert: false,
  midSideMode: "off",
};

function defaultRoutingForType(type: DawTrack["type"]): TrackRouting {
  switch (type) {
    case "master":
      return { inputType: "bus", outputType: "hardware" };
    case "midi":
      return { inputType: "midi-device", outputType: "master" };
    case "bus":
    case "group":
    case "return":
      return { inputType: "bus", outputType: "master" };
    default:
      return { inputType: "system-audio", outputType: "master" };
  }
}

/** Derive a TrackInputRouting from legacy flat fields (migration helper). */
function legacyToInputRouting(raw: Partial<TrackRouting>, type: DawTrack["type"]): TrackInputRouting {
  // If already has new sub-object, pass it through (strip unknown keys).
  if (raw.input) {
    const i = raw.input;
    return { kind: i.kind, channel: i.channel, channelPair: i.channelPair, midiDeviceId: i.midiDeviceId, midiChannel: i.midiChannel, targetId: i.targetId };
  }
  if (type === "midi") {
    return { kind: "midi-input", midiDeviceId: raw.inputId, midiChannel: "all" };
  }
  if (type === "master" || type === "bus" || type === "group" || type === "return") {
    return { kind: raw.inputType === "bus" ? "bus" : "none", targetId: raw.inputId };
  }
  // audio / instrument / plugin — default to stereo system input
  if (raw.inputType === "audio-channel") {
    const ch = raw.inputChannel;
    if (typeof ch === "number") return { kind: "audio-channel", channel: ch };
    return { kind: "audio-channel", channelPair: [1, 2] };
  }
  return { kind: "audio-channel", channelPair: [1, 2] };
}

/** Derive a TrackOutputRouting from legacy flat fields (migration helper). */
function legacyToOutputRouting(raw: Partial<TrackRouting>): TrackOutputRouting {
  if (raw.output) {
    const o = raw.output;
    return { kind: o.kind, targetId: o.targetId, hardwarePair: o.hardwarePair };
  }
  if (!raw.outputType || raw.outputType === "master") return { kind: "master" };
  if (raw.outputType === "bus") return { kind: "bus", targetId: raw.outputId };
  if (raw.outputType === "hardware") return { kind: "hardware" };
  return { kind: "none" };
}

export function normalizeRouting(raw: Partial<TrackRouting> | undefined, type: DawTrack["type"]): TrackRouting {
  const defaults = defaultRoutingForType(type);
  if (!raw) {
    return {
      ...defaults,
      input: legacyToInputRouting(defaults, type),
      output: legacyToOutputRouting(defaults),
    };
  }
  const base: TrackRouting = {
    inputType: raw.inputType ?? defaults.inputType,
    inputId: raw.inputId,
    inputChannel: raw.inputChannel,
    outputType: raw.outputType ?? defaults.outputType,
    outputId: raw.outputId,
    input: legacyToInputRouting(raw, type),
    output: legacyToOutputRouting(raw),
  };
  return base;
}

export function normalizeAdvanced(raw: Partial<TrackAdvanced> | undefined): TrackAdvanced {
  if (!raw) return { ...DEFAULT_TRACK_ADVANCED };
  return {
    latencyMs: raw.latencyMs ?? 0,
    delayMs: raw.delayMs ?? 0,
    semitone: raw.semitone ?? 0,
    phaseInvert: raw.phaseInvert ?? false,
    midSideMode: raw.midSideMode ?? "off",
  };
}

export function normalizeTrack(raw: Partial<DawTrack>): DawTrack {
  const type = raw.type ?? "audio";
  const inserts = (raw.inserts ?? []).map((ins, i) =>
    normalizeInsertDevice(ins as Partial<InsertDevice>, i)
  );
  return {
    id: raw.id ?? crypto.randomUUID(),
    name: raw.name ?? "Track",
    type,
    color: raw.color ?? "#3b82f6",
    channelCount: raw.channelCount ?? 2,
    volume: raw.volume ?? 1,
    pan: raw.pan ?? 0,
    muted: raw.muted ?? false,
    solo: raw.solo ?? false,
    armed: raw.armed ?? false,
    clips: (raw.clips ?? []).map((c) => normalizeClip(c as Partial<DawClip>)),
    inserts,
    sends: (raw.sends ?? []).map((s) => normalizeSend(s as Partial<TrackSend>)),
    output: raw.output ?? "master",
    routing: normalizeRouting(raw.routing as Partial<TrackRouting> | undefined, type),
    advanced: normalizeAdvanced(raw.advanced as Partial<TrackAdvanced> | undefined),
    monitorMode: raw.monitorMode ?? "off",
    channelMode: raw.channelMode ?? (raw.channelCount === 1 ? "mono" : "stereo"),
    height: raw.height,
    collapsed: raw.collapsed ?? false,
    automationLanes: (raw.automationLanes ?? []).map((l) =>
      normalizeAutomationLane(l as Partial<AutomationLane>)
    ),
  };
}

export const DEFAULT_AUDIO_PROCESS: AudioClipProcess = {
  speedRatio: 1,
  pitchSemitones: 0,
  preservePitch: true,
  mode: "polyphonic",
  quality: "balanced",
};

const VALID_MODES = new Set(["resample", "monophonic", "polyphonic", "percussive", "granular"]);

function normalizeAudioProcess(raw?: Partial<AudioClipProcess>): AudioClipProcess {
  if (!raw) return DEFAULT_AUDIO_PROCESS;
  const rawMode = raw.mode as string | undefined;
  return {
    speedRatio:     Math.max(0.25, Math.min(4, raw.speedRatio ?? 1)),
    pitchSemitones: Math.max(-24, Math.min(24, raw.pitchSemitones ?? 0)),
    preservePitch:  raw.preservePitch ?? true,
    mode:           (rawMode && VALID_MODES.has(rawMode) ? rawMode : "polyphonic") as AudioClipProcess["mode"],
    quality:        raw.quality ?? "balanced",
  };
}

export function normalizeClip(raw: Partial<DawClip>): DawClip {
  const clipType = raw.type ?? (raw.fileId ? "audio" : "midi");
  return {
    id: raw.id ?? crypto.randomUUID(),
    name: raw.name ?? "Clip",
    type: clipType,
    fileId: raw.fileId ?? "",
    trackId: raw.trackId ?? "",
    startTime: raw.startTime ?? 0,
    offset: raw.offset ?? 0,
    duration: raw.duration ?? 1,
    gain: raw.gain ?? 1,
    fadeIn: raw.fadeIn ?? 0,
    fadeOut: raw.fadeOut ?? 0,
    color: raw.color,
    muted: raw.muted ?? false,
    locked: raw.locked ?? false,
    notes: raw.notes ?? [],
    audioProcess: clipType === "audio"
      ? normalizeAudioProcess(raw.audioProcess)
      : undefined,
  };
}

export function normalizeProject(raw: Partial<DawProject>): DawProject {
  return {
    id: raw.id ?? crypto.randomUUID(),
    name: raw.name ?? "Untitled Project",
    version: raw.version ?? 1,
    sampleRate: raw.sampleRate ?? 48000,
    bpm: Math.max(20, Math.min(300, raw.bpm ?? 120)),
    timeSignature: raw.timeSignature ?? { numerator: 4, denominator: 4 },
    tracks: (raw.tracks ?? []).map((t) => normalizeTrack(t as Partial<DawTrack>)),
    files: raw.files ?? [],
    masterTrackId: raw.masterTrackId,
    loop: normalizeLoop(raw.loop as Partial<ProjectLoop> | undefined),
    markers: (raw.markers ?? []).map((m) => normalizeMarker(m as Partial<ProjectMarker>)),
  };
}
