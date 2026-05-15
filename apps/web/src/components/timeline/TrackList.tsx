import { useProjectStore } from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
import { TrackHeader } from "./TrackHeader";
import { TrackLane } from "./TrackLane";
import { AutomationLaneView } from "./AutomationLaneView";
import { HEADER_WIDTH, TRACK_HEIGHT } from "../../theme";
import {
  DndContext,
  PointerSensor,
  useSensor,
  useSensors,
  closestCenter,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  SortableContext,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import type { DawTrack } from "../../types/daw";

/** Total pixel height of one track including all visible automation lanes. */
function trackTotalHeight(track: DawTrack): number {
  const laneHeight = (track.automationLanes ?? [])
    .filter((l) => l.visible)
    .reduce((sum, l) => sum + l.height, 0);
  return TRACK_HEIGHT + laneHeight;
}

function SortableTrackRow({
  track,
  index,
  topOffset,
  allTracks,
  timelineWidth,
  minTimelineWidth,
}: {
  track: DawTrack;
  index: number;
  topOffset: number;
  allTracks: DawTrack[];
  timelineWidth: number;
  minTimelineWidth: number;
}) {
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: track.id });

  const style: React.CSSProperties = {
    minWidth: minTimelineWidth,
    top: topOffset,
    transform: CSS.Transform.toString(transform),
    transition,
    opacity: isDragging ? 0.7 : 1,
    zIndex: isDragging ? 60 : undefined,
    outline: isDragging ? "1px solid rgba(120,170,255,0.55)" : undefined,
    outlineOffset: isDragging ? "-1px" : undefined,
  };

  const visibleLanes = (track.automationLanes ?? []).filter((l) => l.visible);

  return (
    <div
      ref={setNodeRef}
      className="absolute left-0 right-0 flex min-w-full flex-col"
      style={style}
    >
      {/* Main track row */}
      <div className="flex">
        <TrackHeader
          track={track}
          index={index}
          dragHandleProps={{ ...attributes, ...listeners }}
          isDragging={isDragging}
        />
        <TrackLane
          track={track}
          allTracks={allTracks}
          trackIndex={index}
          width={timelineWidth}
        />
      </div>

      {/* Automation lanes */}
      {visibleLanes.map((lane) => (
        <AutomationLaneView
          key={lane.id}
          lane={lane}
          trackColor={track.color}
          width={timelineWidth + HEADER_WIDTH}
        />
      ))}
    </div>
  );
}

const OVERSCAN = 3;

export function TrackList({ timelineWidth }: { timelineWidth: number }) {
  const tracks = useProjectStore((s) => s.project.tracks);
  const reorderTracks = useProjectStore((s) => s.reorderTracks);
  const scrollY = useUIStore((s) => s.scrollY);
  const trackAreaHeight = useUIStore((s) => s.trackAreaHeight);

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } })
  );

  if (tracks.length === 0) {
    return (
      <div className="flex h-full min-h-96 flex-col items-center justify-center gap-3" style={{ paddingLeft: HEADER_WIDTH }}>

      </div>
    );
  }

  const minTimelineWidth = HEADER_WIDTH + timelineWidth;

  // Compute accumulated top offsets per track (supports variable lane heights).
  const topOffsets: number[] = [];
  let accumulated = 0;
  for (const track of tracks) {
    topOffsets.push(accumulated);
    accumulated += trackTotalHeight(track);
  }
  const contentHeight = accumulated;

  // Compute visible slice with overscan
  const visibleStart = scrollY;
  const visibleEnd   = scrollY + Math.max(trackAreaHeight, TRACK_HEIGHT);
  const visibleTracks = tracks.filter((_, i) => {
    const top = topOffsets[i];
    const bot = top + trackTotalHeight(tracks[i]);
    return bot >= visibleStart - OVERSCAN * TRACK_HEIGHT &&
           top <= visibleEnd + OVERSCAN * TRACK_HEIGHT;
  });

  function handleDragEnd(event: DragEndEvent) {
    const { active, over } = event;
    if (!over || active.id === over.id) return;
    reorderTracks(String(active.id), String(over.id));
  }

  return (
    <div
      className="relative flex h-full min-h-full min-w-full flex-col"
      style={{ minWidth: minTimelineWidth, minHeight: `max(100%, ${contentHeight}px)` }}
    >
      <div
        className="sticky left-0 z-40 h-full shrink-0 border-r border-daw-border bg-daw-surface shadow-[8px_0_18px_rgba(0,0,0,0.22)]"
        style={{ width: HEADER_WIDTH, minWidth: HEADER_WIDTH }}
      />
      <DndContext
        sensors={sensors}
        collisionDetection={closestCenter}
        onDragEnd={handleDragEnd}
      >
        <SortableContext
          items={tracks.map((t) => t.id)}
          strategy={verticalListSortingStrategy}
        >
          {visibleTracks.map((track) => {
            const i = tracks.indexOf(track);
            return (
              <SortableTrackRow
                key={track.id}
                track={track}
                index={i}
                topOffset={topOffsets[i]}
                allTracks={tracks}
                timelineWidth={timelineWidth}
                minTimelineWidth={minTimelineWidth}
              />
            );
          })}
        </SortableContext>
      </DndContext>

      {import.meta.env.DEV && (
        <div className="pointer-events-none fixed bottom-2 left-2 z-[9999] rounded bg-black/70 px-2 py-0.5 text-[9px] tabular-nums text-white/50">
          tracks: {visibleTracks.length}/{tracks.length}
        </div>
      )}
    </div>
  );
}
