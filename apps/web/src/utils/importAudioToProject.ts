import { audioEngine } from "../engine/AudioEngine";
import { audioStorage } from "../engine/AudioStorage";
import { buildCacheKey, WAVEFORM_PEAK_LEVELS } from "../engine/waveformCache";
import { useProjectStore } from "../store/projectStore";
import { useHistoryStore } from "../store/historyStore";
import { AddTrackCommand, AddClipCommand } from "../commands";
import { getTrackColor } from "../theme";
import type { DawClip, DawFile, DawTrack } from "../types/daw";
import { useUIStore } from "../store/uiStore";
import { showToast } from "../components/ui/Toast";

type ElectronBackedFile = File & { __futureboardPath?: string };

export function isImportableAudioFile(file: File): boolean {
  if (file.type.startsWith("audio/")) return true;
  const m = /\.([^.]+)$/.exec(file.name);
  const ext = m?.[1]?.toLowerCase() ?? "";
  return ext === "wav" || ext === "mp3";
}

export async function decodeAndAddAudioFile(file: File): Promise<DawFile | null> {
  if (!isImportableAudioFile(file)) return null;
  const { addFile, setPeaks, setWaveformStatus, setWaveformProgress } = useProjectStore.getState();
  const fileId = crypto.randomUUID();
  const storageKey = `audio:${fileId}`;
  const electronPath = (file as ElectronBackedFile).__futureboardPath;
  const storageProvider = electronPath ? "file-handle" : "indexeddb";
  const sourceKey = electronPath ?? storageKey;
  const waveformCacheKeys = WAVEFORM_PEAK_LEVELS.map((level) => buildCacheKey(fileId, level));
  const isLarge = file.size > 200 * 1024 * 1024;
  const isHuge = file.size > 750 * 1024 * 1024;

  try {
    if (isLarge) {
      showToast(isHuge ? "Large audio import: coarse waveform first" : "Large audio import: generating waveform cache");
    }
    setWaveformStatus(fileId, "loading");
    setWaveformProgress(fileId, 0);
    if (!electronPath) {
      audioStorage.save(fileId, file).catch((e) =>
        console.warn("[AudioStorage] source save failed:", e)
      );
    }
    const arrayBuffer = await file.arrayBuffer();
    const audioBuffer = await audioEngine.loadBuffer(
      {
        id: fileId,
        name: file.name,
        mimeType: file.type,
        size: file.size,
        lastModified: file.lastModified,
        originalFileName: file.name,
        storageProvider,
        storageKey: sourceKey,
        cacheKey: sourceKey,
        waveformCacheKeys,
        duration: 0,
        sampleRate: 48000,
        channels: 2,
      },
      arrayBuffer,
      (fid, peaks) => setPeaks(fid, peaks),
      (fid, progress) => setWaveformProgress(fid, progress),
      (fid) => setWaveformStatus(fid, "error")
    );

    const dawFile: DawFile = {
      id: fileId,
      name: file.name,
      mimeType: file.type,
      size: file.size,
      lastModified: file.lastModified,
      originalFileName: file.name,
      duration: audioBuffer.duration,
      sampleRate: audioBuffer.sampleRate,
      channels: audioBuffer.numberOfChannels,
      storageProvider,
      storageKey: sourceKey,
      cacheKey: sourceKey,
      waveformCacheKeys,
      localObjectUrl: URL.createObjectURL(file),
    };

    addFile(dawFile);
    return dawFile;
  } catch (err) {
    console.error("Failed to import", file.name, err);
    setWaveformStatus(fileId, "error");
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
