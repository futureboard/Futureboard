import type { DawFile, DawProject, FileId } from "../types/daw";
import type { PeakLevelMeta } from "../store/projectStore";
import { audioStorage } from "./AudioStorage";
import { audioEngine } from "./AudioEngine";
import {
  buildCacheKey,
  waveformCache,
  WAVEFORM_PEAK_LEVELS,
} from "./waveformCache";
import { platform } from "../platform";
import { useProjectStore } from "../store/projectStore";
import { putChunk } from "./peakChunkCache";
import { readPeakChunk } from "./peakChunkStore";
import { audioImportQueue } from "./AudioImportQueue";

// Electron exposes `__futureboardPath` (set by preload) on files picked via the
// native dialog, AND `path` (Electron File extension) on files dropped from the
// OS file manager.  Both are absolute filesystem paths.
type ElectronBackedFile = File & { __futureboardPath?: string; path?: string };

export type AudioAssetResolution =
  | { status: "available"; provider: NonNullable<DawFile["storageProvider"]> }
  | { status: "missing" };

export type ImportedAudioAsset = Pick<
  DawFile,
  "storageProvider" | "storageKey" | "cacheKey" | "waveformCacheKeys" | "size" | "lastModified" | "originalFileName" | "relativePath"
> & { name?: string };

type AudioImportSource = {
  name: string;
  size: number;
  lastModified?: number;
  type?: string;
  mimeType?: string;
  file?: File;
  sourcePath?: string;
};

class AudioAssetManager {
  private nativePathForFile(file: File): string | undefined {
    const f = file as ElectronBackedFile;
    const electronPath = f.path && f.path.length > 3 ? f.path : undefined;
    const bridgedPath = platform.fileSystem.getNativePathForFile(file) ?? undefined;
    return f.__futureboardPath ?? electronPath ?? bridgedPath;
  }

