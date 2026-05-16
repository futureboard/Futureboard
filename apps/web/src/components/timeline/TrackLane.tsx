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
import { addFileToTimeline, importAudioFileToTimelineProgressive } from "../../utils/importAudioToProject";
import { showToast } from "../ui/Toast";
import { useState } from "react";

type Props = {
  track: DawTrack;
  allTracks: DawTrack[];
  trackIndex: number;
  width: number;
};

// Overscan: render clips this many seconds beyond the visible edge on each side.
const OVERSCAN_SECONDS = 4;

export function TrackLane({ track, allTracks, trackIndex, width }: Props) {
  const selectedTrackId       = useUIStore((s) => s.selectedTrackId);
  const draggingClipTargetIdx = useUIStore((s) => s.draggingClipTargetIdx);
  const selectedClipIds       = useUIStore((s) => s.selectedClipIds);
  const scrollX               = useUIStore((s) => s.scrollX);
  const pixelsPerSecond       = useUIStore((s) => s.pixelsPerSecond);

  // Viewport bounds in seconds — only clips overlapping this range are rendered.
  const viewportWidth = typeof window !== "undefined" ? window.innerWidth - HEADER_WIDTH : 1200;
  const visibleStart  = Math.max(0, scrollX / pixelsPerSecond - OVERSCAN_SECONDS);
  const visibleEnd    = scrollX / pixelsPerSecond + viewportWidth / pixelsPerSecond + OVERSCAN_SECONDS;
  const [isDragOver, setIsDragOver] = useState(false);

  const selected = selectedTrackId === track.id;
  const dropTarget = draggingClipTargetIdx === trackIndex || isDragOver;
  const even = trackIndex % 2 === 0;

  const bg = selected
    ? "rgba(255,255,255,0.028)"
    : even
      ? "rgba(255,255,255,0.010)"
      : "rgba(0,0,0,0.12)";

  const handlePointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    // Only handle clicks directly on the lane (not bubbled up from clips)
    if (e.target !== e.currentTarget) return;

    const { currentTool, selectedBrowserFileId, pixelsPerSecond, snapToGrid } =
      useUIStore.getState();
    const { project } = useProjectStore.getState();

    const selectTrack = () => {
      useUIStore.getState().setSelectedTrackId(track.id);
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
      } else {
        // MIDI / placeholder clip — one bar duration
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
      onDragEnter={(e) => {
        if (![...e.dataTransfer.types].includes("Files") && !e.dataTransfer.types.includes("application/x-mochi-file-id")) return;
        setIsDragOver(true);
      }}
      onDragLeave={() => {
        setIsDragOver(false);
      }}
      onDragOver={(e) => {
        if (![...e.dataTransfer.types].includes("Files") && !e.dataTransfer.types.includes("application/x-mochi-file-id")) return;
        e.preventDefault();
        e.stopPropagation();
        e.dataTransfer.dropEffect = "copy";
      }}
      onDrop={async (e) => {
        const hasFiles = [...e.dataTransfer.types].includes("Files");
        const hasMochiFile = e.dataTransfer.types.includes("application/x-mochi-file-id");
        if (!hasFiles && !hasMochiFile) return;

        e.preventDefault();
        e.stopPropagation();
        setIsDragOver(false);

        const { pixelsPerSecond, snapToGrid } = useUIStore.getState();
        const { project } = useProjectStore.getState();

        const rect = e.currentTarget.getBoundingClientRect();
        const dropX = e.clientX - rect.left;
        let time = dropX / pixelsPerSecond;

        if (snapToGrid) {
          const spb = secondsPerBeat(project.bpm);
          time = snapTime(time, project.bpm, project.timeSignature ?? { numerator: 4, denominator: 4 }, pixelsPerSecond * spb);
        }

        if (hasMochiFile) {
          const fileId = e.dataTransfer.getData("application/x-mochi-file-id");
          const dawFile = project.files.find(f => f.id === fileId);
          if (dawFile) addFileToTimeline(dawFile, Math.max(0, time), track.id);
          return;
        }

        const list = e.dataTransfer.files;
        if (!list?.length) return;
        for (const f of Array.from(list)) {
          await importAudioFileToTimelineProgressive(f, Math.max(0, time), track.id);
        }
      }}
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
          style={{ background: track.color }}
        />
      )}
      {dropTarget && (
        <div
          className="pointer-events-none absolute inset-0"
          style={{ background: `${track.color}18` }}
        />
      )}
      {track.clips
        .filter((clip) => {
          // Keep selected clips mounted even when scrolled off — preserves selection state.
          if (selectedClipIds.includes(clip.id)) return true;
          // Visibility: clip overlaps [visibleStart, visibleEnd]
          return clip.startTime < visibleEnd && clip.startTime + clip.duration > visibleStart;
        })
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
