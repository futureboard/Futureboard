import { useProjectStore } from "../../store/projectStore";
import { TrackHeader } from "./TrackHeader";
import { TrackLane } from "./TrackLane";
import { HEADER_WIDTH } from "../../theme";

export function TrackList({ timelineWidth }: { timelineWidth: number }) {
  const tracks = useProjectStore((s) => s.project.tracks);

  if (tracks.length === 0) {
    return (
      <div className="flex h-full min-h-96 flex-col items-center justify-center gap-3" style={{ paddingLeft: HEADER_WIDTH }}>
        <svg width={40} height={40} viewBox="0 0 24 24" fill="none" stroke="#3d4854" strokeWidth={1.25}>
          <path d="M9 18V5l12-2v13" /><circle cx="6" cy="18" r="3" /><circle cx="18" cy="16" r="3" />
        </svg>
        <div className="text-center">
          <div className="text-[13px] font-medium text-daw-dim">No arrangement yet</div>
          <div className="mt-1 text-[11px] text-daw-faint">Import audio to create tracks and clips.</div>
        </div>
      </div>
    );
  }

  return (
    <div className="flex min-h-full flex-col" style={{ width: HEADER_WIDTH + timelineWidth }}>
      {tracks.map((track, i) => (
        <div key={track.id} className="flex" style={{ width: HEADER_WIDTH + timelineWidth }}>
          <TrackHeader track={track} index={i} />
          <TrackLane track={track} width={timelineWidth} />
        </div>
      ))}
    </div>
  );
}
