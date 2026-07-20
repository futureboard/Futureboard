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

type NativeBridge = {
  setParam?: (id: string, value: number) => void;
  setEnabled?: (stage: string, enabled: boolean) => void;
  selectModel?: (category: string, modelId: string) => void;
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

/** Whether a native host bridge is present (useful for conditional UI). */
export function hasNativeBridge(): boolean {
  const b = bridge();
  return !!(b && (b.setParam || b.setEnabled || b.selectModel));
}
