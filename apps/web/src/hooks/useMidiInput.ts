import { useEffect, useState } from "react";
import type { DawTrack } from "../types/daw";
import { midiDeviceService } from "../engine/MidiDeviceService";
import { useDeviceStore } from "../store/deviceStore";

// ── Note name table ───────────────────────────────────────────────────────────

const NOTE_NAMES = ["C","C#","D","D#","E","F","F#","G","G#","A","A#","B"] as const;

function noteName(n: number): string {
  return NOTE_NAMES[n % 12]! + (Math.floor(n / 12) - 1);
}

// ── Event log type ────────────────────────────────────────────────────────────

export type MidiEventType = "note-on" | "note-off" | "cc" | "pc" | "pitch" | "other";

export type MidiEventLog = {
  id: number;
  ts: number;
  type: MidiEventType;
  channel: number;
  label: string;
};

let _eventId = 0;

function parseMidiEvent(e: MIDIMessageEvent): MidiEventLog | null {
  const data = e.data;
  if (!data || data.length === 0) return null;

  const status = data[0]!;
  const cmd    = status & 0xf0;
  const ch     = (status & 0x0f) + 1;
  const d1     = data[1] ?? 0;
  const d2     = data[2] ?? 0;

  // Suppress MIDI clock/tick (0xF8) and active-sense (0xFE) — far too frequent.
  if (status === 0xf8 || status === 0xfe) return null;

  let type: MidiEventType = "other";
  let label = "";

  if (cmd === 0x90 && d2 > 0) {
    type  = "note-on";
    label = `${noteName(d1)}  vel ${d2}`;
  } else if (cmd === 0x80 || (cmd === 0x90 && d2 === 0)) {
    type  = "note-off";
    label = noteName(d1);
  } else if (cmd === 0xb0) {
    type  = "cc";
    label = `CC ${d1}  →  ${d2}`;
  } else if (cmd === 0xc0) {
    type  = "pc";
    label = `Prog ${d1}`;
  } else if (cmd === 0xe0) {
    type  = "pitch";
    const val = ((d2 << 7) | d1) - 8192;
    label = `Pitch ${val >= 0 ? "+" : ""}${val}`;
  } else {
    label = Array.from(data).map((b) => b.toString(16).padStart(2, "0")).join(" ");
  }

  return { id: ++_eventId, ts: Date.now(), type, channel: ch, label };
}

// ── Hook ──────────────────────────────────────────────────────────────────────

/**
 * Subscribe to MIDI input events for a track.
 * Active only when the track is armed and monitorMode is "auto" or "in".
 * Automatically re-subscribes when the device list changes.
 */
export function useMidiInput(track: DawTrack | null): {
  events: MidiEventLog[];
  isListening: boolean;
  clearEvents: () => void;
} {
  const [events, setEvents] = useState<MidiEventLog[]>([]);
  const midiInputs = useDeviceStore((s) => s.midiInputs);

  const trackId    = track?.id;
  const trackType  = track?.type;
  const armed      = track?.armed ?? false;
  const monitor    = track?.monitorMode ?? "off";
  const inputId    = track?.routing?.inputId;

  const isListening =
    !!trackId &&
    trackType === "midi" &&
    armed &&
    (monitor === "auto" || monitor === "in");

  useEffect(() => {
    if (!isListening || !trackId) return;

    const handler = (e: MIDIMessageEvent) => {
      const log = parseMidiEvent(e);
      if (log) setEvents((prev) => [log, ...prev.slice(0, 29)]);
    };

    const unsubs: Array<() => void> = [];

    if (!inputId) {
      // "All MIDI Inputs" — subscribe to every connected device.
      for (const dev of midiInputs) {
        unsubs.push(midiDeviceService.subscribeMidiMessages(trackId, dev.id, handler));
      }
    } else {
      unsubs.push(midiDeviceService.subscribeMidiMessages(trackId, inputId, handler));
    }

    return () => {
      for (const unsub of unsubs) unsub();
    };
    // midiInputs identity changes when devices connect/disconnect → resubscribe.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isListening, trackId, inputId, midiInputs]);

  return { events, isListening, clearEvents: () => setEvents([]) };
}
