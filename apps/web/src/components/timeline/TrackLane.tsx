import type { DawTrack } from "../../types/daw";
import { AudioClip } from "./AudioClip";
import { TRACK_HEIGHT } from "../../theme";
import { useUIStore } from "../../store/uiStore";

type Props = {
  track: DawTrack;
  allTracks: DawTrack[];
  trackIndex: number;
  width: number;
};

export function TrackLane({ track, allTracks, trackIndex, width }: Props) {
  const selectedTrackId       = useUIStore((s) => s.selectedTrackId);
  const draggingClipTargetIdx = useUIStore((s) => s.draggingClipTargetIdx);

  const selected    = selectedTrackId === track.id;
  const dropTarget  = draggingClipTargetIdx === trackIndex;
  const even        = trackIndex % 2 === 0;

  const bg = selected
    ? "rgba(255,255,255,0.028)"
    : even
      ? "rgba(255,255,255,0.010)"
      : "rgba(0,0,0,0.12)";

  return (
    <div
      className="relative min-w-0 flex-1 overflow-hidden border-b border-daw-border transition-colors"
      style={{
        height: TRACK_HEIGHT,
        minWidth: width,
        background: bg,
        // drop-target ring
        outline: dropTarget ? `1.5px solid ${track.color}` : undefined,
        outlineOffset: dropTarget ? "-1.5px" : undefined,
      }}
    >
      {/* selected track edge highlight */}
      {selected && (
        <div
          className="pointer-events-none absolute inset-x-0 top-0 h-px opacity-40"
          style={{ background: track.color }}
        />
      )}

      {/* drop-target tint overlay */}
      {dropTarget && (
        <div
          className="pointer-events-none absolute inset-0"
          style={{ background: `${track.color}18` }}
        />
      )}

      {track.clips.map((clip) => (
        <AudioClip
          key={clip.id}
          clip={clip}
          track={track}
          trackIndex={trackIndex}
          allTracks={allTracks}
        />
      ))}
    </div>
  );
}
