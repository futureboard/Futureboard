import { audioEngine } from "../engine/AudioEngine";
import { useProjectStore } from "../store/projectStore";
import { useHistoryStore } from "../store/historyStore";
import { AddTrackCommand, AddClipCommand } from "../commands";
import { getTrackColor } from "../theme";
import type { DawClip, DawFile, DawTrack } from "../types/daw";
import { useUIStore } from "../store/uiStore";

export function isImportableAudioFile(file: File): boolean {
  if (file.type.startsWith("audio/")) return true;
  const m = /\.([^.]+)$/.exec(file.name);
  const ext = m?.[1]?.toLowerCase() ?? "";
  return ext === "wav" || ext === "mp3";
}

export async function decodeAndAddAudioFile(file: File): Promise<DawFile | null> {
  if (!isImportableAudioFile(file)) return null;
  const { addFile, setPeaks } = useProjectStore.getState();
  const arrayBuffer = await file.arrayBuffer();
  const fileId = crypto.randomUUID();

  try {
    const audioBuffer = await audioEngine.loadBuffer(
      { id: fileId, name: file.name, mimeType: file.type, duration: 0, sampleRate: 48000, channels: 2 },
      arrayBuffer,
      (fid, peaks) => setPeaks(fid, peaks)
    );

    const dawFile: DawFile = {
      id: fileId,
      name: file.name,
      mimeType: file.type,
      duration: audioBuffer.duration,
      sampleRate: audioBuffer.sampleRate,
      channels: audioBuffer.numberOfChannels,
      localObjectUrl: URL.createObjectURL(file),
    };

    addFile(dawFile);
    return dawFile;
  } catch (err) {
    console.error("Failed to import", file.name, err);
    alert(`Could not import "${file.name}". The format may not be supported.`);
    return null;
  }
}

export function addFileToTimeline(dawFile: DawFile, startTime: number, targetTrackId?: string) {
  const { project } = useProjectStore.getState();
  const history = useHistoryStore.getState();
  
  const clipId = crypto.randomUUID();
  let trackId = targetTrackId;
  
  if (!trackId) {
    trackId = crypto.randomUUID();
    const trackColor = getTrackColor(project.tracks.length);
    const track: DawTrack = {
      id: trackId,
      name: dawFile.name.replace(/\.[^.]+$/, ""),
      type: "audio",
      color: trackColor,
      channelCount: dawFile.channels,
      volume: 0.8,
      pan: 0,
      muted: false,
      solo: false,
      armed: false,
      clips: [],
    };
    history.execute(new AddTrackCommand(track));
  }

  const clip: DawClip = {
    id: clipId,
    name: dawFile.name.replace(/\.[^.]+$/, ""),
    fileId: dawFile.id,
    trackId,
    startTime,
    offset: 0,
    duration: dawFile.duration,
    gain: 1,
  };

  history.execute(new AddClipCommand(trackId, clip));
  useUIStore.getState().setSelectedClipIds([clipId]);
  useUIStore.getState().setSelectedTrackId(trackId);
  useUIStore.getState().setFocusedPanel("timeline");
}

/**
 * Legacy utility: decode each file, add a new audio track, and place one clip at t=0.
 */
export async function importAudioFilesAsNewTracks(files: File[]): Promise<void> {
  const audioFiles = files.filter(isImportableAudioFile);
  if (audioFiles.length === 0) return;

  for (const f of audioFiles) {
    const dawFile = await decodeAndAddAudioFile(f);
    if (dawFile) {
      addFileToTimeline(dawFile, 0);
    }
  }
}
