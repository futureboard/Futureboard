/**
 * MidiDeviceService — Web MIDI API device discovery and message routing.
 *
 * Gracefully handles:
 * - Browser without Web MIDI support     → permission = "unsupported"
 * - User denies MIDI access              → permission = "denied"
 * - No devices connected                 → empty lists, no crash
 *
 * Usage:
 *   await midiDeviceService.requestMidiAccess();
 *   midiDeviceService.getMidiInputs();
 *   const unsub = midiDeviceService.subscribeMidiMessages("track-1", inputId, handler);
 *   unsub(); // clean up
 */
import { useDeviceStore } from "../store/deviceStore";
import type { MidiDeviceInfo, MidiPermissionState } from "../store/deviceStore";

export type MidiMessageHandler = (event: MIDIMessageEvent) => void;

function store() {
  return useDeviceStore.getState();
}

class MidiDeviceService {
  private _access: MIDIAccess | null = null;

  /** Key: `${trackId}:${inputId}`, Value: handler registered on the MIDIInput */
  private _subscriptions = new Map<string, { input: MIDIInput; handler: MidiMessageHandler; wrapped: EventListener }>();

  /**
   * Request Web MIDI access. Call once at app init or on first user gesture.
   * Returns the resolved permission state.
   */
  async requestMidiAccess(): Promise<MidiPermissionState> {
    if (!navigator.requestMIDIAccess) {
      store().setMidiPermission("unsupported");
      return "unsupported";
    }

    store().setMidiPermission("prompting");
    try {
      const access = await navigator.requestMIDIAccess({ sysex: false });
      this._access = access;
      store().setMidiPermission("granted");
      this.refreshMidiDevices();
      // Re-enumerate whenever devices connect/disconnect.
      access.onstatechange = () => this.refreshMidiDevices();
      return "granted";
    } catch {
      store().setMidiPermission("denied");
      return "denied";
    }
  }

  /** Sync current MIDI device list into the store. */
  refreshMidiDevices(): void {
    if (!this._access) {
      store().setMidiDevices([], []);
      return;
    }

    const inputs: MidiDeviceInfo[] = [];
    this._access.inputs.forEach((input) => {
      inputs.push({
        id: input.id,
        name: input.name || `MIDI Input ${input.id}`,
        kind: "input",
        state: input.state,
      });
    });

    const outputs: MidiDeviceInfo[] = [];
    this._access.outputs.forEach((output) => {
      outputs.push({
        id: output.id,
        name: output.name || `MIDI Output ${output.id}`,
        kind: "output",
        state: output.state,
      });
    });

    store().setMidiDevices(inputs, outputs);
  }

  // ── Accessors (snapshots from the store) ──────────────────────────────────

  getMidiInputs(): MidiDeviceInfo[] {
    return store().midiInputs;
  }

  getMidiOutputs(): MidiDeviceInfo[] {
    return store().midiOutputs;
  }

  /**
   * Subscribe a handler to raw MIDI messages from a specific input device.
   *
   * - Safe to call before `requestMidiAccess` — returns a no-op unsubscribe.
   * - The `trackId` scopes the subscription so the same input can have
   *   multiple per-track handlers without conflict.
   *
   * Returns an unsubscribe function.
   */
  subscribeMidiMessages(
    trackId: string,
    inputId: string,
    handler: MidiMessageHandler
  ): () => void {
    if (!this._access) return () => {};

    const input = this._access.inputs.get(inputId);
    if (!input) return () => {};

    const key = `${trackId}:${inputId}`;
    // Remove old subscription for this track+input if it exists.
    this._removeSubscription(key);

    // Wrap handler so we hold a stable reference for removeEventListener.
    const wrapped: EventListener = (e) => handler(e as MIDIMessageEvent);
    input.addEventListener("midimessage", wrapped);
    this._subscriptions.set(key, { input, handler, wrapped });

    return () => this._removeSubscription(key);
  }

  private _removeSubscription(key: string): void {
    const existing = this._subscriptions.get(key);
    if (existing) {
      existing.input.removeEventListener("midimessage", existing.wrapped);
      this._subscriptions.delete(key);
    }
  }

  /** Remove all active MIDI message subscriptions. */
  unsubscribeAll(): void {
    this._subscriptions.forEach((_, key) => this._removeSubscription(key));
  }

  /** Whether the browser supports Web MIDI at all. */
  get isSupported(): boolean {
    return typeof navigator !== "undefined" && "requestMIDIAccess" in navigator;
  }

  /** Whether MIDI access has been successfully granted. */
  get isReady(): boolean {
    return this._access !== null;
  }
}

export const midiDeviceService = new MidiDeviceService();
