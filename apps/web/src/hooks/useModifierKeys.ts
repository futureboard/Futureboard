import { useEffect, useState } from "react";

// Module-level state — one listener set for the whole app lifetime.
let _ctrl  = false;
let _meta  = false;
let _shift = false;
let _alt   = false;

function onDown(e: KeyboardEvent) {
  if (e.key === "Control") _ctrl  = true;
  if (e.key === "Meta")    _meta  = true;
  if (e.key === "Shift")   _shift = true;
  if (e.key === "Alt")     _alt   = true;
}
function onUp(e: KeyboardEvent) {
  if (e.key === "Control") _ctrl  = false;
  if (e.key === "Meta")    _meta  = false;
  if (e.key === "Shift")   _shift = false;
  if (e.key === "Alt")     _alt   = false;
}

if (typeof window !== "undefined") {
  window.addEventListener("keydown", onDown, { capture: true });
  window.addEventListener("keyup",   onUp,   { capture: true });
  // Reset on focus loss so stuck-key state doesn't persist.
  window.addEventListener("blur", () => { _ctrl = _meta = _shift = _alt = false; });
}

export function getModifierKeys() {
  return { ctrl: _ctrl, meta: _meta, shift: _shift, alt: _alt };
}

/** Ctrl on Win/Linux, Cmd/Meta on macOS — the primary action modifier. */
export function isPrimaryModifier(e?: { ctrlKey: boolean; metaKey: boolean }): boolean {
  if (e) return e.ctrlKey || e.metaKey;
  return _ctrl || _meta;
}

/** React hook that re-renders when any modifier changes. */
export function useModifierKeys() {
  const [state, setState] = useState(getModifierKeys);
  useEffect(() => {
    const update = () => setState(getModifierKeys());
    window.addEventListener("keydown", update, { capture: true });
    window.addEventListener("keyup",   update, { capture: true });
    window.addEventListener("blur",    update);
    return () => {
      window.removeEventListener("keydown", update, { capture: true });
      window.removeEventListener("keyup",   update, { capture: true });
      window.removeEventListener("blur",    update);
    };
  }, []);
  return state;
}
