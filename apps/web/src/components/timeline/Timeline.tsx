import { ZoomIn, ZoomOut } from "lucide-react";
import { TimelineGrid } from "./TimelineGrid";
import { useCallback, useEffect, useRef, useState } from "react";
import { AddTrackDialog } from "../AddTrackDialog";
import { TimelineRuler } from "./TimelineRuler";
import { TrackList } from "./TrackList";
import { Playhead } from "./Playhead";
import { FloatingToolsBar } from "./FloatingToolsBar";
import { useUIStore, type ArrangementTool } from "../../store/uiStore";
import { useProjectStore } from "../../store/projectStore";
import { secondsPerBeat, snapTime } from "../../utils/musicalTime";
import { decodeAndAddAudioFile, addFileToTimeline } from "../../utils/importAudioToProject";
import { TRACK_HEIGHT, HEADER_WIDTH } from "../../theme";

const MIN_PPS = 10;
const MAX_PPS = 800;

export function Timeline() {
  const [addTrackOpen, setAddTrackOpen] = useState(false);
  const [dropHighlight, setDropHighlight] = useState(false);
  const fileDragDepth = useRef(0);
  const { pixelsPerSecond, setPixelsPerSecond, setScrollX, snapToGrid, toggleSnapToGrid, currentTool } = useUIStore();

  const TOOL_CURSOR: Record<ArrangementTool, string> = {
    pointer:    "default",
    pen:        "crosshair",
    cut:        "crosshair",
    glue:       "copy",
    mute:       "pointer",
    time:       "ew-resize",
    automation: "crosshair",
  };
  const { tracks, bpm } = useProjectStore((s) => s.project);
  const scrollRef = useRef<HTMLDivElement>(null);

  const isFileDrag = (e: React.DragEvent) => {
    const types = [...e.dataTransfer.types];
    return types.includes("Files") || types.includes("application/x-mochi-file-id");
  };

  const resetDragState = useCallback(() => {
    fileDragDepth.current = 0;
    setDropHighlight(false);
  }, []);

  const onTimelineDragEnter = (e: React.DragEvent) => {
    if (!isFileDrag(e)) return;
    e.preventDefault();
    e.stopPropagation();
    fileDragDepth.current += 1;
    setDropHighlight(true);
  };

  const onTimelineDragLeave = (e: React.DragEvent) => {
    if (!isFileDrag(e)) return;
    e.preventDefault();
    e.stopPropagation();
    // Ignore leaves between children that still land on a descendant of the drop zone.
    const next = e.relatedTarget as Node | null;
    if (next && e.currentTarget.contains(next)) return;
    fileDragDepth.current = Math.max(0, fileDragDepth.current - 1);
    if (fileDragDepth.current === 0) setDropHighlight(false);
  };

  const onTimelineDragOver = (e: React.DragEvent) => {
    if (!isFileDrag(e)) return;
    e.preventDefault();
    e.stopPropagation();
    e.dataTransfer.dropEffect = "copy";
    if (!dropHighlight) setDropHighlight(true);
  };

  const onTimelineDrop = async (e: React.DragEvent) => {
    if (!isFileDrag(e)) {
      resetDragState();
      return;
    }
    e.preventDefault();
    e.stopPropagation();

    // Snapshot the data we need synchronously — `dataTransfer` may become invalid after await.
    const types = [...e.dataTransfer.types];
    const hasMochiFile = types.includes("application/x-mochi-file-id");
    const mochiFileId = hasMochiFile ? e.dataTransfer.getData("application/x-mochi-file-id") : "";
    const fileList: File[] = e.dataTransfer.files ? Array.from(e.dataTransfer.files) : [];
    const clientX = e.clientX;

    // Reset overlay state immediately — import is async and must never leave the overlay stuck.
    resetDragState();

    try {
      let time = 0;
      if (scrollRef.current) {
        const rect = scrollRef.current.getBoundingClientRect();
        const dropX = clientX - rect.left - HEADER_WIDTH + scrollRef.current.scrollLeft;
        time = Math.max(0, dropX / pixelsPerSecond);
        if (snapToGrid) {
          const spb = secondsPerBeat(bpm);
          time = snapTime(time, bpm, useProjectStore.getState().project.timeSignature ?? { numerator: 4, denominator: 4 }, pixelsPerSecond * spb);
        }
      }

      if (hasMochiFile) {
        const dawFile = useProjectStore.getState().project.files.find((f) => f.id === mochiFileId);
        if (dawFile) addFileToTimeline(dawFile, time);
        return;
      }

      if (!fileList.length) return;

      for (const f of fileList) {
        const dawFile = await decodeAndAddAudioFile(f);
        if (dawFile) addFileToTimeline(dawFile, time);
      }
    } finally {
      resetDragState();
    }
  };

  // ── Global safety reset ─────────────────────────────────────────────────────
  // Covers cases where React's drop/dragleave never fires:
  //  - drop handled by a descendant (e.g. TrackLane) that calls stopPropagation
  //  - drag cancelled with Escape
  //  - dropped outside the window
  //  - window loses focus
  // Use CAPTURE phase so descendants' stopPropagation cannot block these.
  useEffect(() => {
    const reset = () => resetDragState();
    window.addEventListener("dragend", reset, true);
    window.addEventListener("drop", reset, true);
    window.addEventListener("blur", reset);
    document.addEventListener("mouseleave", reset);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") reset();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("dragend", reset, true);
      window.removeEventListener("drop", reset, true);
      window.removeEventListener("blur", reset);
      document.removeEventListener("mouseleave", reset);
      window.removeEventListener("keydown", onKey);
    };
  }, [resetDragState]);

  // Keep a stable ref so the wheel handler never goes stale
  const ppsRef = useRef(pixelsPerSecond);
  ppsRef.current = pixelsPerSecond;

  // ── Ctrl/Cmd + wheel zoom (page zoom is blocked at App root) ─────────────────
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;

    const onWheel = (e: WheelEvent) => {
      if (!e.ctrlKey && !e.metaKey) return;
      e.preventDefault();

      const factor = e.deltaY < 0 ? 1.12 : 1 / 1.12;
      const oldPPS = ppsRef.current;
      const newPPS = Math.min(MAX_PPS, Math.max(MIN_PPS, oldPPS * factor));

      // Anchor zoom to cursor: keep the time-position under the pointer fixed
      const cursorX    = e.offsetX;
      const timeAtCursor = (el.scrollLeft + cursorX) / oldPPS;

      setPixelsPerSecond(newPPS);

      requestAnimationFrame(() => {
        el.scrollLeft = Math.max(0, timeAtCursor * newPPS - cursorX);
      });
    };

    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [setPixelsPerSecond]);

  // ── zoom buttons ─────────────────────────────────────────────────────────────
  const zoom = (f: number) => {
    const newPPS = Math.min(MAX_PPS, Math.max(MIN_PPS, pixelsPerSecond * f));
    setPixelsPerSecond(newPPS);
  };

  const pixelsPerBeat = pixelsPerSecond * secondsPerBeat(bpm);
  const timelineSeconds = Math.max(
    16,
    ...tracks.flatMap((t) => t.clips.map((c) => c.startTime + c.duration + 4))
  );
  const timelineWidth = Math.max(1200, Math.ceil(timelineSeconds * pixelsPerSecond));
  const trackContentHeight = Math.max(0, tracks.length * TRACK_HEIGHT);

  return (
    <div
      className="relative flex flex-1 flex-col overflow-hidden border border-daw-border bg-daw-sunken shadow-[0_8px_24px_rgba(0,0,0,0.18)]"
      onDragEnter={onTimelineDragEnter}
      onDragLeave={onTimelineDragLeave}
      onDragOver={onTimelineDragOver}
      onDrop={onTimelineDrop}
    >
      {dropHighlight && (
        <div
          className="pointer-events-none absolute inset-0 z-50 flex items-center justify-center border-2 border-dashed border-daw-accent/80 bg-daw-accent/[0.07]"
          aria-hidden
        >
          <span className="rounded-md border border-daw-accent/40 bg-daw-surface/90 px-3 py-2 text-[11px] font-semibold text-daw-accent shadow-lg">
            Drop audio to create new tracks
          </span>
        </div>
      )}

      <TimelineRuler
        width={timelineWidth}
        onAddTrack={() => setAddTrackOpen(true)}
        snapToGrid={snapToGrid}
        onToggleSnapToGrid={toggleSnapToGrid}
      />

      <div className="relative flex-1 overflow-hidden bg-daw-bg">
        <FloatingToolsBar />

        <div className="absolute inset-0 z-0 pointer-events-none">
          <TimelineGrid />
        </div>

        {/* scrollable track area — ctrl/cmd+wheel handled via non-passive listener */}
        <div
          ref={scrollRef}
          className="absolute inset-0 z-10 overflow-auto"
          style={{ cursor: TOOL_CURSOR[currentTool] }}
          onScroll={(e) => setScrollX(e.currentTarget.scrollLeft)}
        >
          <TrackList timelineWidth={timelineWidth} />
          <Playhead height={trackContentHeight} />
        </div>
      </div>

      {addTrackOpen && <AddTrackDialog onClose={() => setAddTrackOpen(false)} />}

      {/* zoom controls */}
      <div className="absolute bottom-4 right-4 z-30 flex items-center gap-1 rounded-full border border-daw-border bg-daw-surface px-2 py-1.5 shadow-xl">
        <button
          onClick={() => zoom(0.75)}
          title="Zoom out [−]"
          className="flex h-7 w-7 items-center justify-center rounded-lg bg-transparent text-daw-faint transition-colors hover:bg-daw-surface-high hover:text-daw-text"
        >
          <ZoomOut size={12} />
        </button>
        <span className="min-w-12 text-center text-[9px] tabular-nums text-daw-dim">
          {Math.round(pixelsPerBeat)} px/bt
        </span>
        <button
          onClick={() => zoom(1.33)}
          title="Zoom in [+]"
          className="flex h-7 w-7 items-center justify-center rounded-lg bg-transparent text-daw-faint transition-colors hover:bg-daw-surface-high hover:text-daw-text"
        >
          <ZoomIn size={12} />
        </button>
      </div>
    </div>
  );
}
