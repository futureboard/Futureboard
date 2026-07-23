// Native bridge for the Rodhareist editor.
//
// The React UI runs inside a CEF view hosted by FutureboardNative. Parameter
// edits travel over the same `__bridge` POST transport as the instance
// binding (see `instanceBridge.ts`): edits are coalesced last-value-per-id
// and flushed once per animation frame as a `futureboard.setParams` batch
// carrying the active instance binding. The ids here map 1:1 to the Rust
// `Dsp::apply_ui_param` contract in `rodharerist/src/dsp/mod.rs` (e.g.
// `drive_gain`, `amp_treble`, `chorus_mix`, plus stage enables `gate_on`…
// and numeric model selects `amp_model`/`drive_model`/`cab_model`/
// `tone_engine`); native resolves them to u32 wire indices from the shared
// `rodharerist::UI_PARAM_IDS` table.
//
// NAM capture loads, clip reset and telemetry ride the same transport:
// loads/clears go out as `__bridge` POSTs, meter/status frames come back as
// native `futureboard.meters` / `futureboard.hostStatus` postMessages (see
// `instanceBridge.ts` for the message shapes).
//
// In a plain browser (e.g. `bun run dev`) no instance is ever bound, so every
// call is a safe no-op — the editor still works for design/preview.

import {
  getActiveParamBinding,
  onNativeMessage,
  onParamBindingReset,
  postLoadNamCaptureForBoundInstance,
  postSetParams,
} from "./instanceBridge";

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

// --- Coalesced param posting -----------------------------------------------
// Knob drags emit far more edits than the DSP needs; buffer last-value-per-id
// and flush one `futureboard.setParams` batch per animation frame. Map keeps
// insertion order, so multi-id sequences (path_slot_0..6, model + reset)
// arrive at the DSP in the order they were made.

const pendingEdits = new Map<string, number>();
let flushScheduled = false;

// Edits queued against an instance must die with its binding — a new
// `selectInstance` or an `instanceRemoved` clears the buffer.
onParamBindingReset(() => {
  pendingEdits.clear();
});

function flushPendingEdits(): void {
  flushScheduled = false;
  if (pendingEdits.size === 0) return;
  const batch = Array.from(pendingEdits, ([id, value]) => ({ id, value }));
  pendingEdits.clear();
  postSetParams(batch);
}

function scheduleFlush(): void {
  if (flushScheduled) return;
  flushScheduled = true;
  if (typeof requestAnimationFrame === "function") {
    requestAnimationFrame(() => flushPendingEdits());
  } else {
    setTimeout(() => flushPendingEdits(), 16);
  }
}

/** Test-only: synchronously flush whatever is queued. */
export function __flushParamEditsForTest(): void {
  flushPendingEdits();
}

/** Forward a continuous parameter edit (knob) to the native DSP. */
export function postParam(id: string, value: number): void {
  try {
    pendingEdits.set(id, value);
    scheduleFlush();
  } catch {
    // Never let a bridge error break the UI.
  }
}

/** Publish the full Helix path (10 slots). Missing stages use -1. Values are
 * the Rust `StageKind` discriminants (comp/eq appended as 7/8, wah as 9). */
export function postPathOrder(path: string[]): void {
  const index: Record<string, number> = {
    dyn: 0,
    dist: 1,
    amp: 2,
    mod: 3,
    delay: 4,
    verb: 5,
    cab: 6,
    comp: 7,
    eq: 8,
    wah: 9,
    gate: 0,
    drive: 1,
  };
  for (let i = 0; i < 10; i++) {
    const cat = path[i];
    const v = cat !== undefined ? (index[cat] ?? -1) : -1;
    postParam(`path_slot_${i}`, v);
  }
}

/** Forward a per-stage bypass toggle. `stage` is a category node id (`amp`…). */
export function postEnabled(stage: string, enabled: boolean): void {
  // Category node ids (`gate`/`drive`/`amp`/`mod`/`delay`/`reverb`/`cab`)
  // match the Rust `*_on` param ids exactly.
  postParam(`${stage}_on`, enabled ? 1 : 0);
}

/**
 * Numeric model-select wire values. Each map mirrors the corresponding Rust
 * enum's `ALL` order (`AmpModel`/`DriveModel`/`CabModel` in
 * `rodharerist/src/dsp/mod.rs`) — pinned on the Rust side by
 * `wire::tests::model_select_wire_values_match_editor_ids`.
 */
export const AMP_MODEL_INDEX: Record<string, number> = {
  mandarin: 0,
  plexi: 1,
  twin: 2,
  topboost: 3,
  recto: 4,
  jcm: 5,
  slate: 6,
  bassman: 7,
};

export const DRIVE_MODEL_INDEX: Record<string, number> = {
  screamer: 0,
  minotaur: 1,
  rat: 2,
  breaker: 3,
  fuzz: 4,
  centurion: 5,
  ds_one: 6,
  super_drive: 7,
  metal_core: 8,
  tight_rift: 9,
};

