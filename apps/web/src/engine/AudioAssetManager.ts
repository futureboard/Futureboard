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
>;

class AudioAssetManager {
  private nativePathForFile(file: File): string | undefined {
    const f = file as ElectronBackedFile;
    const electronPath = f.path && f.path.length > 3 ? f.path : undefined;
    const bridgedPath = platform.fileSystem.getNativePathForFile(file) ?? undefined;
    return f.__futureboardPath ?? electronPath ?? bridgedPath;
  }

  createAssetManifest(fileId: FileId, file: File): ImportedAudioAsset {
    // Prefer the preload-injected path (picked via native dialog), then
    // Electron's File.path / webUtils.getPathForFile (drag-and-drop from OS).
    // Both are absolute filesystem paths. Ignore empty strings / null.
    const nativePath = this.nativePathForFile(file);
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
