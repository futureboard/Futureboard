import { useRef, useState } from "react";
import type { DawClip, DawTrack } from "../../types/daw";
import { useProjectStore } from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
import { useHistoryStore } from "../../store/historyStore";
import {
  DuplicateClipsCommand,
  GlueClipsCommand,
  MoveClipCommand,
  ResizeClipCommand,
  SplitClipCommand,
  UpdateClipCommand,
} from "../../commands";
import { isPrimaryModifier } from "../../hooks/useModifierKeys";
import { WaveformCanvas } from "./WaveformCanvas";
import { TRACK_HEIGHT } from "../../theme";
import { formatBeatLength, secondsPerBeat, snapTime } from "../../utils/musicalTime";
import { showToast } from "../ui/Toast";

const LABEL_H = 14;
const PAD = 7;
const MIN_SPLIT_MARGIN = 0.05; // seconds from clip edge — prevent splits too close to endpoints

function hex2rgba(hex: string, a: number) {
  const r = parseInt(hex.slice(1, 3), 16);
  const g = parseInt(hex.slice(3, 5), 16);
  const b = parseInt(hex.slice(5, 7), 16);
  return `rgba(${r},${g},${b},${a})`;
}

// ── Tool cursor map ────────────────────────────────────────────────────────────
const TOOL_CURSOR: Record<string, string> = {
  pointer:    "grab",
  pen:        "crosshair",
  cut:        "crosshair",
  glue:       "copy",
  mute:       "pointer",
  time:       "ew-resize",
  automation: "crosshair",
};

type Props = {
  clip: DawClip;
  track: DawTrack;
  trackIndex: number;
  allTracks: DawTrack[];
};