  createAssetManifest(fileId: FileId, file: File | AudioImportSource): ImportedAudioAsset {
    // Prefer the preload-injected path (picked via native dialog), then
    // Electron's File.path / webUtils.getPathForFile (drag-and-drop from OS).
    // Both are absolute filesystem paths. Ignore empty strings / null.
    const nativePath = "sourcePath" in file && file.sourcePath
      ? file.sourcePath
      : file instanceof File
        ? this.nativePathForFile(file)
        : undefined;
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

  async saveImportedAudioAsset(fileId: FileId, file: File | AudioImportSource): Promise<ImportedAudioAsset> {
    const manifest = this.createAssetManifest(fileId, file);
    const nativePath = "sourcePath" in file && file.sourcePath
      ? file.sourcePath
      : file instanceof File
        ? this.nativePathForFile(file)
        : undefined;

    if (nativePath && platform.folderProject.getProjectRoot()) {
      const imported = await platform.folderProject.importAudio(nativePath);
      if (imported) {
        return {
          ...manifest,
          name: imported.name,
          size: imported.size,
          lastModified: imported.lastModified,
          originalFileName: file.name,
          storageProvider: "project-folder",
          storageKey: imported.absolutePath,
          cacheKey: imported.absolutePath,
          relativePath: imported.relativePath,
        };
      }
    }

    if (manifest.storageProvider === "indexeddb") {
      const blob = file instanceof File ? file : file.file;
      if (blob) await audioStorage.save(fileId, blob);
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

  /**
   * Check whether disk-backed peak chunks exist for this file, load metadata
   * into Zustand, and pre-warm chunk 0 into the LRU for immediate display.
   * Returns true when a valid cache entry was found.
   */
  async loadCachedWaveform(file: DawFile): Promise<boolean> {
    const meta = await this.tryLoadWaveformMeta(file);
    if (!meta) return false;
    useProjectStore.getState().setPeakMeta(file.id, meta);
    return true;
  }

  /**
   * Like `loadCachedWaveform` but returns the metadata without touching the
   * Zustand store. Used to pre-warm metadata before `loadProject` clears it,
   * so the caller can re-apply immediately after.
   */
  async tryLoadWaveformMeta(file: DawFile): Promise<PeakLevelMeta | null> {
    const cacheKeys = (file.waveformCacheKeys?.length
      ? file.waveformCacheKeys
      : [...WAVEFORM_PEAK_LEVELS].reverse().map((level) => buildCacheKey(file.id, level)));

    for (const key of cacheKeys) {
      const entry = await waveformCache.get(key).catch(() => null);
      if (!entry) continue;

      // Verify chunk 0 actually exists on disk (metadata without chunks = stale).
      const chunk0 = await readPeakChunk(file.id, entry.samplesPerPeak, 0);
      if (!chunk0) continue;

      // Warm only chunk 0 into LRU for immediate render. Long files may have
      // many peak chunks; WaveformCanvas requests visible chunks on demand.
      putChunk(file.id, entry.samplesPerPeak, 0, chunk0);

      return {
        spp:          entry.samplesPerPeak,
        peakCount:    entry.peakCount,
        channelCount: entry.channelCount,
        sampleRate:   entry.sampleRate,
        duration:     entry.duration,
      };
    }

    return null;
  }

  async restoreProjectAssets(project: DawProject): Promise<void> {
    const store = useProjectStore.getState();
    for (const file of project.files) {
      const status = store.waveformStatus.get(file.id);
      if (status === "pending" || status === "copying" || status === "indexing" || status === "generating-peaks") {
        continue;
      }

      // Skip files that were pre-warmed by loadOpenedProject — they already
      // have peakMeta set and show the waveform. Only validate asset existence.
      const preWarmed = status === "ready";
      if (preWarmed) {
        const resolution = await this.resolveAudioAsset(file);
        if (resolution.status === "missing") store.setWaveformStatus(file.id, "missing");
        continue;
      }

      store.setWaveformStatus(file.id, "loading");

      const hasCached = await this.loadCachedWaveform(file);

      const resolution = await this.resolveAudioAsset(file);
      if (resolution.status === "missing") {
        store.setWaveformStatus(file.id, "missing");
      } else if (!hasCached) {
        const queued = audioImportQueue.enqueuePeakGenerationForFile(file);
        if (!queued) store.setWaveformStatus(file.id, "idle");
      }
    }

    // Update the project asset manifest with missing flags so the Browser
    // panel and native snapshot can show/skip missing assets.
    await this.validateProjectAssetManifest(project);
  }

  /**
   * For every entry in `project.assets`, check whether the file exists on disk
   * (Electron only) and update the `missing` flag in the store.
   * Runs in the background after project load — does not block UI.
   */
  async validateProjectAssetManifest(project: DawProject): Promise<void> {
    const assets = project.assets ?? [];
    if (assets.length === 0) return;
    const projectRoot = platform.folderProject.getProjectRoot();
    const store = useProjectStore.getState();

    for (const asset of assets) {
      if (!asset.relativePath) continue;
      let missing = false;
      try {
        const absPath = projectRoot
          ? `${projectRoot}/${asset.relativePath}`.replace(/\\/g, "/")
          : null;
        if (absPath) {
          const stat = await platform.fileSystem.statAudioFile(absPath).catch(() => null);
          missing = stat === null;
        }
        // If no projectRoot, we can't validate — leave as-is.
      } catch {
        missing = true;
      }
      if (missing !== (asset.missing ?? false)) {
        store.updateAsset(asset.id, { missing });
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
      (fid, peaks) => { audioImportQueue.storePeakChunks(peaks); audioImportQueue.registerPeakMeta(fid, peaks, peaks.duration ?? 0); },
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

    // Also update the DawProjectAsset entry so the Browser panel clears the
    // missing badge and the native snapshot can resolve the new mediaPath.
    if (manifest.storageProvider === "project-folder" && manifest.relativePath) {
      store.updateAsset(fileId, {
        relativePath: manifest.relativePath,
        missing: false,
        updatedAt: new Date().toISOString(),
      });
    } else if (manifest.storageProvider === "file-handle" || manifest.storageProvider === "indexeddb") {
      // Relinked to a non-project-folder location — just clear the missing flag.
      store.updateAsset(fileId, { missing: false });
    }

    return { ...existing, ...updates };
  }
}

export const audioAssetManager = new AudioAssetManager();
