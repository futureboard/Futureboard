import { useRef, useState } from "react";
import type { DawClip, DawTrack } from "../../types/daw";
import { useProjectStore } from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
import { useHistoryStore } from "../../store/historyStore";
import { MoveClipCommand, ResizeClipCommand } from "../../commands";
import { WaveformCanvas } from "./WaveformCanvas";
import { TRACK_HEIGHT } from "../../theme";
import { formatBeatLength, secondsPerBeat, snapTime } from "../../utils/musicalTime";

const LABEL_H = 14;
const PAD = 7;

function hex2rgba(hex: string, a: number) {
  const r = parseInt(hex.slice(1, 3), 16);
  const g = parseInt(hex.slice(3, 5), 16);
  const b = parseInt(hex.slice(5, 7), 16);
  return `rgba(${r},${g},${b},${a})`;
}

type Props = {
  clip: DawClip;
  track: DawTrack;
  trackIndex: number;
  allTracks: DawTrack[];
};

export function AudioClip({ clip, track, trackIndex, allTracks }: Props) {
  const {
    pixelsPerSecond,
    selectedClipIds, setSelectedClipIds, toggleClipSelection,
    setSelectedTrackId, setFocusedPanel, setDraggingClipTargetIdx,
  } = useUIStore();
  const { peakCache, moveClip, moveClipToTrack, project } = useProjectStore();
  const peaks = peakCache.get(clip.fileId);

  const dragStartX    = useRef(0);
  const dragStartY    = useRef(0);
  const dragStartTime = useRef(0);
  const [dragging, setDragging] = useState(false);

  const left  = clip.startTime * pixelsPerSecond;
  const width = Math.max(4, clip.duration * pixelsPerSecond);
  const clipH = TRACK_HEIGHT - PAD * 2;
  const waveH = clipH - LABEL_H;
  const selected = selectedClipIds.includes(clip.id);
  const color = track.color;

  // ── Move drag ─────────────────────────────────────────────────────────────
  const handleMouseDown = (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    e.stopPropagation();

    if (e.shiftKey) {
      toggleClipSelection(clip.id);
    } else if (!selectedClipIds.includes(clip.id)) {
      setSelectedClipIds([clip.id]);
    }
    setSelectedTrackId(track.id);
    setFocusedPanel("timeline");

    dragStartX.current    = e.clientX;
    dragStartY.current    = e.clientY;
    dragStartTime.current = clip.startTime;
    setDragging(true);

    let lastSeconds = clip.startTime;

    const onMove = (ev: MouseEvent) => {
      let t = Math.max(
        0,
        dragStartTime.current + (ev.clientX - dragStartX.current) / pixelsPerSecond,
      );
      if (useUIStore.getState().snapToGrid) {
        const spb = secondsPerBeat(project.bpm);
        t = snapTime(t, project.bpm, project.timeSignature ?? { numerator: 4, denominator: 4 }, pixelsPerSecond * spb);
      }
      lastSeconds = t;
      moveClip(clip.id, clip.trackId, t);

      const slot = Math.round((ev.clientY - dragStartY.current) / TRACK_HEIGHT);
      setDraggingClipTargetIdx(Math.max(0, Math.min(allTracks.length - 1, trackIndex + slot)));
    };

    const onUp = (ev: MouseEvent) => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      setDragging(false);
      setDraggingClipTargetIdx(null);

      const slot        = Math.round((ev.clientY - dragStartY.current) / TRACK_HEIGHT);
      const idx         = Math.max(0, Math.min(allTracks.length - 1, trackIndex + slot));
      const targetTrack = allTracks[idx];
      const crossTrack  = targetTrack && targetTrack.id !== track.id;

      if (crossTrack) {
        const currentTime = useProjectStore.getState().project.tracks
          .flatMap((t) => t.clips).find((c) => c.id === clip.id)?.startTime ?? lastSeconds;
        moveClipToTrack(clip.id, targetTrack.id, currentTime);
      }

      // Register the completed drag as one undoable command (action already applied live)
      useHistoryStore.getState().push(
        new MoveClipCommand(
          clip.id, clip.trackId,
          lastSeconds, dragStartTime.current,
          crossTrack ? targetTrack.id : undefined,
          crossTrack ? track.id       : undefined,
        ),
      );
    };

    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  };

  // ── Resize left ───────────────────────────────────────────────────────────
  const handleResizeLeft = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (e.button !== 0) return;
    if (!selectedClipIds.includes(clip.id)) setSelectedClipIds([clip.id]);
    setSelectedTrackId(track.id);
    setFocusedPanel("timeline");

    const startX           = e.clientX;
    const initStart        = clip.startTime;
    const initOffset       = clip.offset;
    const initDuration     = clip.duration;
    let finalStart         = initStart;
    let finalOffset        = initOffset;
    let finalDuration      = initDuration;

    const onMove = (ev: MouseEvent) => {
      let delta = (ev.clientX - startX) / pixelsPerSecond;
      delta = Math.max(-initOffset, Math.min(initDuration - 0.1, delta));
      let newStart = initStart + delta;

      if (useUIStore.getState().snapToGrid) {
        const spb = secondsPerBeat(project.bpm);
        newStart  = snapTime(newStart, project.bpm, project.timeSignature ?? { numerator: 4, denominator: 4 }, pixelsPerSecond * spb);
        delta     = Math.max(-initOffset, Math.min(initDuration - 0.1, newStart - initStart));
      }

      finalStart    = initStart    + delta;
      finalOffset   = initOffset   + delta;
      finalDuration = initDuration - delta;
      useProjectStore.getState().resizeClip(clip.id, clip.trackId, finalStart, finalOffset, finalDuration);
    };

    const onUp = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      useHistoryStore.getState().push(
        new ResizeClipCommand(clip.id, clip.trackId, finalStart, finalOffset, finalDuration, initStart, initOffset, initDuration),
      );
    };

    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  };

  // ── Resize right ──────────────────────────────────────────────────────────
  const handleResizeRight = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (e.button !== 0) return;
    if (!selectedClipIds.includes(clip.id)) setSelectedClipIds([clip.id]);
    setSelectedTrackId(track.id);
    setFocusedPanel("timeline");

    const startX       = e.clientX;
    const initDuration = clip.duration;
    let finalDuration  = initDuration;

    const onMove = (ev: MouseEvent) => {
      let d = Math.max(0.1, initDuration + (ev.clientX - startX) / pixelsPerSecond);
      if (useUIStore.getState().snapToGrid) {
        const spb    = secondsPerBeat(project.bpm);
        const snapped = snapTime(clip.startTime + d, project.bpm, project.timeSignature ?? { numerator: 4, denominator: 4 }, pixelsPerSecond * spb);
        d = Math.max(0.1, snapped - clip.startTime);
      }
      finalDuration = d;
      useProjectStore.getState().resizeClip(clip.id, clip.trackId, clip.startTime, clip.offset, finalDuration);
    };

    const onUp = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      useHistoryStore.getState().push(
        new ResizeClipCommand(clip.id, clip.trackId, clip.startTime, clip.offset, finalDuration, clip.startTime, clip.offset, initDuration),
      );
    };

    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  };

  return (
    <div
      onMouseDown={handleMouseDown}
      onContextMenu={(e) => {
        e.preventDefault();
        e.stopPropagation();
        if (!selectedClipIds.includes(clip.id)) setSelectedClipIds([clip.id]);
        setSelectedTrackId(track.id);
        setFocusedPanel("timeline");
        useUIStore.getState().setContextMenu(true, { x: e.clientX, y: e.clientY }, [
          {
            id: "ctx.duplicate_clip",
            label: "Duplicate",
            accelerator: "Ctrl+D",
            action: "edit:duplicate"
          },
          {
            type: "separator",
            id: "ctx.sep.1"
          },
          {
            id: "ctx.delete_clip",
            label: "Delete",
            accelerator: "Del",
            danger: true,
            action: "edit:delete"
          }
        ]);
      }}
      className={`group absolute select-none overflow-hidden border shadow-lg ${clip.muted ? "opacity-50" : ""}`}
      style={{
        left,
        top: PAD,
        width,
        height: clipH,
        cursor: dragging ? "grabbing" : "grab",
        opacity: dragging ? 0.85 : 1,
        borderColor: selected ? color : "rgba(238,242,245,0.14)",
        boxShadow: selected
          ? `0 0 0 1px ${color}, 0 14px 26px rgba(0,0,0,0.26)`
          : "0 10px 22px rgba(0,0,0,0.2)",
      }}
    >
      {/* label bar */}
      <div className="flex items-center gap-2 overflow-hidden px-2" style={{ height: LABEL_H, background: color }}>
        <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-black/50" />
        <span className="truncate text-[9px] -mt-[2px] font-bold leading-none text-black/80">
          {clip.name}
        </span>
        <span className="ml-auto shrink-0 text-[9px] tabular-nums text-black/60">
          {formatBeatLength(clip.duration, project.bpm, project.timeSignature)}
        </span>
      </div>

      {/* waveform area */}
      <div className="relative overflow-hidden" style={{ height: waveH, background: hex2rgba(color, 0.19) }}>
        <div className="pointer-events-none absolute inset-y-0 left-0 w-1.5 bg-white/20" />
        <div className="pointer-events-none absolute inset-y-0 right-0 w-1.5 bg-black/20" />
        {peaks
          ? <WaveformCanvas peaks={peaks} width={width} height={waveH} color={hex2rgba(color, 0.95)} />
          : <div className="flex h-full items-center justify-center text-[9px]" style={{ color: hex2rgba(color, 0.55) }}>
              {project.files.some(f => f.id === clip.fileId) ? "Generating…" : "Missing File"}
            </div>
        }
      </div>

      {/* Resize handles */}
      <div
        className={`absolute left-0 top-0 bottom-0 w-2 cursor-ew-resize opacity-0 transition-opacity ${selected ? "opacity-100" : "group-hover:opacity-100"}`}
        style={{ background: "linear-gradient(90deg, rgba(255,255,255,0.2) 0%, transparent 100%)" }}
        onMouseDown={handleResizeLeft}
      />
      <div
        className={`absolute right-0 top-0 bottom-0 w-2 cursor-ew-resize opacity-0 transition-opacity ${selected ? "opacity-100" : "group-hover:opacity-100"}`}
        style={{ background: "linear-gradient(270deg, rgba(255,255,255,0.2) 0%, transparent 100%)" }}
        onMouseDown={handleResizeRight}
      />
    </div>
  );
}
