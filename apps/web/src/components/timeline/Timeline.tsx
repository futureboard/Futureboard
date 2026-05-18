import { ZoomIn, ZoomOut } from "lucide-react";
import { TimelineGrid } from "./TimelineGrid";
import { useCallback, useEffect, useRef, useState } from "react";
import { AddTrackDialog } from "../AddTrackDialog";
import { TimelineRuler } from "./TimelineRuler";
import { TrackList } from "./TrackList";
import { Playhead } from "./Playhead";
import { FloatingToolsBar } from "./FloatingToolsBar";
import { useUIStore, type ArrangementTool, type MarqueeSelectionState } from "../../store/uiStore";
import { useProjectStore } from "../../store/projectStore";
import { isPrimaryModifier } from "../../hooks/useModifierKeys";
import { secondsPerBeat, snapTime, timelineXToTime } from "../../utils/musicalTime";
import { activeAudioEngine } from "../../engine/activeAudioEngine";
import { addFileToTimeline, importNativeAudioPathToTimeline } from "../../utils/importAudioToProject";
import { audioImportQueue } from "../../engine/AudioImportQueue";
import { TIMELINE_Z } from "../../utils/timelineZ";
import { HEADER_WIDTH } from "../../theme";

// Zoom range: 4 px/s lets you see ~250 bars in a typical viewport at 120 BPM.
// 4000 px/s lets you inspect individual samples with 1/32-note subdivisions visible.
const MIN_PPS = 4;
const MAX_PPS = 4000;
const NATIVE_AUDIO_DRAG_TYPE = "application/x-futureboard-native-audio-path";