export const CAB_MODEL_INDEX: Record<string, number> = {
  vintage_cab: 0,
  american_2x12: 1,
  tweed_1x12: 2,
  modern_412: 3,
  open_back: 4,
  vintage_212: 5,
  oversized_412: 6,
  bass_cabinet: 7,
};

/** `ModModel` indices (mirrors Rust `ModModel::ALL`). */
export const MOD_MODEL_INDEX: Record<string, number> = {
  chorus: 0,
  phaser: 1,
  flanger: 2,
  tremolo: 3,
};

/** `WahModel` indices (mirrors Rust `WahModel::ALL`). */
export const WAH_MODEL_INDEX: Record<string, number> = {
  cry_wah: 0,
  touch_wah: 1,
};

/** `ToneEngineKind` indices (Classic=0, NamCapture=1, Bypass=2). */
export const TONE_ENGINE_INDEX = {
  classic: 0,
  nam_capture: 1,
  bypass: 2,
} as const;

/** Forward a model selection within a category (`amp` → `plexi`, …). */
export function postModel(category: string, modelId: string): void {
  switch (category) {
    case "amp": {
      // The Tone/Amp slot's special engines ride the `tone_engine` param;
      // a concrete amp model implies Classic (the Rust side resets
      // `tone_engine` itself on `amp_model`).
      if (modelId === "bypass") {
        postParam("tone_engine", TONE_ENGINE_INDEX.bypass);
        return;
      }
      if (modelId === "nam_capture") {
        postParam("tone_engine", TONE_ENGINE_INDEX.nam_capture);
        return;
      }
      const i = AMP_MODEL_INDEX[modelId];
      if (i !== undefined) postParam("amp_model", i);
      return;
    }
    case "dist":
    case "drive": {
      const i = DRIVE_MODEL_INDEX[modelId];
      if (i !== undefined) postParam("drive_model", i);
      return;
    }
    case "cab": {
      const i = CAB_MODEL_INDEX[modelId];
      if (i !== undefined) postParam("cab_model", i);
      return;
    }
    case "mod": {
      const i = MOD_MODEL_INDEX[modelId];
      if (i !== undefined) postParam("mod_model", i);
      return;
    }
    case "wah": {
      const i = WAH_MODEL_INDEX[modelId];
      if (i !== undefined) postParam("wah_model", i);
      return;
    }
    default:
      // Single-algorithm stages (gate/delay/verb) have no model select.
      return;
  }
}

/**
 * Load a `.nam` capture into the Tone/Amp slot's NAM engine. `json` is the
 * `.nam` file's raw text content (read client-side via `FileReader`, since
 * the editor runs sandboxed and has no filesystem path access). Travels the
 * `__bridge` POST like params; the result arrives asynchronously as a
 * `futureboard.namCaptureResult` native message (see `instanceBridge.ts`).
 */
export function postLoadNamCapture(json: string, opts: NamCaptureLoadOptions): void {
  postLoadNamCaptureForBoundInstance(json, {
    name: opts.name,
    stereo: opts.stereo,
    fullRig: opts.fullRig,
  });
}

/** Reset the DSP's sticky clip indicators (meter click-to-reset). Routed as a
 * wire param — the DSP treats `clear_clip` as an action, not a value. */
export function postClearClip(): void {
  postParam("clear_clip", 1);
}

/**
 * Subscribe to host telemetry. Frames arrive as native `futureboard.meters` /
 * `futureboard.hostStatus` postMessages for the currently bound instance
 * (~30 Hz / ~1 Hz). Returns an unsubscribe function; with no native host the
 * listener simply never fires and the editor keeps its "no host" state.
 */
export function subscribeTelemetry(sink: {
  onMeters?: (frame: MeterFrame) => void;
  onStatus?: (status: HostStatus) => void;
}): () => void {
  return onNativeMessage((msg) => {
    const binding = getActiveParamBinding();
    if (msg.type === "futureboard.meters") {
      if (binding && msg.instanceId !== binding.instanceId) return;
      sink.onMeters?.({
        inPeak: msg.inPeak,
        inRms: msg.inRms,
        outPeak: msg.outPeak,
        outRms: msg.outRms,
        inClip: msg.inClip,
        outClip: msg.outClip,
      });
    } else if (msg.type === "futureboard.hostStatus") {
      if (binding && msg.instanceId !== binding.instanceId) return;
      sink.onStatus?.({
        sampleRate: msg.sampleRate,
        blockSize: msg.blockSize,
        latencySamples: msg.latencySamples,
        channels: 2,
      });
    }
  });
}

/** Whether a native host bridge is present (useful for conditional UI): true
 * once native has bound this page to a DSP instance. */
export function hasNativeBridge(): boolean {
  return getActiveParamBinding() !== null;
}

/** Whether the host can deliver meter/status telemetry — same condition as
 * `hasNativeBridge` now that telemetry rides the native message channel. */
export function hasTelemetry(): boolean {
  return getActiveParamBinding() !== null;
}
