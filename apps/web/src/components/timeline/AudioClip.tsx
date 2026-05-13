import { useRef, useState } from "react";
import type { DawClip, DawTrack } from "../../types/daw";
import { useProjectStore } from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
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
  const { pixelsPerSecond, selectedClipId, setSelectedClipId, setSelectedTrackId, setDraggingClipTargetIdx } = useUIStore();
  const { peakCache, moveClip, moveClipToTrack, project } = useProjectStore();
  const peaks = peakCache.get(clip.fileId);

  const dragStartX   = useRef(0);
  const dragStartY   = useRef(0);
  const dragStartTime = useRef(0);
  const [dragging, setDragging] = useState(false);

  const left  = clip.startTime * pixelsPerSecond;
  const width = Math.max(4, clip.duration * pixelsPerSecond);
  const clipH = TRACK_HEIGHT - PAD * 2;
  const waveH = clipH - LABEL_H;
  const selected = selectedClipId === clip.id;
  const color = track.color;

  const handleMouseDown = (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    setSelectedClipId(clip.id);
    setSelectedTrackId(track.id);
    dragStartX.current    = e.clientX;
    dragStartY.current    = e.clientY;
    dragStartTime.current = clip.startTime;
    setDragging(true);

    const onMove = (ev: MouseEvent) => {
      // ── horizontal: move in time ──────────────────────────────────────────
      let targetSeconds = Math.max(
        0,
        dragStartTime.current + (ev.clientX - dragStartX.current) / pixelsPerSecond
      );
      if (useUIStore.getState().snapToGrid) {
        const spb     = secondsPerBeat(project.bpm);
        const timeSig = project.timeSignature ?? { numerator: 4, denominator: 4 };
        targetSeconds = snapTime(targetSeconds, project.bpm, timeSig, pixelsPerSecond * spb);
      }
      moveClip(clip.id, clip.trackId, targetSeconds);

      // ── vertical: track which lane the clip will land in ─────────────────
      const deltaSlots   = Math.round((ev.clientY - dragStartY.current) / TRACK_HEIGHT);
      const targetIdx    = Math.max(0, Math.min(allTracks.length - 1, trackIndex + deltaSlots));
      setDraggingClipTargetIdx(targetIdx);
    };

    const onUp = (ev: MouseEvent) => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      setDragging(false);
      setDraggingClipTargetIdx(null);

      // ── commit cross-track move if track changed ───────────────────────
      const deltaSlots  = Math.round((ev.clientY - dragStartY.current) / TRACK_HEIGHT);
      const targetIdx   = Math.max(0, Math.min(allTracks.length - 1, trackIndex + deltaSlots));
      const targetTrack = allTracks[targetIdx];

      if (targetTrack && targetTrack.id !== track.id) {
        // Time was already updated by the last onMove; read it from store
        const currentTime = useProjectStore.getState().project.tracks
          .flatMap((t) => t.clips)
          .find((c) => c.id === clip.id)?.startTime ?? clip.startTime;
        moveClipToTrack(clip.id, targetTrack.id, currentTime);
      }
    };

    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  };

  return (
    <div
      onMouseDown={handleMouseDown}
      className="absolute select-none overflow-hidden border shadow-lg"
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
              Generating…
            </div>
        }
      </div>
    </div>
  );
}
