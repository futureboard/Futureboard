import { ZoomIn, ZoomOut } from "lucide-react";
import { TimelineRuler } from "./TimelineRuler";
import { TrackList } from "./TrackList";
import { Playhead } from "./Playhead";
import { useUIStore } from "../../store/uiStore";
import { useProjectStore } from "../../store/projectStore";

export function Timeline() {
  const { pixelsPerSecond, setPixelsPerSecond, setScrollX } = useUIStore();
  const tracks = useProjectStore((s) => s.project.tracks);
  const zoom = (f: number) => setPixelsPerSecond(Math.min(800, Math.max(10, pixelsPerSecond * f)));
  const timelineSeconds = Math.max(
    16,
    ...tracks.flatMap((track) => track.clips.map((clip) => clip.startTime + clip.duration + 4))
  );
  const timelineWidth = Math.max(1200, Math.ceil(timelineSeconds * pixelsPerSecond));

  return (
    <div className="relative flex flex-1 flex-col overflow-hidden bg-daw-bg">
      <TimelineRuler width={timelineWidth} />

      <div
        className="relative flex-1 overflow-auto bg-daw-bg"
        onScroll={(event) => setScrollX(event.currentTarget.scrollLeft)}
      >
        <TrackList timelineWidth={timelineWidth} />
        <Playhead />
      </div>

      <div className="absolute bottom-3 right-3 z-30 flex items-center gap-1 rounded border border-daw-border bg-daw-surface px-2 py-1 shadow-xl">
        <button onClick={() => zoom(0.75)} title="Zoom out"
          className="flex h-6 w-6 items-center justify-center rounded bg-transparent text-daw-faint transition-colors hover:bg-daw-surface-high hover:text-daw-text">
          <ZoomOut size={12} />
        </button>
        <span className="min-w-10 text-center text-[10px] tabular-nums text-daw-faint">
          {Math.round(pixelsPerSecond)}px/s
        </span>
        <button onClick={() => zoom(1.33)} title="Zoom in"
          className="flex h-6 w-6 items-center justify-center rounded bg-transparent text-daw-faint transition-colors hover:bg-daw-surface-high hover:text-daw-text">
          <ZoomIn size={12} />
        </button>
      </div>
    </div>
  );
}
