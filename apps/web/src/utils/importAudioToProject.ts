import { audioEngine } from "../engine/AudioEngine";
import { audioAssetManager } from "../engine/AudioAssetManager";
import { useProjectStore } from "../store/projectStore";
import { useHistoryStore } from "../store/historyStore";
import { AddTrackCommand, AddClipCommand } from "../commands";
import { getTrackColor } from "../theme";
import type { DawClip, DawFile, DawProjectAsset, DawTrack } from "../types/daw";
import { useUIStore } from "../store/uiStore";
import { showToast } from "../components/ui/Toast";

export function isImportableAudioFile(file: File): boolean {
  if (file.type.startsWith("audio/")) return true;
  const m = /\.([^.]+)$/.exec(file.name);
  const ext = m?.[1]?.toLowerCase() ?? "";
  return ext === "wav" || ext === "mp3";
}

async function readWavMetadata(file: File): Promise<Pick<DawFile, "duration" | "sampleRate" | "channels"> | null> {
  if (!file.name.toLowerCase().endsWith(".wav") && !file.type.includes("wav")) return null;
  const header = await file.slice(0, Math.min(file.size, 4096)).arrayBuffer();
  const view = new DataView(header);
  if (header.byteLength < 44) return null;
  const riff = String.fromCharCode(view.getUint8(0), view.getUint8(1), view.getUint8(2), view.getUint8(3));
  const wave = String.fromCharCode(view.getUint8(8), view.getUint8(9), view.getUint8(10), view.getUint8(11));
  if (riff !== "RIFF" || wave !== "WAVE") return null;

  let offset = 12;
  let channels = 0;
  let sampleRate = 0;
  let bitsPerSample = 0;
  let dataBytes = 0;
  while (offset + 8 <= header.byteLength) {
    const id = String.fromCharCode(view.getUint8(offset), view.getUint8(offset + 1), view.getUint8(offset + 2), view.getUint8(offset + 3));
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

export async function decodeAndAddAudioFile(file: File): Promise<DawFile | null> {
  if (!isImportableAudioFile(file)) return null;
  const store = useProjectStore.getState();
  const { addFile, setPeaks, setWaveformStatus, setWaveformProgress } = store;
  const fileId = crypto.randomUUID();
  const isLarge = file.size > 200 * 1024 * 1024;
  const isHuge = file.size > 750 * 1024 * 1024;

  try {
    if (isLarge) {
      showToast(isHuge ? "Large audio import: coarse waveform first" : "Large audio import: generating waveform cache");
    }
    setWaveformStatus(fileId, "loading");
    setWaveformProgress(fileId, 0);
    const assetManifest = await audioAssetManager.saveImportedAudioAsset(fileId, file);
    const arrayBuffer = await file.arrayBuffer();
    const audioBuffer = await audioEngine.loadBuffer(
      {
        id: fileId,
        name: file.name,
        mimeType: file.type,
        size: file.size,
        lastModified: file.lastModified,
        originalFileName: file.name,
        ...assetManifest,
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
      ...assetManifest,
      localObjectUrl: URL.createObjectURL(file),
    };

    addFile(dawFile);

    // ── Register project asset manifest for folder-project imports ─────────
    // `saveImportedAudioAsset` already copied the file to Media/Audio/ when
    // the project is in folder mode.  Register it so:
    //   • The Browser panel survives project reopen.
    //   • clip.assetId → asset.relativePath → mediaPath for native playback.
    if (dawFile.storageProvider === "project-folder" && dawFile.relativePath) {
      const now = new Date().toISOString();
      const asset: DawProjectAsset = {
        id: fileId,          // same UUID — DawFile and DawProjectAsset share it
        type: "audio",
        name: dawFile.name,
        originalName: file.name,
        relativePath: dawFile.relativePath,
        size: dawFile.size,
        durationSeconds: audioBuffer.duration,
        sampleRate: audioBuffer.sampleRate,
        channels: audioBuffer.numberOfChannels,
        mimeType: file.type || "audio/wav",
        createdAt: now,
        updatedAt: now,
      };
      // addAsset is idempotent — safe to call even if already present.
      useProjectStore.getState().addAsset(asset);
    }

    return dawFile;
  } catch (err) {
    console.error("Failed to import", file.name, err);
    setWaveformStatus(fileId, "error");
    alert(`Could not import "${file.name}". The format may not be supported.`);
    return null;
  }
}

export async function importAudioFileToTimelineProgressive(
  file: File,
  startTime: number,
  targetTrackId?: string,
): Promise<DawFile | null> {
  if (!isImportableAudioFile(file)) return null;
  const isLarge = file.size > 200 * 1024 * 1024;
  if (!isLarge) {
    const dawFile = await decodeAndAddAudioFile(file);
    if (dawFile) addFileToTimeline(dawFile, startTime, targetTrackId);
    return dawFile;
  }

  const fileId = crypto.randomUUID();
  const assetManifest = audioAssetManager.createAssetManifest(fileId, file);
  const meta = await readWavMetadata(file);
  const placeholder: DawFile = {
    id: fileId,
    name: file.name,
    mimeType: file.type,
    size: file.size,
    lastModified: file.lastModified,
    originalFileName: file.name,
    duration: meta?.duration ?? 1,
    sampleRate: meta?.sampleRate ?? 48000,
    channels: meta?.channels ?? 1,
    ...assetManifest,
    localObjectUrl: URL.createObjectURL(file),
  };

  const store = useProjectStore.getState();
  store.addFile(placeholder);
  store.setWaveformStatus(fileId, "loading");
  store.setWaveformProgress(fileId, 0);
  addFileToTimeline(placeholder, startTime, targetTrackId);
  showToast("Generating waveform...");

  try {
    const savedManifest = await audioAssetManager.saveImportedAudioAsset(fileId, file);
    const audioBuffer = await audioEngine.loadBuffer(
      placeholder,
      await file.arrayBuffer(),
      (fid, peaks) => useProjectStore.getState().setPeaks(fid, peaks),
      (fid, progress) => useProjectStore.getState().setWaveformProgress(fid, progress),
      (fid) => useProjectStore.getState().setWaveformStatus(fid, "error"),
    );
    const updates: Partial<DawFile> = {
      duration: audioBuffer.duration,
      sampleRate: audioBuffer.sampleRate,
      channels: audioBuffer.numberOfChannels,
      storageProvider: savedManifest.storageProvider,
      cacheKey: savedManifest.cacheKey,
      storageKey: savedManifest.storageKey,
      relativePath: savedManifest.relativePath,
    };
    useProjectStore.getState().updateFile(fileId, updates);
    for (const track of useProjectStore.getState().project.tracks) {
      for (const clip of track.clips) {
        if (clip.fileId === fileId && Math.abs(clip.duration - placeholder.duration) < 0.001) {
          useProjectStore.getState().updateClip(clip.id, { duration: audioBuffer.duration });
        }
      }
    }

    // Register project asset after successful folder-project copy.
    if (savedManifest.storageProvider === "project-folder" && savedManifest.relativePath) {
      const now = new Date().toISOString();
      const asset: DawProjectAsset = {
        id: fileId,
        type: "audio",
        name: file.name,
        originalName: file.name,
        relativePath: savedManifest.relativePath,
        size: file.size,
        durationSeconds: audioBuffer.duration,
        sampleRate: audioBuffer.sampleRate,
        channels: audioBuffer.numberOfChannels,
        mimeType: file.type || "audio/wav",
        createdAt: now,
        updatedAt: now,
      };
      useProjectStore.getState().addAsset(asset);
      // Backfill assetId on any clips already created from this file.
      for (const track of useProjectStore.getState().project.tracks) {
        for (const clip of track.clips) {
          if (clip.fileId === fileId && !clip.assetId) {
            useProjectStore.getState().updateClip(clip.id, { assetId: fileId });
          }
        }
      }
    }
  } catch (error) {
    console.warn("[Import] progressive audio import failed:", error);
    useProjectStore.getState().setWaveformStatus(fileId, "error");
  }

  return placeholder;
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

  // If the file is backed by a project asset, set assetId on the clip so the
  // native engine can resolve mediaPath via the asset manifest or DawFile map.
  const assetId = dawFile.id;

  const clip: DawClip = {
    id: clipId,
    name: dawFile.name.replace(/\.[^.]+$/, ""),
    fileId: dawFile.id,
    assetId,          // set when backed by a DawProjectAsset (folder project)
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
    await importAudioFileToTimelineProgressive(f, 0);
  }
}
