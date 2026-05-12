import type { DawTrack } from "../../types/daw";
import { AudioClip } from "./AudioClip";
import { TRACK_HEIGHT } from "../../theme";

export function TrackLane({ track, width }: { track: DawTrack; width: number }) {
  return (
    <div
      className="relative shrink-0 overflow-hidden border-b border-daw-border bg-daw-bg"
      style={{ height: TRACK_HEIGHT, width }}
    >
      <div className="absolute inset-0 bg-[linear-gradient(to_right,rgba(61,72,84,0.32)_1px,transparent_1px),linear-gradient(to_right,rgba(61,72,84,0.12)_1px,transparent_1px)] bg-[length:100px_100%,20px_100%] pointer-events-none" />
      <div className="pointer-events-none absolute inset-x-0 h-px bg-daw-surface-high" style={{ top: TRACK_HEIGHT / 2 }} />
      {track.clips.map((clip) => (
        <AudioClip key={clip.id} clip={clip} track={track} />
      ))}
    </div>
  );
}
