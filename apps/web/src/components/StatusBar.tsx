import { useEffect, useMemo, useRef, useState } from "react";
import { useUIStore } from "../store/uiStore";
import { useProjectStore } from "../store/projectStore";
import { buildSelectionState, getSelectionSummary } from "../store/selectionSelectors";
import { transport } from "../engine/Transport";
import { C } from "../theme";
import { formatBarBeat } from "../utils/musicalTime";
import { pxPerBeat } from "../utils/musicalGrid";
import { audioCacheManager } from "../audio/AudioCacheManager";

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
  const selectedClipIds       = useUIStore((s) => s.selectedClipIds);
  const selectedTrackId       = useUIStore((s) => s.selectedTrackId);
  const selectedBrowserFileId = useUIStore((s) => s.selectedBrowserFileId);
  const focusedPanel          = useUIStore((s) => s.focusedPanel);
  const currentTool           = useUIStore((s) => s.currentTool);
  const snapToGrid            = useUIStore((s) => s.snapToGrid);
  const pixelsPerSecond       = useUIStore((s) => s.pixelsPerSecond);
  const saveStatus            = useUIStore((s) => s.saveStatus);

  const project       = useProjectStore((s) => s.project);
  const peakCache     = useProjectStore((s) => s.peakCache);
  const bpm           = project.bpm;
  const timeSignature = project.timeSignature;

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

  // Selection summary via centralized selector — recomputed when selection or project changes.
  const selText = useMemo(() => {
    const sel = buildSelectionState({
      focusedPanel,
      selectedTrackId,
      selectedClipIds,
      selectedBrowserFileId,
    });
    return getSelectionSummary(project, sel) || "No selection";
  }, [focusedPanel, selectedTrackId, selectedClipIds, selectedBrowserFileId, project]);

  const timeSig  = timeSignature ?? { numerator: 4, denominator: 4 };
  const ppb      = Math.round(pxPerBeat(pixelsPerSecond, bpm));
  const saveSt   = saveStatus as SaveStatus;
  const fpsColor = fps >= 55 ? C.green : fps >= 30 ? C.yellow : C.red;
  const audioStats = audioCacheManager.getStats();
  const sourceBytes = project.files.reduce((sum, file) => sum + (file.size ?? 0), 0);
  const peakBytes = [...peakCache.values()].reduce((sum, peaks) => sum + peaks.peaks.byteLength, 0);
  const missingAssets = project.files.filter((file) => file.storageProvider === "missing").length;
  const audioDebug = [
    sourceBytes > 0 ? `${formatBytes(sourceBytes)} source` : null,
    audioStats.decodedBytes > 0 ? `${formatBytes(audioStats.decodedBytes)} decoded` : null,
    peakBytes > 0 ? `peaks ${formatBytes(peakBytes)}` : null,
    audioStats.processedBytes > 0 ? `processed ${formatBytes(audioStats.processedBytes)}` : null,
    missingAssets > 0 ? `${missingAssets} missing` : null,
  ].filter(Boolean).join(" · ");

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
        <span className="max-w-[36ch] truncate text-daw-faint" title={audioDebug || "Audio OK"}>
          {audioDebug || "Audio OK"}
        </span>
      </div>
    </div>
  );
}

function formatBytes(bytes: number): string {
  if (bytes >= 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)}GB`;
  if (bytes >= 1024 * 1024) return `${Math.round(bytes / (1024 * 1024))}MB`;
  return `${Math.max(1, Math.round(bytes / 1024))}KB`;
}
