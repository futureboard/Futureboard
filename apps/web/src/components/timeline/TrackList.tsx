import { useProjectStore } from "../../store/projectStore";
import { TrackHeader } from "./TrackHeader";
import { TrackLane } from "./TrackLane";
import { HEADER_WIDTH, TRACK_HEIGHT } from "../../theme";

export function TrackList({ timelineWidth }: { timelineWidth: number }) {
  const tracks = useProjectStore((s) => s.project.tracks);

  if (tracks.length === 0) {
    return (
      <div className="flex h-full min-h-96 flex-col items-center justify-center gap-3" style={{ paddingLeft: HEADER_WIDTH }}>

      </div>
    );
  }

  const minTimelineWidth = HEADER_WIDTH + timelineWidth;
  const contentHeight = tracks.length * TRACK_HEIGHT;

  return (
    <div
      className="relative flex h-full min-h-full min-w-full flex-col"
      style={{ minWidth: minTimelineWidth, minHeight: `max(100%, ${contentHeight}px)` }}
    >
      <div
        className="sticky left-0 z-40 h-full shrink-0 border-r border-daw-border bg-daw-surface shadow-[8px_0_18px_rgba(0,0,0,0.22)]"
        style={{ width: HEADER_WIDTH, minWidth: HEADER_WIDTH }}
      />
      {tracks.map((track, i) => (
        <div
          key={track.id}
          className="absolute left-0 right-0 flex min-w-full"
          style={{ minWidth: minTimelineWidth, top: i * TRACK_HEIGHT }}
        >
          <TrackHeader track={track} index={i} />
          <TrackLane track={track} allTracks={tracks} trackIndex={i} width={timelineWidth} />
        </div>
      ))}
    </div>
  );
}
