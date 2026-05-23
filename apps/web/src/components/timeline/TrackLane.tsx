import { useRef, useState, useEffect } from "react";
import type { DawClip, DawTrack } from "../../types/daw";
import { clipType } from "../../types/daw";
import { AudioClip } from "./AudioClip";
import { MidiClip } from "./MidiClip";
import { HEADER_WIDTH, TRACK_HEIGHT } from "../../theme";
import { useUIStore } from "../../store/uiStore";
import { snapTime, secondsPerBeat } from "../../utils/musicalTime";
import { useProjectStore } from "../../store/projectStore";
import { useHistoryStore } from "../../store/historyStore";
import { AddClipCommand } from "../../commands";
import { isPrimaryModifier } from "../../hooks/useModifierKeys";
import { showToast } from "../ui/Toast";

type Props = {
  track: DawTrack;
  allTracks: DawTrack[];
  trackIndex: number;
  width: number;
};

// Overscan: render clips this many seconds beyond the visible edge on each side.
const OVERSCAN_SECONDS = 4;

function computeVisibleIds(
  clips: DawClip[],
  selectedClipIds: string[],
  scrollX: number,
  pixelsPerSecond: number,
): string[] {
  const viewW = typeof window !== "undefined" ? window.innerWidth - HEADER_WIDTH : 1200;
  const start = Math.max(0, scrollX / pixelsPerSecond - OVERSCAN_SECONDS);
  const end   = scrollX / pixelsPerSecond + viewW / pixelsPerSecond + OVERSCAN_SECONDS;
  return clips
    .filter((c) => selectedClipIds.includes(c.id) || (c.startTime < end && c.startTime + c.duration > start))
    .map((c) => c.id);
}

