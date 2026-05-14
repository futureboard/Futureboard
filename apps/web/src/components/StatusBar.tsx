import { useEffect, useMemo, useRef, useState } from "react";
import { useUIStore } from "../store/uiStore";
import { useProjectStore } from "../store/projectStore";
import { transport } from "../engine/Transport";
import { C } from "../theme";
import { formatBarBeat } from "../utils/musicalTime";
import { pxPerBeat } from "../utils/musicalGrid";

// ── Save status ───────────────────────────────────────────────────────────────
type SaveStatus = "saved" | "unsaved" | "saving" | "error";

const SAVE_COLOR: Record<SaveStatus, string> = {
  saved:   C.green,
  unsaved: C.yellow,
  saving:  C.accent,
  error:   C.red,
};
const SAVE_LABEL: Record<SaveStatus, string> = {
  saved:   "Saved",
  unsaved: "Unsaved",
  saving:  "Saving…",
  error:   "Save failed",
};

// ── Tool display names ────────────────────────────────────────────────────────
const TOOL_LABEL: Record<string, string> = {
  pointer:    "Pointer",
  pen:        "Pen",
  cut:        "Cut",
  glue:       "Glue",
  mute:       "Mute",
  time:       "Stretch",
  automation: "Auto",
};

// ── Small helpers ─────────────────────────────────────────────────────────────
function Dot({ color }: { color: string }) {
  return (
    <span
      aria-hidden
      style={{
        display: "inline-block",
        width: 5, height: 5,
        borderRadius: "50%",
        background: color,
        flexShrink: 0,
        marginBottom: 1,
      }}
    />
  );
}

function Sep() {
  return <span className="mx-[5px]" style={{ color: "rgba(255,255,255,0.12)" }} aria-hidden>·</span>;
}

// ── StatusBar ─────────────────────────────────────────────────────────────────
export function StatusBar() {
  // Store subscriptions — individual selectors avoid broad re-renders
  const selectedClipIds = useUIStore((s) => s.selectedClipIds);
  const selectedTrackId = useUIStore((s) => s.selectedTrackId);
  const currentTool     = useUIStore((s) => s.currentTool);
  const snapToGrid      = useUIStore((s) => s.snapToGrid);
  const pixelsPerSecond = useUIStore((s) => s.pixelsPerSecond);
  const saveStatus      = useUIStore((s) => s.saveStatus);

  const tracks       = useProjectStore((s) => s.project.tracks);
  const bpm          = useProjectStore((s) => s.project.bpm);
  const timeSignature = useProjectStore((s) => s.project.timeSignature);

  // High-frequency values live in local state, updated by a throttled RAF loop
  // so they never go through Zustand and don't trigger global re-renders.
  const [pos,   setPos]   = useState("1.1");
  const [fps,   setFps]   = useState(60);
  const [memMB, setMemMB] = useState<number | null>(null);

  // Refs let the RAF closure read current bpm/timeSig without restarting
  const bpmRef    = useRef(bpm);
  const timeSigRef = useRef(timeSignature);
  bpmRef.current   = bpm;
  timeSigRef.current = timeSignature;

  useEffect(() => {
    let frames    = 0;
    let lastFlush = performance.now();
    let raf: number;

    const tick = (now: number) => {
      frames++;
      if (now - lastFlush >= 250) {        // ~4 Hz refresh
        const elapsed = now - lastFlush;
        setFps(Math.round((frames * 1000) / elapsed));
        setPos(
          formatBarBeat(
            transport.projectTime,
            bpmRef.current,
            timeSigRef.current ?? { numerator: 4, denominator: 4 },
          ),
        );
        // performance.memory is Chrome/Electron-only; cast safely
        const perf = performance as { memory?: { usedJSHeapSize: number } };
        if (perf.memory) setMemMB(perf.memory.usedJSHeapSize / 1_048_576);
        frames    = 0;
        lastFlush = now;
      }
      raf = requestAnimationFrame(tick);
    };

    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, []); // empty deps: bpm/timeSig are read via refs

  // Selection summary — recomputed only when selection or tracks change
  const selText = useMemo(() => {
    const n = selectedClipIds.length;
    if (n > 1) return `${n} clips`;
    if (n === 1) {
      const clip = tracks.flatMap((t) => t.clips).find((c) => c.id === selectedClipIds[0]);
      if (clip) return `${clip.type === "midi" ? "MIDI" : "Audio"}: ${clip.name}`;
    }
    if (selectedTrackId) {
      const track = tracks.find((t) => t.id === selectedTrackId);
      if (track) return track.name;
    }
    return "No selection";
  }, [selectedClipIds, selectedTrackId, tracks]);

  const timeSig  = timeSignature ?? { numerator: 4, denominator: 4 };
  const ppb      = Math.round(pxPerBeat(pixelsPerSecond, bpm));
  const saveSt   = saveStatus as SaveStatus;
  const fpsColor = fps >= 55 ? C.green : fps >= 30 ? C.yellow : C.red;

  return (
    <div
      className="flex h-[22px] shrink-0 select-none items-center justify-between overflow-hidden border-t border-daw-border px-3 text-[10px]"
      style={{ background: C.sunken }}
    >
      {/* ── Left: Save · Selection · Tool ── */}
      <div className="flex min-w-0 shrink items-center">
        <span className="flex items-center gap-1 text-daw-dim">
          <Dot color={SAVE_COLOR[saveSt]} />
          {SAVE_LABEL[saveSt]}
        </span>
        <Sep />
        <span className="max-w-[18ch] truncate text-daw-dim" title={selText}>
          {selText}
        </span>
        <Sep />
        <span className="text-daw-faint">
          {TOOL_LABEL[currentTool] ?? currentTool}
        </span>
      </div>

      {/* ── Center: Position · BPM · TimeSig · Snap · Zoom ── */}
      <div className="hidden shrink-0 items-center lg:flex">
        <span className="tabular-nums text-daw-text">{pos}</span>
        <Sep />
        <span className="tabular-nums text-daw-dim">{bpm} BPM</span>
        <Sep />
        <span className="tabular-nums text-daw-dim">
          {timeSig.numerator}/{timeSig.denominator}
        </span>
        <Sep />
        <span
          title={snapToGrid ? "Snap to grid: ON" : "Snap to grid: OFF"}
          style={{ color: snapToGrid ? C.accent : C.faint }}
        >
          {snapToGrid ? "Snap" : "Free"}
        </span>
        <Sep />
        <span className="tabular-nums text-daw-faint">{ppb} px/bt</span>
      </div>

      {/* ── Right: FPS · Memory · Audio ── */}
      <div className="flex shrink-0 items-center">
        <span
          className="tabular-nums"
          style={{ color: fpsColor }}
          title="UI frames per second"
        >
          {fps} fps
        </span>
        {memMB !== null && (
          <>
            <Sep />
            <span className="tabular-nums text-daw-faint" title="JS heap usage">
              {memMB >= 1024
                ? `${(memMB / 1024).toFixed(1)} GB`
                : `${Math.round(memMB)} MB`}
            </span>
          </>
        )}
        <Sep />
        <span className="text-daw-faint">Audio OK</span>
      </div>
    </div>
  );
}
