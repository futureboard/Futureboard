import type { DawFile, DawProject, FileId, WaveformPeaks } from "../types/daw";
import { audioStorage } from "./AudioStorage";
import { audioEngine } from "./AudioEngine";
import {
  buildCacheKey,
  entryPeaksAsInt16,
  waveformCache,
  WAVEFORM_PEAK_LEVELS,
} from "./waveformCache";
import { platform } from "../platform";
import { useProjectStore } from "../store/projectStore";

type ElectronBackedFile = File & { __futureboardPath?: string };

export type AudioAssetResolution =
  | { status: "available"; provider: NonNullable<DawFile["storageProvider"]> }
  | { status: "missing" };

export type ImportedAudioAsset = Pick<
  DawFile,
  "storageProvider" | "storageKey" | "cacheKey" | "waveformCacheKeys" | "size" | "lastModified" | "originalFileName" | "relativePath"
>;

class AudioAssetManager {
  createAssetManifest(fileId: FileId, file: File): ImportedAudioAsset {
    const nativePath = (file as ElectronBackedFile).__futureboardPath;
    const storageKey = nativePath ?? `audio:${fileId}`;
    return {
      size: file.size,
      lastModified: file.lastModified,
      originalFileName: file.name,
      storageProvider: nativePath ? "file-handle" : "indexeddb",
      storageKey,
      cacheKey: storageKey,
      waveformCacheKeys: WAVEFORM_PEAK_LEVELS.map((level) => buildCacheKey(fileId, level)),
    };
  }

  async saveImportedAudioAsset(fileId: FileId, file: File): Promise<ImportedAudioAsset> {
    const nativePath = (file as ElectronBackedFile).__futureboardPath;
    const projectRoot = platform.folderProject.getProjectRoot();

    // Folder project mode: copy the source file into Media/Audio/
    if (projectRoot && nativePath && platform.folderProject.isSupported) {
      const result = await platform.folderProject.importAudio(nativePath);
      if (result) {
        return {
          size: file.size,
          lastModified: file.lastModified,
          originalFileName: file.name,
          storageProvider: "project-folder",
          storageKey: result.absolutePath,
          cacheKey: result.absolutePath,
          relativePath: result.relativePath,
          waveformCacheKeys: WAVEFORM_PEAK_LEVELS.map((level) => buildCacheKey(fileId, level)),
        };
      }
    }

    const manifest = this.createAssetManifest(fileId, file);
    if (manifest.storageProvider === "indexeddb") {
      await audioStorage.save(fileId, file);
    }
    return manifest;
  }

  async resolveAudioAsset(file: DawFile): Promise<AudioAssetResolution> {
    // Folder project: asset lives at an absolute path stored in cacheKey
    if (file.storageProvider === "project-folder" && file.cacheKey) {
      const stat = await platform.fileSystem.statAudioFile(file.cacheKey).catch(() => null);
      if (stat) return { status: "available", provider: "project-folder" };
      return { status: "missing" };
    }

    if (await audioStorage.has(file.id)) {
      return { status: "available", provider: "indexeddb" };
    }

    if (file.storageProvider === "file-handle" && file.cacheKey) {
      const stat = await platform.fileSystem.statAudioFile(file.cacheKey).catch(() => null);
      if (stat) return { status: "available", provider: "file-handle" };
    }

    return { status: "missing" };
  }

  async loadCachedWaveform(file: DawFile): Promise<WaveformPeaks | null> {
    const cacheKeys = (file.waveformCacheKeys?.length
      ? file.waveformCacheKeys
      : [...WAVEFORM_PEAK_LEVELS].reverse().map((level) => buildCacheKey(file.id, level)));

    for (const key of cacheKeys) {
      const cached = await waveformCache.get(key).catch(() => null);
      if (!cached) continue;
      return {
        fileId: file.id,
        samplesPerPeak: cached.samplesPerPeak,
        channelCount: cached.channelCount,
        peakCount: cached.peakCount,
        peaks: entryPeaksAsInt16(cached),
        sampleRate: cached.sampleRate,
        duration: cached.duration,
        version: cached.version,
      };
    }

    return null;
  }

  async restoreProjectAssets(project: DawProject): Promise<void> {
    const store = useProjectStore.getState();
    for (const file of project.files) {
      store.setWaveformStatus(file.id, "loading");

      const cachedPeaks = await this.loadCachedWaveform(file);
      if (cachedPeaks) {
        store.setPeaks(file.id, cachedPeaks);
      }

      const resolution = await this.resolveAudioAsset(file);
      if (resolution.status === "missing") {
        store.setWaveformStatus(file.id, "missing");
      } else if (!cachedPeaks) {
        store.setWaveformStatus(file.id, "idle");
      }
    }
  }

  async relinkMissingAsset(fileId: FileId, file: File): Promise<DawFile | null> {
    const store = useProjectStore.getState();
    const existing = store.project.files.find((f) => f.id === fileId);
    if (!existing) return null;

    const manifest = await this.saveImportedAudioAsset(fileId, file);
    store.setWaveformStatus(fileId, "loading");
    store.setWaveformProgress(fileId, 0);

    const audioBuffer = await audioEngine.loadBuffer(
      {
        ...existing,
        name: file.name,
        mimeType: file.type,
        ...manifest,
      },
      await file.arrayBuffer(),
      (fid, peaks) => useProjectStore.getState().setPeaks(fid, peaks),
      (fid, progress) => useProjectStore.getState().setWaveformProgress(fid, progress),
      (fid) => useProjectStore.getState().setWaveformStatus(fid, "error"),
    );

    const updates: Partial<DawFile> = {
      ...manifest,
      name: file.name,
      mimeType: file.type,
      duration: audioBuffer.duration,
      sampleRate: audioBuffer.sampleRate,
      channels: audioBuffer.numberOfChannels,
      localObjectUrl: URL.createObjectURL(file),
    };

    store.updateFile(fileId, updates);
    return { ...existing, ...updates };
  }
}

export const audioAssetManager = new AudioAssetManager();