export function TrackLane({ track, allTracks, trackIndex, width }: Props) {
  const selectedTrackId       = useUIStore((s) => s.selectedTrackId);
  const selectedTrackIds      = useUIStore((s) => s.selectedTrackIds);
  const draggingClipTargetIdx = useUIStore((s) => s.draggingClipTargetIdx);
  const selectedClipIds       = useUIStore((s) => s.selectedClipIds);
  const pixelsPerSecond       = useUIStore((s) => s.pixelsPerSecond);

  // scrollX drives clip visibility — use ref + imperative subscribe to avoid
  // re-rendering all TrackLane instances on every scroll frame.
  const scrollXRef = useRef(useUIStore.getState().scrollX);

  const [visibleIds, setVisibleIds] = useState<string[]>(() =>
    computeVisibleIds(track.clips, selectedClipIds, scrollXRef.current, pixelsPerSecond),
  );

  // Refs to latest props/state for use inside the store subscription callback.
  const trackRef           = useRef(track);
  const selectedIdsRef     = useRef(selectedClipIds);
  const pixelsPerSecondRef = useRef(pixelsPerSecond);
  trackRef.current           = track;
  selectedIdsRef.current     = selectedClipIds;
  pixelsPerSecondRef.current = pixelsPerSecond;

  // Recompute when scrollX changes without triggering a React rerender per frame.
  useEffect(() => {
    const unsub = useUIStore.subscribe((state) => {
      if (state.scrollX === scrollXRef.current) return;
      scrollXRef.current = state.scrollX;
      const next = computeVisibleIds(trackRef.current.clips, selectedIdsRef.current, state.scrollX, pixelsPerSecondRef.current);
      setVisibleIds((prev) => {
        if (prev.length === next.length && prev.every((id, i) => id === next[i])) return prev;
        return next;
      });
    });
    return unsub;
  }, []);

  // Recompute when clips/selection/zoom changes (non-scroll prop changes).
  useEffect(() => {
    const next = computeVisibleIds(track.clips, selectedClipIds, scrollXRef.current, pixelsPerSecond);
    setVisibleIds((prev) => {
      if (prev.length === next.length && prev.every((id, i) => id === next[i])) return prev;
      return next;
    });
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [track.clips, selectedClipIds, pixelsPerSecond]);

  const selected = selectedTrackId === track.id || selectedTrackIds.includes(track.id);
  const primary  = selectedTrackId === track.id;
  const dropTarget = draggingClipTargetIdx === trackIndex;
  const even = trackIndex % 2 === 0;

  const bg = primary
    ? "rgba(255,255,255,0.028)"
    : selected
      ? "rgba(255,255,255,0.018)"
      : even
        ? "rgba(255,255,255,0.010)"
        : "rgba(0,0,0,0.12)";

  const handlePointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    // Only handle clicks directly on the lane (not bubbled up from clips)
    if (e.target !== e.currentTarget) return;

    const { currentTool, selectedBrowserFileId, pixelsPerSecond, snapToGrid, arrangementGridDivision } =
      useUIStore.getState();
    const { project } = useProjectStore.getState();

    const selectTrack = () => {
      if (e.shiftKey) {
        useUIStore.getState().toggleTrackInSelection(track.id);
        if (!useUIStore.getState().selectedTrackId) {
          useUIStore.getState().setSelectedTrackId(track.id);
        }
      } else {
        useUIStore.getState().setSelectedTrackId(track.id);
        useUIStore.getState().setSelectedTrackIds([]);
      }
      useUIStore.getState().setFocusedPanel("timeline");
    };

    if (currentTool === "pen") {
      // Calculate click time from pointer position
      const rect = e.currentTarget.getBoundingClientRect();
      const rawX = e.clientX - rect.left;
      let time = Math.max(0, rawX / pixelsPerSecond);
      if (snapToGrid) {
        const spb = secondsPerBeat(project.bpm);
        time = snapTime(
          time,
          project.bpm,
          project.timeSignature ?? { numerator: 4, denominator: 4 },
          pixelsPerSecond * spb,
          arrangementGridDivision,
        );
      }

      if (track.type === "audio") {
        if (!selectedBrowserFileId) {
          showToast("Select an audio file in the Browser first", true);
          selectTrack();
          return;
        }
        const file = project.files.find((f) => f.id === selectedBrowserFileId);
        if (!file) {
          showToast("Select an audio file in the Browser first", true);
          selectTrack();
          return;
        }
        const newClip: DawClip = {
          id: crypto.randomUUID(),
          name: file.name,
          type: "audio",
          fileId: file.id,
          trackId: track.id,
          startTime: time,
          offset: 0,
          duration: file.duration,
          gain: 1,
        };
        useHistoryStore.getState().execute(new AddClipCommand(track.id, newClip));
        useUIStore.getState().setSelectedClipIds([newClip.id]);
      } else if (track.type === "midi" || track.type === "instrument") {
        // MIDI clip — one bar duration
        const spb = secondsPerBeat(project.bpm);
        const barDuration = spb * (project.timeSignature?.numerator ?? 4);
        const newClip: DawClip = {
          id: crypto.randomUUID(),
          name: "MIDI Clip",
          type: "midi",
          fileId: "",
          trackId: track.id,
          startTime: time,
          offset: 0,
          duration: barDuration,
          gain: 1,
        };
        useHistoryStore.getState().execute(new AddClipCommand(track.id, newClip));
        useUIStore.getState().setSelectedClipIds([newClip.id]);
      } else {
        // Bus, Return, Group, Master, Plugin — clips not supported
        showToast(`${track.type} tracks don't support clips`, true);
        selectTrack();
        return;
      }
      selectTrack();
      return;
    }

    if (currentTool === "automation") {
      e.stopPropagation();
      selectTrack();
      return;
    }

    // pointer / cut / glue / mute / time — lane click selects track, clears clips
    if (isPrimaryModifier(e)) {
      // DO NOT stop propagation here if Ctrl/Cmd is held.
      // We want the event to bubble up to Timeline.tsx so it can start the Snip gesture.
      selectTrack();
    } else {
      e.stopPropagation();
      useUIStore.getState().setSelectedClipIds([]);
      selectTrack();
    }
  };

  return (
    <div
      onPointerDown={handlePointerDown}
      className="relative min-w-0 flex-1 overflow-hidden border-b border-daw-border transition-colors"
      style={{
        height: TRACK_HEIGHT,
        minWidth: width,
        background: bg,
        outline: dropTarget ? `1.5px solid ${track.color}` : undefined,
        outlineOffset: dropTarget ? "-1.5px" : undefined,
      }}
    >
      {selected && (
        <div
          className="pointer-events-none absolute inset-x-0 top-0 h-px opacity-40"
          style={{ background: primary ? track.color : "rgba(255,255,255,0.25)" }}
        />
      )}
      {dropTarget && (
        <div
          className="pointer-events-none absolute inset-0"
          style={{ background: `${track.color}18` }}
        />
      )}
      {track.clips
        .filter((clip) => visibleIds.includes(clip.id))
        .map((clip) =>
          clipType(clip) === "midi" ? (
            <MidiClip
              key={clip.id}
              clip={clip}
              track={track}
              trackIndex={trackIndex}
              allTracks={allTracks}
            />
          ) : (
            <AudioClip
              key={clip.id}
              clip={clip}
              track={track}
              trackIndex={trackIndex}
              allTracks={allTracks}
            />
          )
        )}
    </div>
  );
}