export function Timeline() {
  const [addTrackOpen, setAddTrackOpen] = useState(false);
  const [dropHighlight, setDropHighlight] = useState(false);
  const fileDragDepth = useRef(0);
  const { pixelsPerSecond, setPixelsPerSecond, setScrollX, setScrollY, setTrackAreaHeight, snapToGrid, toggleSnapToGrid, currentTool, marqueeSelection, setMarqueeSelection } = useUIStore();

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
  const timelineRef = useRef<HTMLDivElement>(null);

  const isFileDrag = (e: React.DragEvent) => {
    const types = [...e.dataTransfer.types];
    return types.includes("Files") || types.includes("application/x-mochi-file-id") || types.includes(NATIVE_AUDIO_DRAG_TYPE);
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
    const hasNativeAudio = types.includes(NATIVE_AUDIO_DRAG_TYPE);
    const mochiFileId = hasMochiFile ? e.dataTransfer.getData("application/x-mochi-file-id") : "";
    const nativeAudioPath = hasNativeAudio ? e.dataTransfer.getData(NATIVE_AUDIO_DRAG_TYPE) : "";
    const fileList: File[] = e.dataTransfer.files ? Array.from(e.dataTransfer.files) : [];
    const clientX = e.clientX;

    // Reset overlay state immediately — import is async and must never leave the overlay stuck.
    resetDragState();

    try {
      let time = 0;
      if (scrollRef.current) {
        const rect = scrollRef.current.getBoundingClientRect();
        // The scroll container's left edge IS the outer timeline content origin
        // (it spans full width including the sticky header lane), so
        // timelineXToTime — which subtracts TIMELINE_CONTENT_LEFT internally —
        // applies the same origin used by the playhead, ruler, and grid.
        time = timelineXToTime(clientX - rect.left, pixelsPerSecond, scrollRef.current.scrollLeft);
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

      if (nativeAudioPath) {
        await importNativeAudioPathToTimeline(nativeAudioPath, time);
        return;
      }

      if (!fileList.length) return;
      audioImportQueue.enqueueFiles(fileList, { startTime: time });
    } finally {
      resetDragState();
    }
  };

  // ── Marquee Selection Gesture ───────────────────────────────────────────────
  const possibleMarquee = useRef<{ x: number; y: number; initialSelection: string[] } | null>(null);

  const onPointerDown = (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    if (currentTool !== "pointer") return;
    if (!isPrimaryModifier(e)) return;

    possibleMarquee.current = {
      x: e.clientX,
      y: e.clientY,
      initialSelection: e.shiftKey ? useUIStore.getState().selectedClipIds : [],
    };
  };

  const onPointerMove = (e: React.PointerEvent) => {
    if (possibleMarquee.current && !marqueeSelection) {
      const dx = e.clientX - possibleMarquee.current.x;
      const dy = e.clientY - possibleMarquee.current.y;
      if (Math.sqrt(dx * dx + dy * dy) > 4) {
        setMarqueeSelection({
          active: true,
          pointerId: e.pointerId,
          startClientX: possibleMarquee.current.x,
          startClientY: possibleMarquee.current.y,
          currentClientX: e.clientX,
          currentClientY: e.clientY,
          rect: { x: 0, y: 0, width: 0, height: 0, left: 0, top: 0, right: 0, bottom: 0 },
          affectedClipIds: [...possibleMarquee.current.initialSelection],
          affectedTrackIds: [],
        });
        e.currentTarget.setPointerCapture(e.pointerId);
      }
    }

    if (marqueeSelection && possibleMarquee.current) {
      const startX = marqueeSelection.startClientX;
      const startY = marqueeSelection.startClientY;
      const currentX = e.clientX;
      const currentY = e.clientY;

      const rect = {
        left: Math.min(startX, currentX),
        top: Math.min(startY, currentY),
        right: Math.max(startX, currentX),
        bottom: Math.max(startY, currentY),
        width: Math.abs(currentX - startX),
        height: Math.abs(currentY - startY),
        x: Math.min(startX, currentX),
        y: Math.min(startY, currentY),
      };

      // Hit test all clips in the DOM
      const intersectingClipIds = new Set<string>();
      const clipElements = document.querySelectorAll("[data-clip-id]");

      for (let i = 0; i < clipElements.length; i++) {
        const el = clipElements[i] as HTMLElement;
        const elRect = el.getBoundingClientRect();
        
        const intersects =
          rect.left < elRect.right &&
          rect.right > elRect.left &&
          rect.top < elRect.bottom &&
          rect.bottom > elRect.top;

        if (intersects) {
          const id = el.getAttribute("data-clip-id");
          if (id) intersectingClipIds.add(id);
        }
      }

      const newSelection = new Set([
        ...possibleMarquee.current.initialSelection,
        ...Array.from(intersectingClipIds)
      ]);

      setMarqueeSelection({
        ...marqueeSelection,
        currentClientX: currentX,
        currentClientY: currentY,
        rect,
        affectedClipIds: Array.from(newSelection),
      });

      // Visually update the selection during drag without committing
      useUIStore.getState().setSelectedClipIds(Array.from(newSelection));
    }
  };

  const onPointerUp = (e: React.PointerEvent) => {
    if (marqueeSelection) {
      // The selection is already updated in the store during onPointerMove.
      setMarqueeSelection(null);
      e.currentTarget.releasePointerCapture(e.pointerId);
    }
    possibleMarquee.current = null;
  };

  const onPointerCancel = () => {
    if (marqueeSelection && possibleMarquee.current) {
      // Revert selection if cancelled
      useUIStore.getState().setSelectedClipIds(possibleMarquee.current.initialSelection);
    }
    possibleMarquee.current = null;
    setMarqueeSelection(null);
  };

  // ── Global safety reset ─────────────────────────────────────────────────────
  // Covers cases where React's drop/dragleave never fires:
  //  - drop handled by a descendant (e.g. TrackLane) that calls stopPropagation
  //  - drag cancelled with Escape
  //  - dropped outside the window
  //  - window loses focus
  // Use CAPTURE phase so descendants' stopPropagation cannot block these.
  useEffect(() => {
    const reset = () => {
      resetDragState();
      setMarqueeSelection(null);
      possibleMarquee.current = null;
    };
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
  }, [resetDragState, setMarqueeSelection]);

  // Track the scroll container height for vertical virtualization
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setTrackAreaHeight(el.clientHeight));
    ro.observe(el);
    setTrackAreaHeight(el.clientHeight);
    return () => ro.disconnect();
  }, [setTrackAreaHeight]);

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

      // Use a slightly smaller factor for smoother, more controlled zooming.
      const factor = e.deltaY < 0 ? 1.10 : 1 / 1.10;
      const oldPPS = ppsRef.current;
      const newPPS = Math.min(MAX_PPS, Math.max(MIN_PPS, oldPPS * factor));
      if (newPPS === oldPPS) return;

      // Anchor zoom to the cursor position.
      // Use clientX relative to the scroll container's bounding rect rather than
      // offsetX (which would be relative to whichever child element is under the
      // cursor, giving incorrect results when hovering over clips or track headers).
      const rect = el.getBoundingClientRect();
      const contentX = Math.max(0, e.clientX - rect.left - HEADER_WIDTH);
      const timeAtCursor = (el.scrollLeft + contentX) / oldPPS;

      setPixelsPerSecond(newPPS);

      requestAnimationFrame(() => {
        el.scrollLeft = Math.max(0, timeAtCursor * newPPS - contentX);
      });
    };

    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [setPixelsPerSecond]);

  // ── zoom buttons ─────────────────────────────────────────────────────────────
  // Anchor priority: playhead (if visible) > viewport center.
  const zoom = (f: number) => {
    const oldPPS = pixelsPerSecond;
    const newPPS = Math.min(MAX_PPS, Math.max(MIN_PPS, oldPPS * f));
    if (newPPS === oldPPS) return;

    const el = scrollRef.current;
    if (!el) { setPixelsPerSecond(newPPS); return; }

    const contentW   = el.clientWidth - HEADER_WIDTH;
    const scrollLeft = el.scrollLeft;

    // Playhead content-space position at the current zoom
    const playheadSec  = activeAudioEngine.projectTime;
    const playheadAbsX = playheadSec * oldPPS;               // absolute content x
    const playheadViewX = playheadAbsX - scrollLeft;         // x within viewport

    // Use playhead as anchor if it is currently visible inside the content area
    const playheadVisible = playheadViewX >= 0 && playheadViewX <= contentW;
    const anchorAbsX   = playheadVisible ? playheadAbsX  : scrollLeft + contentW / 2;
    const anchorOffsetX = playheadVisible ? playheadViewX : contentW / 2;

    setPixelsPerSecond(newPPS);
    requestAnimationFrame(() => {
      el.scrollLeft = Math.max(0, (anchorAbsX / oldPPS) * newPPS - anchorOffsetX);
    });
  };

  const pixelsPerBeat = pixelsPerSecond * secondsPerBeat(bpm);
  const timelineSeconds = Math.max(
    16,
    ...tracks.flatMap((t) => t.clips.map((c) => c.startTime + c.duration + 4))
  );
  const timelineWidth = Math.max(1200, Math.ceil(timelineSeconds * pixelsPerSecond));

  return (
    <div
      ref={timelineRef}
      className="relative flex flex-1 flex-col overflow-hidden border border-daw-border bg-daw-sunken shadow-[0_8px_24px_rgba(0,0,0,0.18)]"
      onDragEnter={onTimelineDragEnter}
      onDragLeave={onTimelineDragLeave}
      onDragOver={onTimelineDragOver}
      onDrop={onTimelineDrop}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerCancel}
    >
      {dropHighlight && (
        <div
          className="pointer-events-none absolute inset-0 flex items-center justify-center border-2 border-dashed border-daw-accent/80 bg-daw-accent/[0.07]"
          style={{ zIndex: TIMELINE_Z.modal }}
          aria-hidden
        >
          <span className="rounded-md border border-daw-accent/40 bg-daw-surface/90 px-3 py-2 text-[11px] font-semibold text-daw-accent shadow-lg">
            Drop audio to create new tracks
          </span>
        </div>
      )}

      {marqueeSelection && <MarqueeSelectionOverlay state={marqueeSelection} containerRect={timelineRef.current?.getBoundingClientRect() || null} />}

      <TimelineRuler
        width={timelineWidth}
        onAddTrack={() => setAddTrackOpen(true)}
        snapToGrid={snapToGrid}
        onToggleSnapToGrid={toggleSnapToGrid}
      />

      <div className="relative flex-1 overflow-hidden bg-daw-bg">
        <FloatingToolsBar />

        <div className="absolute inset-0 pointer-events-none" style={{ zIndex: TIMELINE_Z.grid }}>
          <TimelineGrid />
        </div>

        {/* scrollable track area — ctrl/cmd+wheel handled via non-passive listener */}
        <div
          ref={scrollRef}
          className="absolute inset-0 overflow-auto"
          style={{ cursor: TOOL_CURSOR[currentTool], zIndex: TIMELINE_Z.scrollArea }}
          onScroll={(e) => {
            setScrollX(e.currentTarget.scrollLeft);
            setScrollY(e.currentTarget.scrollTop);
          }}
        >
          <TrackList timelineWidth={timelineWidth} />
        </div>
      </div>

      {/* Playhead spans the full height of this container (ruler + all track rows).
          Positioned relative to the outer Timeline div so the triangle marker
          sits correctly in the ruler area. */}
      <Playhead />

      {addTrackOpen && <AddTrackDialog onClose={() => setAddTrackOpen(false)} />}

      {/* zoom controls */}
      <div
        className="absolute bottom-4 right-4 flex items-center gap-1 rounded-full border border-daw-border bg-daw-surface px-2 py-1.5 shadow-xl"
        style={{ zIndex: TIMELINE_Z.zoomControls }}
      >
        <button
          onClick={() => zoom(0.75)}
          title="Zoom out [−]"
          className="flex h-7 w-7 items-center justify-center rounded-lg bg-transparent text-daw-faint transition-colors hover:bg-daw-surface-high hover:text-daw-text"
        >
          <ZoomOut size={12} />
        </button>
        <span className="min-w-[52px] text-center text-[9px] tabular-nums text-daw-dim">
          {pixelsPerBeat >= 10
            ? `${Math.round(pixelsPerBeat)} px/bt`
            : `${pixelsPerBeat.toFixed(1)} px/bt`}
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

function MarqueeSelectionOverlay({ state, containerRect }: { state: MarqueeSelectionState, containerRect: DOMRect | null }) {
  if (!state.rect || !containerRect) return null;

  return (
    <div
      className="pointer-events-none absolute border border-cyan-400 bg-cyan-400/20 shadow-[0_0_12px_rgba(34,211,238,0.2)]"
      style={{
        left: state.rect.left - containerRect.left,
        top: state.rect.top - containerRect.top,
        width: state.rect.width,
        height: state.rect.height,
        zIndex: TIMELINE_Z.modal,
      }}
    >
      <div className="absolute top-0 left-0 rounded-br-sm bg-cyan-500/80 px-1 py-[1px] text-[8px] font-bold text-black uppercase">
        Select
      </div>
    </div>
  );
}
