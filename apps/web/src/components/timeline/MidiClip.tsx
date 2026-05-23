import { useRef, useState } from "react";
import type { DawClip, DawTrack } from "../../types/daw";
import { useProjectStore } from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
import { useHistoryStore } from "../../store/historyStore";
import {
  DuplicateClipsCommand,
  GlueClipsCommand,
  MoveClipCommand,
  MoveClipsCommand,
  ResizeClipCommand,
  SplitClipCommand,
  UpdateClipCommand,
} from "../../commands";
import { isPrimaryModifier } from "../../hooks/useModifierKeys";
import { TRACK_HEIGHT } from "../../theme";
import { formatBeatLength, secondsPerBeat, snapTime } from "../../utils/musicalTime";
import { showToast } from "../ui/Toast";

const LABEL_H = 14;
const PAD = 7;
const MIN_SPLIT_MARGIN = 0.05;

function hex2rgba(hex: string, a: number) {
  const r = parseInt(hex.slice(1, 3), 16);
  const g = parseInt(hex.slice(3, 5), 16);
  const b = parseInt(hex.slice(5, 7), 16);
  return `rgba(${r},${g},${b},${a})`;
}

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

export function MidiClip({ clip, track, trackIndex, allTracks }: Props) {
  // Specific selectors — avoid subscribing to scrollX which updates at 60fps during scroll.
  const pixelsPerSecond = useUIStore(s => s.pixelsPerSecond);
  const selectedClipIds = useUIStore(s => s.selectedClipIds);
  const currentTool     = useUIStore(s => s.currentTool);
  const { moveClip, moveClipToTrack, project } = useProjectStore();

  const dragStartX    = useRef(0);
  const dragStartY    = useRef(0);
  const dragStartTime = useRef(0);
  const [dragging, setDragging] = useState(false);

  const left  = clip.startTime * pixelsPerSecond;
  const width = Math.max(4, clip.duration * pixelsPerSecond);
  const clipH = TRACK_HEIGHT - PAD * 2;
  const noteH = clipH - LABEL_H;
  const selected = selectedClipIds.includes(clip.id);
  const color = track.color;

  // ── Real MIDI note preview ─────────────────────────────────────────────────
  const clipOffset = clip.offset ?? 0;
  const clipNotes  = clip.notes ?? [];
  // Only notes that overlap the visible clip window [clipOffset, clipOffset + clip.duration]
  const visibleNotes = clipNotes.filter(
    (n) => n.start < clipOffset + clip.duration && n.start + n.duration > clipOffset,
  );
  // Pitch range — default to C3–C5 when empty so proportions look reasonable
  let topPitch = 72, bottomPitch = 48;
  if (visibleNotes.length > 0) {
    const lo = Math.min(...visibleNotes.map((n) => n.pitch));
    const hi = Math.max(...visibleNotes.map((n) => n.pitch));
    topPitch    = hi + 2;
    bottomPitch = lo - 2;
  }
  const pitchRange = Math.max(12, topPitch - bottomPitch);

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

    const rightClip = useProjectStore.getState().project.tracks
      .flatMap((t) => t.clips)
      .find(
        (c) => c.id !== clip.id &&
          Math.abs(c.startTime - splitTime) < 0.002 &&
          c.trackId === clip.trackId,
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

    const targetIds = sids.includes(clip.id) && sids.length >= 2 ? sids : null;

    if (!targetIds) {
      useUIStore.getState().setSelectedClipIds([clip.id]);
      useUIStore.getState().setSelectedTrackId(track.id);
      useUIStore.getState().setFocusedPanel("timeline");
      showToast("Select 2 or more adjacent clips to glue");
      return;
    }

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
  const startPointerDrag = (e: React.PointerEvent) => {
    const primaryMod = isPrimaryModifier(e);

    if (e.shiftKey) {
      useUIStore.getState().toggleClipSelection(clip.id);
    } else if (primaryMod) {
      if (!selectedClipIds.includes(clip.id)) {
        useUIStore.getState().setSelectedClipIds([...selectedClipIds, clip.id]);
      }
      // ABORT: Primary modifier + drag is reserved for the global Snip gesture in Timeline.tsx.
      return;
    } else if (!selectedClipIds.includes(clip.id)) {
      useUIStore.getState().setSelectedClipIds([clip.id]);
    }
    useUIStore.getState().setSelectedTrackId(track.id);
    useUIStore.getState().setFocusedPanel("timeline");

    dragStartX.current    = e.clientX;
    dragStartY.current    = e.clientY;
    dragStartTime.current = clip.startTime;
    setDragging(true);

    let draggedClipId  = clip.id;
    let draggedTrackId = clip.trackId;
    let duplicated     = false;
    let lastSeconds    = clip.startTime;
    let rafId: number | null = null;

    const allClipsFlat = () => useProjectStore.getState().project.tracks.flatMap((t) => t.clips);
    const ui0      = useUIStore.getState();
    const groupIds = ui0.selectedClipIds.includes(clip.id) ? ui0.selectedClipIds : [clip.id];
    const initialTimes = new Map<string, number>(
      allClipsFlat()
        .filter((c) => groupIds.includes(c.id))
        .map((c) => [c.id, c.startTime]),
    );

    const onMove = (ev: MouseEvent) => {
      // Duplicate on threshold when Alt was held at drag start.
      const altPressed = ev.altKey;
      if (altPressed && !duplicated && Math.abs(ev.clientX - dragStartX.current) >= 4) {
        duplicated = true;
        const ui = useUIStore.getState();
        const clipsToDup = ui.selectedClipIds.includes(clip.id)
          ? ui.selectedClipIds
          : [clip.id];

        const cmd = new DuplicateClipsCommand(clipsToDup);
        useHistoryStore.getState().execute(cmd);
        const newIds = cmd.newClipIds;

        const dupClip = allClipsFlat().find(
          (c) =>
            newIds.includes(c.id) &&
            c.trackId === clip.trackId &&
            Math.abs(c.startTime - (clip.startTime + clip.duration)) < 0.001,
        );

        if (dupClip) {
          moveClip(dupClip.id, dupClip.trackId, clip.startTime);
          draggedClipId  = dupClip.id;
          draggedTrackId = dupClip.trackId;
          dragStartTime.current = clip.startTime;
          dragStartX.current    = ev.clientX;
        }

        initialTimes.clear();
        allClipsFlat()
          .filter((c) => newIds.includes(c.id))
          .forEach((c) => initialTimes.set(c.id, c.startTime));

        ui.setSelectedClipIds(newIds);
      }

      let t = Math.max(
        0,
        dragStartTime.current + (ev.clientX - dragStartX.current) / pixelsPerSecond,
      );
      if (useUIStore.getState().snapToGrid) {
        const spb = secondsPerBeat(project.bpm);
        t = snapTime(t, project.bpm, project.timeSignature ?? { numerator: 4, denominator: 4 }, pixelsPerSecond * spb, useUIStore.getState().arrangementGridDivision);
      }
      const delta = t - dragStartTime.current;
      lastSeconds = t;

      const slot = Math.round((ev.clientY - dragStartY.current) / TRACK_HEIGHT);
      useUIStore.getState().setDraggingClipTargetIdx(Math.max(0, Math.min(allTracks.length - 1, trackIndex + slot)));

      if (rafId === null) {
        rafId = requestAnimationFrame(() => {
          rafId = null;
          const clips = allClipsFlat();
          for (const [id, origTime] of initialTimes) {
            const c = clips.find((x) => x.id === id);
            if (!c) continue;
            moveClip(id, c.trackId, Math.max(0, origTime + delta));
          }
        });
      }
    };

    const onUp = (ev: MouseEvent) => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      const delta = lastSeconds - dragStartTime.current;
      if (rafId !== null) {
        cancelAnimationFrame(rafId);
        rafId = null;
        const clips = allClipsFlat();
        for (const [id, origTime] of initialTimes) {
          const c = clips.find((x) => x.id === id);
          if (!c) continue;
          moveClip(id, c.trackId, Math.max(0, origTime + delta));
        }
      }
      setDragging(false);
      useUIStore.getState().setDraggingClipTargetIdx(null);

      const slot        = Math.round((ev.clientY - dragStartY.current) / TRACK_HEIGHT);
      const idx         = Math.max(0, Math.min(allTracks.length - 1, trackIndex + slot));
      const targetTrack = allTracks[idx];
      const crossTrack  = targetTrack && targetTrack.id !== draggedTrackId;

      if (crossTrack) {
        const currentTime = allClipsFlat().find((c) => c.id === draggedClipId)?.startTime ?? lastSeconds;
        moveClipToTrack(draggedClipId, targetTrack.id, currentTime);
      }

      if (initialTimes.size > 1) {
        const moves = Array.from(initialTimes.entries()).map(([id, oldTime]) => ({
          clipId: id,
          trackId: allClipsFlat().find((c) => c.id === id)?.trackId ?? id,
          newTime: Math.max(0, oldTime + delta),
          oldTime,
        }));
        useHistoryStore.getState().push(new MoveClipsCommand(moves));
      } else {
        useHistoryStore.getState().push(
          new MoveClipCommand(
            draggedClipId, draggedTrackId,
            lastSeconds, dragStartTime.current,
            crossTrack ? targetTrack.id : undefined,
            crossTrack ? draggedTrackId : undefined,
          ),
        );
      }
    };

    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  };

  // ── Main pointerDown dispatcher ─────────────────────────────────────────────
  const handlePointerDown = (e: React.PointerEvent) => {
    if (e.button !== 0) return;

    const tool = useUIStore.getState().currentTool;

    if (tool === "cut")  { e.stopPropagation(); handleCutTool(e); return; }
    if (tool === "mute") { e.stopPropagation(); handleMuteTool();  return; }
    if (tool === "glue") { e.stopPropagation(); handleGlueTool();  return; }
    if (tool === "time") { e.stopPropagation(); handleTimeTool();  return; }

    if (tool === "pen") {
      e.stopPropagation();
      if (!selectedClipIds.includes(clip.id)) useUIStore.getState().setSelectedClipIds([clip.id]);
      useUIStore.getState().setSelectedTrackId(track.id);
      useUIStore.getState().setFocusedPanel("timeline");
      return;
    }

    if (tool === "pointer") {
      if (isPrimaryModifier(e)) {
        startPointerDrag(e);
      } else {
        e.stopPropagation();
        startPointerDrag(e);
      }
    }
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
        newStart  = snapTime(newStart, project.bpm, project.timeSignature ?? { numerator: 4, denominator: 4 }, pixelsPerSecond * spb, useUIStore.getState().arrangementGridDivision);
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
        const snapped = snapTime(clip.startTime + d, project.bpm, project.timeSignature ?? { numerator: 4, denominator: 4 }, pixelsPerSecond * spb, useUIStore.getState().arrangementGridDivision);
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
      data-clip-id={clip.id}
      data-track-id={track.id}
      onPointerDown={handlePointerDown}
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

      {/* MIDI note pattern area */}
      <div
        className="relative overflow-hidden"
        style={{ height: noteH, background: hex2rgba(color, 0.12) }}
      >
        <div className="pointer-events-none absolute inset-y-0 left-0 w-1.5 bg-white/20" />
        <div className="pointer-events-none absolute inset-y-0 right-0 w-1.5 bg-black/20" />
        {visibleNotes.length === 0 ? (
          // Empty MIDI clip — subtle horizontal grid lines
          [0.33, 0.66].map((f) => (
            <div key={f} className="pointer-events-none absolute left-0 right-0 h-px"
                 style={{ top: `${f * 100}%`, background: hex2rgba(color, 0.15) }} />
          ))
        ) : (
          visibleNotes.map((note) => {
            // Clamp note to clip's visible window
            const visStart = Math.max(note.start - clipOffset, 0);
            const visEnd   = Math.min(note.start + note.duration - clipOffset, clip.duration);
            if (visEnd <= visStart) return null;
            const leftPct  = (visStart / clip.duration) * 100;
            const widthPct = ((visEnd - visStart) / clip.duration) * 100;
            const topPct   = ((topPitch - note.pitch) / pitchRange) * 100;
            return (
              <div
                key={note.id}
                className="pointer-events-none absolute rounded-sm"
                style={{
                  left:   `${leftPct}%`,
                  width:  `${Math.max(widthPct, 0.5)}%`,
                  top:    `${topPct}%`,
                  height: "2px",
                  background: hex2rgba(color, 0.85),
                }}
              />
            );
          })
        )}
      </div>

      {/* Cut tool indicator */}
      {currentTool === "cut" && (
        <div
          className="pointer-events-none absolute inset-0 flex items-center justify-center opacity-0 group-hover:opacity-100"
          aria-hidden
        >
          <div className="absolute top-0 bottom-0 w-px bg-white/70" style={{ left: "50%" }} />
        </div>
      )}

      {/* Resize handles — only for pointer/pen tools */}
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
