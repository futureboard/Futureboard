import type { DawTrack } from "../../types/daw";
import { AudioClip } from "./AudioClip";
import { TRACK_HEIGHT, HEADER_WIDTH } from "../../theme";
import { useUIStore } from "../../store/uiStore";
import { snapTime, secondsPerBeat } from "../../utils/musicalTime";
import { useProjectStore } from "../../store/projectStore";
import { addFileToTimeline, decodeAndAddAudioFile } from "../../utils/importAudioToProject";
import { useState } from "react";

type Props = {
  track: DawTrack;
  allTracks: DawTrack[];
  trackIndex: number;
  width: number;
};

export function TrackLane({ track, allTracks, trackIndex, width }: Props) {
  const selectedTrackId       = useUIStore((s) => s.selectedTrackId);
  const draggingClipTargetIdx = useUIStore((s) => s.draggingClipTargetIdx);
  const [isDragOver, setIsDragOver] = useState(false);

  const selected    = selectedTrackId === track.id;
  const dropTarget  = draggingClipTargetIdx === trackIndex || isDragOver;
  const even        = trackIndex % 2 === 0;

  const bg = selected
    ? "rgba(255,255,255,0.028)"
    : even
      ? "rgba(255,255,255,0.010)"
      : "rgba(0,0,0,0.12)";

  return (
    <div
      onPointerDown={() => {
        useUIStore.getState().setSelectedTrackId(track.id);
        useUIStore.getState().setSelectedClipIds([]);
        useUIStore.getState().setFocusedPanel("timeline");
      }}
      onDragEnter={(e) => {
        if (![...e.dataTransfer.types].includes("Files") && !e.dataTransfer.types.includes("application/x-mochi-file-id")) return;
        setIsDragOver(true);
      }}
      onDragLeave={(e) => {
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

        const { pixelsPerSecond, scrollX, snapToGrid } = useUIStore.getState();
        const { project } = useProjectStore.getState();
        
        // Calculate drop time
        const rect = e.currentTarget.getBoundingClientRect();
        // The container is offset by HEADER_WIDTH, but track lane rect.left might already be correct.
        // TrackLane is rendered inside TrackList which is rendered alongside TimelineRuler, but TrackLane starts at X=0 relative to the scroll container's content.
        // Actually TrackList has `padding-left: HEADER_WIDTH` or similar? Let's check. 
        // We know scrollX. The absolute cursor X is e.clientX.
        // The timeline content starts at rect.left.
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

        // It's OS files
        const list = e.dataTransfer.files;
        if (!list?.length) return;
        
        // Use the drop time for all imported files (or stagger them?)
        for (const f of list) {
          const dawFile = await decodeAndAddAudioFile(f);
          if (dawFile) {
            addFileToTimeline(dawFile, Math.max(0, time), track.id);
          }
        }
      }}
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
