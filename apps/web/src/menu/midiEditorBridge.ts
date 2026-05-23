/**
 * Thin action bridge between the global menu/command system and the active
 * MidiEditorPanel instance. The panel registers callbacks on mount and clears
 * them on unmount; actionRunner calls through this bridge.
 */

export type MidiEditorActions = {
  selectAll:          () => void;
  deleteSelected:     () => void;
  duplicateSelected:  () => void;
  quantize:           () => void;
  nudgeLeft:          () => void;
  nudgeRight:         () => void;
  transposeUp:        () => void;
  transposeDown:      () => void;
  transposeOctaveUp:  () => void;
  transposeOctaveDown:() => void;
};

let _active: MidiEditorActions | null = null;

export const midiEditorBridge = {
  register(actions: MidiEditorActions) { _active = actions; },
  unregister()                         { _active = null; },
  call<K extends keyof MidiEditorActions>(action: K) {
    _active?.[action]();
  },
  isActive(): boolean { return _active !== null; },
};
