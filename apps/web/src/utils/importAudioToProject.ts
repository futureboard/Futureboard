import { useProjectStore } from "../store/projectStore";
import { useHistoryStore } from "../store/historyStore";
import { AddTrackCommand, AddClipCommand, BatchImportCommand } from "../commands";
import { getTrackColor } from "../theme";
import type { DawClip, DawFile, DawTrack } from "../types/daw";
import { useUIStore } from "../store/uiStore";
import { audioImportQueue } from "../engine/AudioImportQueue";

export function isImportableAudioFile(file: File): boolean {
  if (file.type.startsWith("audio/")) return true;
  const m = /\.([^.]+)$/.exec(file.name);
  const ext = m?.[1]?.toLowerCase() ?? "";
  return ext === "wav" || ext === "mp3";
}

export async function readWavMetadata(file: File): Promise<Pick<DawFile, "duration" | "sampleRate" | "channels"> | null> {
  if (!file.name.toLowerCase().endsWith(".wav") && !file.type.includes("wav")) return null;
  const header = await file.slice(0, Math.min(file.size, 65536)).arrayBuffer();
  const view = new DataView(header);
  if (header.byteLength < 44) return null;
  const riff = fourCc(view, 0);
  const wave = fourCc(view, 8);
  if (riff !== "RIFF" || wave !== "WAVE") return null;

  let offset = 12;
  let channels = 0;
  let sampleRate = 0;
  let bitsPerSample = 0;
  let dataBytes = 0;
  while (offset + 8 <= header.byteLength) {
    const id = fourCc(view, offset);
    const size = view.getUint32(offset + 4, true);
    const chunk = offset + 8;
    if (id === "fmt " && chunk + 16 <= header.byteLength) {
      channels = view.getUint16(chunk + 2, true);
      sampleRate = view.getUint32(chunk + 4, true);
      bitsPerSample = view.getUint16(chunk + 14, true);
    } else if (id === "data") {
      dataBytes = size;
      break;
    }
    offset = chunk + size + (size % 2);
  }

  if (!channels || !sampleRate || !bitsPerSample || !dataBytes) return null;
  const bytesPerFrame = channels * (bitsPerSample / 8);
  return {
    duration: dataBytes / bytesPerFrame / sampleRate,
    sampleRate,
    channels,
  };
}

function fourCc(view: DataView, offset: number): string {
  return String.fromCharCode(view.getUint8(offset), view.getUint8(offset + 1), view.getUint8(offset + 2), view.getUint8(offset + 3));
}

export async function decodeAndAddAudioFile(file: File): Promise<DawFile | null> {
  return audioImportQueue.enqueueFile(file, {});
}

export async function importAudioFileToTimelineProgressive(
  file: File,
  startTime: number,
  targetTrackId?: string,
): Promise<DawFile | null> {
  return audioImportQueue.enqueueFile(file, { startTime, trackId: targetTrackId });
}

export async function importNativeAudioPathToTimeline(
  sourcePath: string,
  startTime: number,
  targetTrackId?: string,
): Promise<DawFile | null> {
  return audioImportQueue.enqueueNativePath(sourcePath, { startTime, trackId: targetTrackId });
}

export async function importNativeAudioPathToBrowser(sourcePath: string): Promise<DawFile | null> {
  return audioImportQueue.enqueueNativePath(sourcePath, {});
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
    type: "audio",
    fileId: dawFile.id,
    assetId: dawFile.id,
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

export function batchAddFilesToTimeline(dawFiles: DawFile[], startTime: number, targetTrackId?: string): void {
  if (dawFiles.length === 0) return;
  const { project } = useProjectStore.getState();
  const history = useHistoryStore.getState();

  const newTracks: DawTrack[] = [];
  const clips: Array<{ trackId: string; clip: DawClip }> = [];

  for (const dawFile of dawFiles) {
    const clipId = crypto.randomUUID();
    let trackId = targetTrackId;
    if (!trackId) {
      trackId = crypto.randomUUID();
      newTracks.push({
        id: trackId,
        name: dawFile.name.replace(/\.[^.]+$/, ""),
        type: "audio",
        color: getTrackColor(project.tracks.length + newTracks.length),
        channelCount: dawFile.channels,
        volume: 0.8,
        pan: 0,
        muted: false,
        solo: false,
        armed: false,
        clips: [],
      });
    }
    clips.push({
      trackId,
      clip: {
        id: clipId,
        name: dawFile.name.replace(/\.[^.]+$/, ""),
        type: "audio",
        fileId: dawFile.id,
        assetId: dawFile.id,
        trackId,
        startTime,
        offset: 0,
        duration: dawFile.duration,
        gain: 1,
      },
    });
  }

  history.execute(new BatchImportCommand(newTracks, clips));
  const ui = useUIStore.getState();
  ui.setSelectedClipIds(clips.map((c) => c.clip.id));
  if (newTracks.length > 0) ui.setSelectedTrackId(newTracks[newTracks.length - 1].id);
  ui.setFocusedPanel("timeline");
}

export async function importAudioFilesAsNewTracks(files: File[]): Promise<void> {
  const audioFiles = files.filter(isImportableAudioFile);
  if (audioFiles.length === 0) return;
  audioImportQueue.enqueueFiles(audioFiles, { startTime: 0 });
}
