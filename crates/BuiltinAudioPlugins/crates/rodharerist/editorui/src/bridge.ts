// Native bridge for the Rodhareist editor.
//
// The React UI runs inside a CEF view hosted by FutureboardNative. Parameter
// edits are forwarded to the native DSP through an injected bridge object; the
// ids here map 1:1 to the Rust `Dsp::apply_ui_param` contract in
// `rodharerist/src/dsp/mod.rs` (e.g. `drive_gain`, `amp_treble`, `chorus_mix`,
// plus stage enables `gate_on`… and model selects `amp_model`/`drive_model`).
//
// In a plain browser (e.g. `bun run dev`) there is no bridge, so every call is a
// safe no-op — the editor still works for design/preview.

export type NamCaptureLoadOptions = {
  /** Display name shown in the editor after a successful load. */
  name: string;
  /** Build two independent models (true stereo width) vs mirror one to both channels. */
  stereo: boolean;
  /** Marks the capture as already modeling amp + cab + mic, for the "Bypass Cab" action. */
  fullRig: boolean;
};

/**
 * One frame of input/output telemetry, mirroring the Rust `MeterFrame` in
 * `rodharerist/src/dsp/mod.rs`. Levels are linear 0..1 amplitudes measured
 * *after* the corresponding trim. Clip flags are sticky until
 * {@link postClearClip}.
 */
export type MeterFrame = {
  inPeak: number;
  inRms: number;
  outPeak: number;
  outRms: number;
  inClip: boolean;
  outClip: boolean;
};

/**
 * Host/engine status for the footer. Every field is optional: the editor shows
 * "—" for anything the host does not report rather than inventing a number.
 */
export type HostStatus = {
  /** Engine sample rate in Hz. */
  sampleRate?: number;
  /** Engine block size in samples. */
  blockSize?: number;
  /** Total plugin latency in samples (includes any NAM receptive field). */
  latencySamples?: number;
  /** Plugin CPU share, 0..1. */
  cpuLoad?: number;
  /** True while the engine is reporting DSP overruns. */
  overload?: boolean;
  /** Channel count the plugin is instantiated with. */
  channels?: number;
};

type NativeBridge = {
  setParam?: (id: string, value: number) => void;
  setEnabled?: (stage: string, enabled: boolean) => void;
  selectModel?: (category: string, modelId: string) => void;
  loadNamCapture?: (json: string, opts: NamCaptureLoadOptions) => void;
  /** Reset the DSP's sticky clip indicators. */
  clearClip?: () => void;
  /**
   * Register a telemetry sink. The host is expected to call `onMeters` from a
   * non-realtime timer that samples the DSP's latest frame — never from the
   * audio callback. Returns an unsubscribe function when supported.
   */
  subscribe?: (sink: {
    onMeters?: (frame: MeterFrame) => void;
    onStatus?: (status: HostStatus) => void;
  }) => (() => void) | void;
};

declare global {
  interface Window {
    rodhareist?: NativeBridge;
  }
}

function bridge(): NativeBridge | undefined {
  if (typeof window === "undefined") return undefined;
  return window.rodhareist;
}

/** Forward a continuous parameter edit (knob) to the native DSP. */
export function postParam(id: string, value: number): void {
  try {
    bridge()?.setParam?.(id, value);
  } catch {
    // Never let a bridge error break the UI.
  }
}

/** Publish the full Helix path (7 slots). Missing stages use -1. */
export function postPathOrder(path: string[]): void {
  const index: Record<string, number> = {
    dyn: 0,
    dist: 1,
    amp: 2,
    mod: 3,
    delay: 4,
    verb: 5,
    cab: 6,
    gate: 0,
    drive: 1,
  };
  for (let i = 0; i < 7; i++) {
    const cat = path[i];
    const v = cat !== undefined ? (index[cat] ?? -1) : -1;
    postParam(`path_slot_${i}`, v);
  }
}

/** Forward a per-stage bypass toggle. `stage` is a category node id (`amp`…). */
export function postEnabled(stage: string, enabled: boolean): void {
  try {
    bridge()?.setEnabled?.(stage, enabled);
  } catch {
    /* no-op */
  }
}

/** Forward a model selection within a category (`amp` → `plexi`, …). */
export function postModel(category: string, modelId: string): void {
  try {
    bridge()?.selectModel?.(category, modelId);
  } catch {
    /* no-op */
  }
}

/**
 * Load a `.nam` capture into the Tone/Amp slot's NAM engine. `json` is the
 * `.nam` file's raw text content (read client-side via `FileReader`, since
 * the editor runs sandboxed and has no filesystem path access).
 */
export function postLoadNamCapture(json: string, opts: NamCaptureLoadOptions): void {
  try {
    bridge()?.loadNamCapture?.(json, opts);
  } catch {
    /* no-op */
  }
}

/** Reset the DSP's sticky clip indicators (meter click-to-reset). */
export function postClearClip(): void {
  try {
    bridge()?.clearClip?.();
  } catch {
    /* no-op */
  }
}

/**
 * Subscribe to host telemetry. Returns an unsubscribe function, which is a
 * no-op when no host bridge is present — in that case no frames ever arrive and
 * the editor keeps showing its "no host" state.
 */
export function subscribeTelemetry(sink: {
  onMeters?: (frame: MeterFrame) => void;
  onStatus?: (status: HostStatus) => void;
}): () => void {
  try {
    const off = bridge()?.subscribe?.(sink);
    if (typeof off === "function") return off;
  } catch {
    /* no-op */
  }
  return () => {};
}

/** Whether a native host bridge is present (useful for conditional UI). */
export function hasNativeBridge(): boolean {
  const b = bridge();
  return !!(b && (b.setParam || b.setEnabled || b.selectModel));
}

/** Whether the host can actually deliver meter/status telemetry. */
export function hasTelemetry(): boolean {
  return !!bridge()?.subscribe;
}