export function AudioClip({ clip, track, trackIndex, allTracks }: Props) {
  // Specific selectors — AudioClip must NOT subscribe to scrollX or unrelated UI state.
  // Scroll updates scrollX at 60fps; subscribing to the full store would cause a rerender storm.
  const pixelsPerSecond = useUIStore(s => s.pixelsPerSecond);
  const selectedClipIds = useUIStore(s => s.selectedClipIds);
  const currentTool     = useUIStore(s => s.currentTool);
  const { peakCache, waveformStatus, moveClip, moveClipToTrack, project } = useProjectStore();
  const peaks = peakCache.get(clip.fileId);
  const sourceFile = project.files.find((f) => f.id === clip.fileId);
  const status = waveformStatus.get(clip.fileId)
    ?? (peaks && peaks.peaks.length > 0 ? "ready" : sourceFile ? "loading" : "error");

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

  // ── Cut tool ──────────────────────────────────────────────────────────────
  const handleCutTool = (e: React.MouseEvent) => {
    const rect = e.currentTarget.getBoundingClientRect();
    const clickX = e.clientX - rect.left;
    const splitTime = clip.startTime + clickX / pixelsPerSecond;

    if (
      splitTime <= clip.startTime + MIN_SPLIT_MARGIN ||
      splitTime >= clip.startTime + clip.duration - MIN_SPLIT_MARGIN
    ) {
      showToast("Click further from the clip edge to split", true);
      return;
    }

    useHistoryStore.getState().execute(new SplitClipCommand(clip.id, splitTime));

    // Select the right-half clip
    const rightClip = useProjectStore.getState().project.tracks
      .flatMap((t) => t.clips)
      .find(
        (c) => c.id !== clip.id &&
          Math.abs(c.startTime - splitTime) < 0.002 &&
          c.fileId === clip.fileId,
      );
    useUIStore.getState().setSelectedClipIds(rightClip ? [rightClip.id] : [clip.id]);
  };

  // ── Mute tool ─────────────────────────────────────────────────────────────
  const handleMuteTool = () => {
    useHistoryStore.getState().execute(
      new UpdateClipCommand(
        clip.id,
        { muted: !clip.muted },
        clip.muted ? "Unmute Clip" : "Mute Clip",
      ),
    );
  };

  // ── Glue tool ─────────────────────────────────────────────────────────────
  const handleGlueTool = () => {
    const sids = useUIStore.getState().selectedClipIds;
    const { project: proj } = useProjectStore.getState();

    // Include this clip in the working set
    const targetIds = sids.includes(clip.id) && sids.length >= 2 ? sids : null;

    if (!targetIds) {
      // Not enough selected — just select this clip
      useUIStore.getState().setSelectedClipIds([clip.id]);
      useUIStore.getState().setSelectedTrackId(track.id);
      useUIStore.getState().setFocusedPanel("timeline");
      showToast("Select 2 or more adjacent clips to glue");
      return;
    }

    // Resolve clips and their tracks
    const resolved = targetIds.flatMap((id) =>
      proj.tracks.flatMap((t) =>
        t.clips.filter((c) => c.id === id).map((c) => ({ clip: c, trackId: t.id })),
      ),
    );

    const trackIds = new Set(resolved.map((r) => r.trackId));
    if (trackIds.size !== 1) {
      showToast("Select clips on the same track to glue", true);
      return;
    }

    const sorted = [...resolved].sort((a, b) => a.clip.startTime - b.clip.startTime);

    // Check adjacency — allow up to 0.1 s gap
    for (let i = 0; i < sorted.length - 1; i++) {
      const end = sorted[i].clip.startTime + sorted[i].clip.duration;
      const nextStart = sorted[i + 1].clip.startTime;
      if (nextStart - end > 0.1) {
        showToast("Clips must be adjacent to glue", true);
        return;
      }
    }

    const glueTrackId = [...trackIds][0];
    useHistoryStore
      .getState()
      .execute(new GlueClipsCommand(sorted.map((r) => r.clip), glueTrackId));
    useUIStore.getState().setSelectedClipIds([sorted[0].clip.id]);
  };

  // ── Time tool ─────────────────────────────────────────────────────────────
  const handleTimeTool = () => {
    if (!selectedClipIds.includes(clip.id)) useUIStore.getState().setSelectedClipIds([clip.id]);
    useUIStore.getState().setSelectedTrackId(track.id);
    showToast("Time stretch coming soon");
  };

  // ── Pointer drag ──────────────────────────────────────────────────────────
  const startPointerDrag = (e: React.MouseEvent) => {
    const primaryMod = isPrimaryModifier(e);

    // Selection at mousedown:
    // Shift → toggle; Ctrl/Cmd → add (never deselect); plain → replace if not already selected.
    if (e.shiftKey) {
      useUIStore.getState().toggleClipSelection(clip.id);
    } else if (primaryMod) {
      if (!selectedClipIds.includes(clip.id)) {
        useUIStore.getState().setSelectedClipIds([...selectedClipIds, clip.id]);
      }
    } else if (!selectedClipIds.includes(clip.id)) {
      useUIStore.getState().setSelectedClipIds([clip.id]);
    }
    useUIStore.getState().setSelectedTrackId(track.id);
    useUIStore.getState().setFocusedPanel("timeline");

    dragStartX.current    = e.clientX;
    dragStartY.current    = e.clientY;
    dragStartTime.current = clip.startTime;
    setDragging(true);

    let draggedClipId = clip.id;
    let draggedTrackId = clip.trackId;
    let duplicated = false;
    let lastSeconds = clip.startTime;

    const onMove = (ev: MouseEvent) => {
      // Duplicate on threshold when primary modifier was held at drag start.
      if (primaryMod && !duplicated && Math.abs(ev.clientX - dragStartX.current) >= 4) {
        duplicated = true;
        const ui = useUIStore.getState();
        const clipsToDup = ui.selectedClipIds.includes(clip.id)
          ? ui.selectedClipIds
          : [clip.id];

        const cmd = new DuplicateClipsCommand(clipsToDup);
        useHistoryStore.getState().execute(cmd);
        const newIds = cmd.newClipIds;

        // Find the duplicate that corresponds to our specific dragged clip
        // (placed at clip.startTime + clip.duration by duplicateClips).
        const dupClip = useProjectStore.getState().project.tracks
          .flatMap((t) => t.clips)
          .find(
            (c) =>
              newIds.includes(c.id) &&
              c.trackId === clip.trackId &&
              Math.abs(c.startTime - (clip.startTime + clip.duration)) < 0.001,
          );

        if (dupClip) {
          // Reposition duplicate to where the original is so the drag feels seamless.
          moveClip(dupClip.id, dupClip.trackId, clip.startTime);
          draggedClipId  = dupClip.id;
          draggedTrackId = dupClip.trackId;
          dragStartTime.current = clip.startTime;
          dragStartX.current    = ev.clientX; // recalibrate so movement starts from zero
        }

        ui.setSelectedClipIds(newIds);
      }

      let t = Math.max(
        0,
        dragStartTime.current + (ev.clientX - dragStartX.current) / pixelsPerSecond,
      );
      if (useUIStore.getState().snapToGrid) {
        const spb = secondsPerBeat(project.bpm);
        t = snapTime(t, project.bpm, project.timeSignature ?? { numerator: 4, denominator: 4 }, pixelsPerSecond * spb);
      }
      lastSeconds = t;
      moveClip(draggedClipId, draggedTrackId, t);

      const slot = Math.round((ev.clientY - dragStartY.current) / TRACK_HEIGHT);
      useUIStore.getState().setDraggingClipTargetIdx(Math.max(0, Math.min(allTracks.length - 1, trackIndex + slot)));
    };

    const onUp = (ev: MouseEvent) => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      setDragging(false);
      useUIStore.getState().setDraggingClipTargetIdx(null);

      const slot        = Math.round((ev.clientY - dragStartY.current) / TRACK_HEIGHT);
      const idx         = Math.max(0, Math.min(allTracks.length - 1, trackIndex + slot));
      const targetTrack = allTracks[idx];
      const crossTrack  = targetTrack && targetTrack.id !== draggedTrackId;

      if (crossTrack) {
        const currentTime = useProjectStore.getState().project.tracks
          .flatMap((t) => t.clips).find((c) => c.id === draggedClipId)?.startTime ?? lastSeconds;
        moveClipToTrack(draggedClipId, targetTrack.id, currentTime);
      }

      useHistoryStore.getState().push(
        new MoveClipCommand(
          draggedClipId, draggedTrackId,
          lastSeconds, clip.startTime,
          crossTrack ? targetTrack.id : undefined,
          crossTrack ? draggedTrackId : undefined,
        ),
      );
    };

    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  };

  // ── Main mouseDown dispatcher ─────────────────────────────────────────────
  const handleMouseDown = (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    e.stopPropagation();

    const tool = useUIStore.getState().currentTool;

    if (tool === "cut")  { handleCutTool(e); return; }
    if (tool === "mute") { handleMuteTool();  return; }
    if (tool === "glue") { handleGlueTool();  return; }
    if (tool === "time") { handleTimeTool();  return; }

    // pen tool on existing clip falls through to pointer (select, no drag)
    if (tool === "pen") {
      if (!selectedClipIds.includes(clip.id)) useUIStore.getState().setSelectedClipIds([clip.id]);
      useUIStore.getState().setSelectedTrackId(track.id);
      useUIStore.getState().setFocusedPanel("timeline");
      return;
    }

    // pointer (default) — full drag behavior
    startPointerDrag(e);
  };

  // ── Resize left ───────────────────────────────────────────────────────────
  const handleResizeLeft = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (e.button !== 0) return;
    if (!selectedClipIds.includes(clip.id)) useUIStore.getState().setSelectedClipIds([clip.id]);
    useUIStore.getState().setSelectedTrackId(track.id);
    useUIStore.getState().setFocusedPanel("timeline");

    const startX       = e.clientX;
    const initStart    = clip.startTime;
    const initOffset   = clip.offset;
    const initDuration = clip.duration;
    let finalStart     = initStart;
    let finalOffset    = initOffset;
    let finalDuration  = initDuration;

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
    if (!selectedClipIds.includes(clip.id)) useUIStore.getState().setSelectedClipIds([clip.id]);
    useUIStore.getState().setSelectedTrackId(track.id);
    useUIStore.getState().setFocusedPanel("timeline");

    const startX      = e.clientX;
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

  const clipCursor = dragging ? "grabbing" : TOOL_CURSOR[currentTool] ?? "grab";

  return (
    <div
      onMouseDown={handleMouseDown}
      onContextMenu={(e) => {
        e.preventDefault();
        e.stopPropagation();
        if (!selectedClipIds.includes(clip.id)) useUIStore.getState().setSelectedClipIds([clip.id]);
        useUIStore.getState().setSelectedTrackId(track.id);
        useUIStore.getState().setFocusedPanel("timeline");
        useUIStore.getState().setContextMenu(true, { x: e.clientX, y: e.clientY }, [
          { id: "ctx.duplicate_clip", label: "Duplicate", accelerator: "Ctrl+D", action: "edit:duplicate" },
          { type: "separator", id: "ctx.sep.1" },
          { id: "ctx.delete_clip", label: "Delete", accelerator: "Del", danger: true, action: "edit:delete" },
        ]);
      }}
      className={`group absolute select-none overflow-hidden border shadow-lg ${clip.muted ? "opacity-50" : ""}`}
      style={{
        left,
        top: PAD,
        width,
        height: clipH,
        cursor: clipCursor,
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
        <WaveformCanvas
          peaks={peaks}
          width={width}
          height={waveH}
          color={hex2rgba(color, 0.95)}
          sourceDuration={sourceFile?.duration ?? peaks?.duration}
          sampleRate={sourceFile?.sampleRate ?? peaks?.sampleRate}
          clipOffset={clip.offset}
          clipDuration={clip.duration}
          muted={!!clip.muted || track.muted}
          selected={selected}
          status={status}
        />
      </div>

      {/* Cut tool indicator — vertical line at cursor position */}
      {currentTool === "cut" && (
        <div
          className="pointer-events-none absolute inset-0 flex items-center justify-center opacity-0 group-hover:opacity-100"
          aria-hidden
        >
          <div className="absolute top-0 bottom-0 w-px bg-white/70" style={{ left: "50%" }} />
        </div>
      )}

      {/* Resize handles — only for pointer tool */}
      {(currentTool === "pointer" || currentTool === "pen") && (
        <>
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
        </>
      )}
    </div>
  );
}
