import type { DawFile, DawProjectAsset, FileId, WaveformPeaks } from "../types/daw";
import { audioAssetManager, type ImportedAudioAsset } from "./AudioAssetManager";
import { waveformCache, buildCacheKey, WAVEFORM_CACHE_VERSION } from "./waveformCache";
import { putChunk, getPeakCacheStats, CHUNK_PEAKS } from "./peakChunkCache";
import { writePeakChunk } from "./peakChunkStore";

/** Finest peak level requested from the worker; coarser levels are derived in-worker. */
export const PEAK_FINE_SPP = 256;
const DERIVED_PEAK_LEVELS = [32768, 16384, 8192, 4096, 2048, 1024, 512] as const;
import { platform } from "../platform";
import { useProjectStore } from "../store/projectStore";
import { addFileToTimeline, batchAddFilesToTimeline, isImportableAudioFile, readWavMetadata } from "../utils/importAudioToProject";
import { showToast } from "../components/ui/Toast";
import { useBackgroundTaskStore } from "../store/backgroundTaskStore";

export const IMPORT_LIMITS = {
  copyConcurrency: 1,
  metadataConcurrency: 2,
  peakConcurrency: 1,
  /** Never decode during import — decodes happen lazily on playback demand. */
  decodeConcurrency: 0,
  nativeSyncDebounceMs: 300,
  saveDebounceMs: 1500,
};

export type AudioImportQueueState =
  | "pending"
  | "copying"
  | "indexing"
  | "generating-peaks"
  | "ready"
  | "failed";

export type AudioImportJob = {
  id: string;
  fileId: FileId;
  fileName: string;
  size: number;
  state: AudioImportQueueState;
  error?: string;
  sourcePath?: string;
  createdAt: number;
  updatedAt: number;
};

type QueueSource = {
  file?: File;
  sourcePath?: string;
  name: string;
  size: number;
  lastModified?: number;
  mimeType?: string;
};

type TimelineTarget = {
  startTime?: number;
  trackId?: string;
};

type EnqueueOptions = TimelineTarget & {
  fileId?: FileId;
  importTaskId?: string;
  peakTaskId?: string;
  batchTotal?: number;
};

type Listener = () => void;

type WorkerMessage =
  | { type: "progress"; fileId: FileId; progress: number; samplesPerPeak: number }
  | { type: "peaks"; fileId: FileId; peaks: WaveformPeaks }
  | { type: "completed"; fileId: FileId }
  | { type: "error"; fileId: FileId; message: string };

class AudioImportQueue {
  private jobs = new Map<string, AudioImportJob>();
  private sources = new Map<string, QueueSource>();
  private targets = new Map<string, TimelineTarget>();
  private taskGroups = new Map<string, { importTaskId?: string; peakTaskId: string; total: number }>();
  private queue: string[] = [];
  private activeCopies = 0;
  private activePeaks = 0;
  private listeners = new Set<Listener>();
  private decodedBuffers = new Map<FileId, AudioBuffer>();
  private decodeQueue: Array<{
    file: DawFile;
    resolve: (buffer: AudioBuffer | null) => void;
  }> = [];
  private activeDecodes = 0;
  private peakWorkers = new Map<FileId, Worker>();
  private sourceTotalBytes = 0;

  subscribe(listener: Listener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  getJobs(): AudioImportJob[] {
    return [...this.jobs.values()].sort((a, b) => a.createdAt - b.createdAt);
  }

  getJob(fileId: FileId): AudioImportJob | undefined {
    return this.jobs.get(fileId);
  }

  get isImporting(): boolean {
    return this.activeCopies > 0 || this.activePeaks > 0 || this.queue.length > 0 || this.deferredPeakJobs.length > 0;
  }

  dumpQueue(): void {
    const jobs = this.getJobs();
    const byState: Record<string, number> = {};
    for (const j of jobs) byState[j.state] = (byState[j.state] ?? 0) + 1;
    console.group("[ImportQueue] dumpQueue");
    console.log("active copies:", this.activeCopies, "/ limit:", IMPORT_LIMITS.copyConcurrency);
    console.log("active peaks:", this.activePeaks, "/ limit:", IMPORT_LIMITS.peakConcurrency);
    console.log("pending queue:", this.queue.length);
    console.log("deferred peaks:", this.deferredPeakJobs.length);
    console.log("jobs by state:", byState);
    console.log("total jobs:", jobs.length);
    console.log("source total MB:", (this.sourceTotalBytes / 1024 / 1024).toFixed(1));
    for (const j of jobs) {
      console.log(`  [${j.state.padEnd(16)}] ${j.fileName}`);
    }
    console.groupEnd();
  }

  getDebugStats() {
    let decodedBytes = 0;
    for (const buffer of this.decodedBuffers.values()) {
      decodedBytes += buffer.length * buffer.numberOfChannels * 4;
    }
    const chunkStats = getPeakCacheStats();
    return {
      sourceTotalMB: this.sourceTotalBytes / 1024 / 1024,
      decodedBuffersCount: this.decodedBuffers.size,
      decodedBuffersMB: decodedBytes / 1024 / 1024,
      decodedBufferBytes: decodedBytes,
      peakCacheBytes: chunkStats.cacheBytes,
      loadedChunks: chunkStats.loadedChunks,
      evictions: chunkStats.evictions,
      canvasPixels: chunkStats.canvasPixels,
      importQueueLength: this.queue.length,
      importQueuePending: this.queue.length,
      importQueueActive: this.activeCopies,
      peakQueuePending: this.deferredPeakJobs.length,
      peakQueueActive: this.activePeaks,
      activeJobs: this.activeCopies + this.activePeaks + this.activeDecodes,
    };
  }

  enqueueFile(file: File, options: EnqueueOptions): DawFile | null {
    if (!isImportableAudioFile(file)) return null;
    const sourcePath = platform.fileSystem.getNativePathForFile(file) ?? undefined;
    const taskOptions = this.ensureTaskOptions(options, 1, file.name);
    return this.enqueueSource({
      file,
      sourcePath,
      name: file.name,
      size: file.size,
      lastModified: file.lastModified,
      mimeType: file.type,
    }, taskOptions);
  }

  async enqueueNativePath(path: string, options: EnqueueOptions): Promise<DawFile | null> {
    const stat = await platform.fileSystem.statAudioFile(path).catch(() => null);
    if (!stat) return null;
    const taskOptions = this.ensureTaskOptions(options, 1, stat.name);
    return this.enqueueSource({
      sourcePath: path,
      name: stat.name,
      size: stat.size,
      lastModified: stat.lastModified,
      mimeType: stat.mimeType,
    }, taskOptions);
  }

  enqueueFiles(files: File[], options: TimelineTarget): DawFile[] {
    const audioFiles = files.filter(isImportableAudioFile);
    if (audioFiles.length === 0) return [];
    const taskOptions = this.createBatchTasks(audioFiles.length);
    const imported: DawFile[] = [];
    for (const file of audioFiles) {
      // Skip timeline placement here — handled in batch below
      const placeholder = this.enqueueFile(file, taskOptions);
      if (!placeholder) continue;
      imported.push(placeholder);
    }
    if (imported.length === 0) return imported;
    if (options.startTime != null) {
      if (imported.length === 1) {
        addFileToTimeline(imported[0], options.startTime, options.trackId);
      } else {
        batchAddFilesToTimeline(imported, options.startTime, options.trackId);
      }
    }
    return imported;
  }

  enqueuePeakGenerationForFile(file: DawFile): boolean {
    if (this.jobs.has(file.id) || this.peakWorkers.has(file.id)) return false;
    const nativePath = file.cacheKey ?? file.storageKey;
    if (!nativePath) return false;

    const peakTaskId = useBackgroundTaskStore.getState().addTask({
      kind: "peak-generation",
      title: `Generating waveform for ${file.name}`,
      detail: `Queued ${file.name}`,
      status: "queued",
      progress: { current: 0, total: 1 },
      cancellable: false,
    });

    this.jobs.set(file.id, {
      id: crypto.randomUUID(),
      fileId: file.id,
      fileName: file.name,
      size: file.size ?? 0,
      sourcePath: nativePath,
      state: "pending",
      createdAt: Date.now(),
      updatedAt: Date.now(),
    });
    this.taskGroups.set(file.id, { peakTaskId, total: 1 });

    this.queuePeakJob(
      file.id,
      {
        sourcePath: nativePath,
        name: file.name,
        size: file.size ?? 0,
        lastModified: file.lastModified,
        mimeType: file.mimeType,
      },
      {
        storageProvider: file.storageProvider,
        storageKey: file.storageKey,
        cacheKey: file.cacheKey,
        waveformCacheKeys: file.waveformCacheKeys,
        size: file.size,
        lastModified: file.lastModified,
        originalFileName: file.originalFileName,
        relativePath: file.relativePath,
        name: file.name,
      },
      file.duration,
    );
    return true;
  }

  evictDecodedBuffer(fileId: FileId): void {
    this.decodedBuffers.delete(fileId);
  }

  async ensureDecodedBuffer(file: DawFile): Promise<AudioBuffer | null> {
    const cached = this.decodedBuffers.get(file.id);
    if (cached) return cached;
    return new Promise((resolve) => {
      this.decodeQueue.push({ file, resolve });
      this.pumpDecodeQueue();
    });
  }

  private enqueueSource(source: QueueSource, options: EnqueueOptions): DawFile {
    const fileId = options.fileId ?? crypto.randomUUID();
    const now = Date.now();
    const manifest = audioAssetManager.createAssetManifest(fileId, source);
    const placeholder: DawFile = {
      id: fileId,
      name: source.name,
      mimeType: source.mimeType || mimeFromName(source.name),
      size: source.size,
      lastModified: source.lastModified,
      originalFileName: source.name,
      duration: 1,
      sampleRate: 48000,
      channels: 1,
      ...manifest,
      localObjectUrl: undefined,
    };
    const store = useProjectStore.getState();
    store.addFile(placeholder);
    store.setWaveformStatus(fileId, "pending");
    store.setWaveformProgress(fileId, 0);
    if (options.startTime != null) {
      addFileToTimeline(placeholder, options.startTime, options.trackId);
    }

    this.jobs.set(fileId, {
      id: crypto.randomUUID(),
      fileId,
      fileName: source.name,
      size: source.size,
      sourcePath: source.sourcePath,
      state: "pending",
      createdAt: now,
      updatedAt: now,
    });
    this.sources.set(fileId, source);
    this.targets.set(fileId, options);
    const taskOptions = this.ensureTaskOptions(options, 1, source.name);
    this.taskGroups.set(fileId, {
      importTaskId: taskOptions.importTaskId,
      peakTaskId: taskOptions.peakTaskId,
      total: taskOptions.batchTotal,
    });
    this.queue.push(fileId);
    this.sourceTotalBytes += source.size;
    this.emit();
    this.pumpImportQueue();
    return placeholder;
  }

  private pumpImportQueue(): void {
    while (this.activeCopies < IMPORT_LIMITS.copyConcurrency && this.queue.length > 0) {
      const fileId = this.queue.shift();
      if (!fileId) return;
      this.activeCopies++;
      void this.processJob(fileId)
        .catch((error) => this.failJob(fileId, error))
        .finally(() => {
          this.activeCopies--;
          this.sources.delete(fileId);
          this.targets.delete(fileId);
          this.pumpImportQueue();
          this.emit();
        });
    }
  }

  private async processJob(fileId: FileId): Promise<void> {
    const source = this.sources.get(fileId);
    if (!source) return;
    this.setJobState(fileId, "copying");
    this.updateImportTask(fileId, "running", `Copying ${source.name}`);
    const savedManifest = await audioAssetManager.saveImportedAudioAsset(fileId, source);

    this.setJobState(fileId, "indexing");
    this.updateImportTask(fileId, "running", `Scanning ${source.name}`);
    const meta = await this.readMetadata(source, savedManifest);
    const current = useProjectStore.getState().project.files.find((f) => f.id === fileId);
    const duration = meta?.duration ?? current?.duration ?? 1;
    const updates: Partial<DawFile> = {
      ...savedManifest,
      duration,
      sampleRate: meta?.sampleRate ?? current?.sampleRate ?? 48000,
      channels: meta?.channels ?? current?.channels ?? 1,
      name: savedManifest.name ?? current?.name ?? source.name,
      size: savedManifest.size ?? current?.size ?? source.size,
      lastModified: savedManifest.lastModified ?? current?.lastModified ?? source.lastModified,
      mimeType: source.mimeType || current?.mimeType || mimeFromName(source.name),
      localObjectUrl: undefined,
    };
    useProjectStore.getState().updateFile(fileId, updates);
    this.updateClipsForFile(fileId, duration);
    this.registerAsset(fileId, source, savedManifest, meta);
    this.advanceParentTask(fileId, "import", source.name);

    const cached = await audioAssetManager.loadCachedWaveform({ ...(current ?? {}), id: fileId, ...updates } as DawFile);
    if (cached) {
      // loadCachedWaveform already put chunks into LRU and called setPeakMeta
      this.setJobState(fileId, "ready");
      this.advanceParentTask(fileId, "peak", source.name);
      return;
    }

    this.queuePeakJob(fileId, source, savedManifest, meta?.duration ?? duration);
  }

  private async readMetadata(source: QueueSource, manifest: ImportedAudioAsset) {
    if (source.file) return readWavMetadata(source.file);
    void manifest;
    return null;
  }

  private registerAsset(fileId: FileId, source: QueueSource, manifest: ImportedAudioAsset, meta: Awaited<ReturnType<typeof readWavMetadata>>) {
    if (manifest.storageProvider !== "project-folder" || !manifest.relativePath) return;
    const now = new Date().toISOString();
    const asset: DawProjectAsset = {
      id: fileId,
      type: "audio",
      name: manifest.name ?? source.name,
      originalName: source.name,
      relativePath: manifest.relativePath,
      size: manifest.size ?? source.size,
      durationSeconds: meta?.duration,
      sampleRate: meta?.sampleRate,
      channels: meta?.channels,
      mimeType: source.mimeType || mimeFromName(source.name),
      createdAt: now,
      updatedAt: now,
    };
    useProjectStore.getState().addAsset(asset);
  }

  private updateClipsForFile(fileId: FileId, duration: number): void {
    for (const track of useProjectStore.getState().project.tracks) {
      for (const clip of track.clips) {
        if (clip.fileId === fileId) {
          useProjectStore.getState().updateClip(clip.id, { duration, assetId: fileId });
        }
      }
    }
  }

  private queuePeakJob(fileId: FileId, source: QueueSource, manifest: ImportedAudioAsset, duration: number): void {
    const start = () => {
      this.activePeaks++;
      this.setJobState(fileId, "generating-peaks");
      this.updatePeakTask(fileId, "running", `Generating ${source.name}`);
      useProjectStore.getState().setWaveformStatus(fileId, "generating-peaks");
      this.runPeakWorker(fileId, source, manifest, duration)
        .then(() => this.advanceParentTask(fileId, "peak", source.name))
        .catch((error) => this.failJob(fileId, error))
        .finally(() => {
          this.activePeaks--;
          this.pumpDeferredPeakJobs();
          this.emit();
        });
    };
    this.updatePeakTask(fileId, "queued", `Queued ${source.name}`);
    this.deferredPeakJobs.push(start);
    this.pumpDeferredPeakJobs();
  }

  private deferredPeakJobs: Array<() => void> = [];

  private pumpDeferredPeakJobs(): void {
    while (this.activePeaks < IMPORT_LIMITS.peakConcurrency && this.deferredPeakJobs.length > 0) {
      const next = this.deferredPeakJobs.shift();
      next?.();
    }
  }

  /** Split WaveformPeaks into CHUNK_PEAKS-sized chunks, fill LRU, and fire-and-forget disk writes. */
  storePeakChunks(peaks: WaveformPeaks): void {
    const src = peaks.peaks as Int16Array;
    const totalPeaks = peaks.peakCount ?? Math.floor(src.length / (peaks.channelCount * 2));
    const chunkCount = Math.ceil(totalPeaks / CHUNK_PEAKS);
    for (let i = 0; i < chunkCount; i++) {
      const start = i * CHUNK_PEAKS * peaks.channelCount * 2;
      const end   = Math.min(start + CHUNK_PEAKS * peaks.channelCount * 2, src.length);
      const chunk = new Int16Array(src.buffer, src.byteOffset + start * 2, end - start);
      const copy  = new Int16Array(chunk); // own buffer — src may be transferred
      putChunk(peaks.fileId ?? "", peaks.samplesPerPeak, i, copy);
      void writePeakChunk(peaks.fileId ?? "", peaks.samplesPerPeak, i, copy);
    }
  }

  /** Write metadata-only waveformCache entry (peaks stripped) and update store. */
  registerPeakMeta(fileId: FileId, peaks: WaveformPeaks, duration: number): void {
    const totalPeaks = peaks.peakCount ?? 0;
    useProjectStore.getState().setPeakMeta(fileId, {
      spp:          peaks.samplesPerPeak,
      peakCount:    totalPeaks,
      channelCount: peaks.channelCount,
      sampleRate:   peaks.sampleRate  ?? 48000,
      duration:     peaks.duration    ?? duration,
    });
    // Persist metadata (empty peaks array) for cache-hit detection on next project open.
    void waveformCache.set(buildCacheKey(fileId, peaks.samplesPerPeak), {
      version:       WAVEFORM_CACHE_VERSION,
      fileId,
      sampleRate:    peaks.sampleRate  ?? 48000,
      channelCount:  peaks.channelCount,
      duration:      peaks.duration    ?? duration,
      samplesPerPeak: peaks.samplesPerPeak,
      peakCount:     totalPeaks,
      createdAt:     Date.now(),
      peaks:         new Int16Array(0), // no data — live in chunk files
    }).catch((e) => console.warn("[PeakMeta] waveformCache.set failed:", e));
  }

  private deriveCoarserPeakLevel(fine: WaveformPeaks, targetSpp: number): WaveformPeaks {
    const ratio = Math.max(1, Math.round(targetSpp / fine.samplesPerPeak));
    const src = fine.peaks as Int16Array;
    const srcPeakCount = fine.peakCount ?? Math.floor(src.length / (fine.channelCount * 2));
    const peakCount = Math.ceil(srcPeakCount / ratio);
    const result = new Int16Array(peakCount * fine.channelCount * 2);

    for (let i = 0; i < peakCount; i++) {
      for (let ch = 0; ch < fine.channelCount; ch++) {
        let lo = 32767;
        let hi = -32768;
        for (let j = 0; j < ratio; j++) {
          const k = i * ratio + j;
          if (k >= srcPeakCount) break;
          const base = (k * fine.channelCount + ch) * 2;
          if (src[base] < lo) lo = src[base];
          if (src[base + 1] > hi) hi = src[base + 1];
        }
        const out = (i * fine.channelCount + ch) * 2;
        result[out] = lo === 32767 ? 0 : lo;
        result[out + 1] = hi === -32768 ? 0 : hi;
      }
    }

    return {
      fileId: fine.fileId,
      samplesPerPeak: targetSpp,
      channelCount: fine.channelCount,
      peakCount,
      peaks: result,
      sampleRate: fine.sampleRate,
      duration: fine.duration,
      version: fine.version,
    };
  }

  private storeDerivedPeakLevels(fileId: FileId, fine: WaveformPeaks, duration: number): void {
    for (const targetSpp of DERIVED_PEAK_LEVELS) {
      const derived = this.deriveCoarserPeakLevel(fine, targetSpp);
      this.storePeakChunks(derived);
      this.registerPeakMeta(fileId, derived, duration);
    }
  }

  private async runPeakWorker(fileId: FileId, source: QueueSource, manifest: ImportedAudioAsset, duration: number): Promise<void> {
    const nativePath = manifest.cacheKey ?? manifest.storageKey ?? source.sourcePath;
    const nativePeaks = nativePath
      ? await platform.fileSystem.generateWavPeaks(nativePath, fileId, PEAK_FINE_SPP)
      : null;
    if (nativePeaks) {
      const peaks: WaveformPeaks = {
        fileId,
        samplesPerPeak: nativePeaks.samplesPerPeak,
        channelCount: nativePeaks.channelCount,
        peakCount: nativePeaks.peakCount,
        peaks: new Int16Array(nativePeaks.peaks),
        sampleRate: nativePeaks.sampleRate,
        duration: nativePeaks.duration,
        version: WAVEFORM_CACHE_VERSION,
      };
      useProjectStore.getState().updateFile(fileId, {
        duration: peaks.duration,
        sampleRate: peaks.sampleRate,
        channels: peaks.channelCount,
      });
      this.updateClipsForFile(fileId, peaks.duration ?? duration);
      this.storeDerivedPeakLevels(fileId, peaks, duration);
      this.storePeakChunks(peaks);
      this.registerPeakMeta(fileId, peaks, duration);
      this.setJobState(fileId, "ready");
      return;
    }

    const peakSource = source.file ?? null;
    if (!peakSource) {
      useProjectStore.getState().setWaveformStatus(fileId, "idle");
      this.setJobState(fileId, "ready");
      return;
    }
    await new Promise<void>((resolve, reject) => {
      const worker = new Worker(new URL("../workers/waveformWorker.ts", import.meta.url), { type: "module" });
      this.peakWorkers.set(fileId, worker);
      worker.onmessage = (e: MessageEvent<WorkerMessage>) => {
        if (e.data.type === "progress") {
          useProjectStore.getState().setWaveformProgress(fileId, e.data.progress);
          return;
        }
        if (e.data.type === "peaks") {
          useProjectStore.getState().updateFile(fileId, {
            duration: e.data.peaks.duration,
            sampleRate: e.data.peaks.sampleRate,
            channels: e.data.peaks.channelCount,
          });
          this.updateClipsForFile(fileId, e.data.peaks.duration ?? duration);
          this.storePeakChunks(e.data.peaks);
          this.registerPeakMeta(fileId, e.data.peaks, duration);
          return;
        }
        if (e.data.type === "completed") {
          this.setJobState(fileId, "ready");
          this.peakWorkers.delete(fileId);
          worker.terminate();
          resolve();
          return;
        }
        if (e.data.type === "error") {
          this.peakWorkers.delete(fileId);
          worker.terminate();
          reject(new Error(e.data.message));
        }
      };
      worker.onerror = () => {
        this.peakWorkers.delete(fileId);
        worker.terminate();
        reject(new Error("Waveform worker failed"));
      };
      worker.postMessage({
        fileId,
        source: peakSource,
        sampleRate: undefined,
        duration,
        // Worker derives all coarser levels from this fine scan; posts coarsest first.
        samplesPerPeakList: [PEAK_FINE_SPP],
      });
    });
  }

  private pumpDecodeQueue(): void {
    while (this.activeDecodes < IMPORT_LIMITS.decodeConcurrency && this.decodeQueue.length > 0) {
      const job = this.decodeQueue.shift();
      if (!job) return;
      this.activeDecodes++;
      void this.decodeFile(job.file)
        .then(job.resolve)
        .catch((error) => {
          console.warn("[AudioImportQueue] lazy decode failed:", error);
          job.resolve(null);
        })
        .finally(() => {
          this.activeDecodes--;
          this.pumpDecodeQueue();
          this.emit();
        });
    }
  }

  private async decodeFile(file: DawFile): Promise<AudioBuffer | null> {
    const AudioContextCtor = window.AudioContext || (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
    if (!AudioContextCtor) return null;
    const context = new AudioContextCtor();
    let sourceFile: File | null = null;
    if (file.storageProvider === "file-handle" || file.storageProvider === "project-folder") {
      const path = file.cacheKey ?? file.storageKey;
      if (path) sourceFile = await platform.fileSystem.readAudioFile(path).catch(() => null);
    }
    if (!sourceFile) return null;
    const buffer = await sourceFile.arrayBuffer();
    const decoded = await context.decodeAudioData(buffer);
    this.decodedBuffers.set(file.id, decoded);
    void context.close().catch(() => undefined);
    return decoded;
  }

  private setJobState(fileId: FileId, state: AudioImportQueueState): void {
    const job = this.jobs.get(fileId);
    if (job) {
      job.state = state;
      job.updatedAt = Date.now();
    }
    const waveformState = state === "failed" ? "error" : state === "ready" ? "ready" : state;
    useProjectStore.getState().setWaveformStatus(fileId, waveformState);
    this.emit();
  }

  private failJob(fileId: FileId, error: unknown): void {
    const job = this.jobs.get(fileId);
    const message = error instanceof Error ? error.message : String(error);
    if (job) {
      job.state = "failed";
      job.error = message;
      job.updatedAt = Date.now();
    }
    useProjectStore.getState().setWaveformStatus(fileId, "error");
    const group = this.taskGroups.get(fileId);
    if (group) {
      const failedInPeak = job?.state === "generating-peaks";
      const parentId = failedInPeak ? group.peakTaskId : group.importTaskId;
      useBackgroundTaskStore.getState().addTask({
        kind: failedInPeak ? "peak-generation" : "import",
        title: job?.fileName ?? (failedInPeak ? "Waveform failed" : "Audio import failed"),
        detail: message,
        status: "failed",
        error: message,
        parentId,
      });
      if (failedInPeak) this.advanceParentTask(fileId, "peak", job?.fileName ?? "audio", true);
      else {
        this.advanceParentTask(fileId, "import", job?.fileName ?? "audio", true);
        this.advanceParentTask(fileId, "peak", job?.fileName ?? "audio", true);
      }
    }
    if (!group || group.total === 1) showToast(`Could not import "${job?.fileName ?? "audio"}"`, true);
    this.emit();
  }

  private createBatchTasks(total: number, fileName?: string): BatchTaskOptions {
    const tasks = useBackgroundTaskStore.getState();
    const importTitle = total === 1 && fileName ? `Importing ${fileName}` : `Importing ${total} audio files`;
    const peakTitle = total === 1 && fileName ? `Generating waveform for ${fileName}` : "Generating waveform peaks";
    const importTaskId = tasks.addTask({
      kind: "import",
      title: importTitle,
      detail: total === 1 && fileName ? `Queued ${fileName}` : `Queued ${total} files`,
      status: "queued",
      progress: { current: 0, total },
      cancellable: false,
    });
    const peakTaskId = tasks.addTask({
      kind: "peak-generation",
      title: peakTitle,
      detail: "Waiting for import",
      status: "queued",
      progress: { current: 0, total },
      cancellable: false,
    });
    return { importTaskId, peakTaskId, batchTotal: total };
  }

  private ensureTaskOptions(options: EnqueueOptions, total: number, fileName?: string): EnqueueOptions & BatchTaskOptions {
    if (options.importTaskId && options.peakTaskId && options.batchTotal) {
      return options as EnqueueOptions & BatchTaskOptions;
    }
    return { ...options, ...this.createBatchTasks(total, fileName) };
  }

  private updateImportTask(fileId: FileId, status: "queued" | "running", detail: string): void {
    const group = this.taskGroups.get(fileId);
    if (!group?.importTaskId) return;
    useBackgroundTaskStore.getState().updateTask(group.importTaskId, { status, detail });
  }

  private updatePeakTask(fileId: FileId, status: "queued" | "running", detail: string): void {
    const group = this.taskGroups.get(fileId);
    if (!group) return;
    useBackgroundTaskStore.getState().updateTask(group.peakTaskId, { status, detail });
  }

  private advanceParentTask(fileId: FileId, kind: "import" | "peak", fileName: string, failed = false): void {
    const group = this.taskGroups.get(fileId);
    if (!group) return;
    const taskId = kind === "import" ? group.importTaskId : group.peakTaskId;
    if (!taskId) return;
    const task = useBackgroundTaskStore.getState().tasks[taskId];
    if (!task) return;
    const current = Math.min(group.total, taskProgressCurrent(taskId) + 1);
    const detail = failed ? `Failed ${fileName}` : kind === "import" ? `Imported ${fileName}` : `Generated ${fileName}`;
    useBackgroundTaskStore.getState().updateTask(taskId, {
      status: current >= group.total ? "complete" : "running",
      detail,
      progress: { current, total: group.total },
    });
    if (current >= group.total) {
      const failedCount = Object.values(useBackgroundTaskStore.getState().tasks).filter((t) => t.parentId === taskId && t.status === "failed").length;
      if (failedCount > 0) {
        showToast(`${failedCount} files failed to ${kind === "import" ? "import" : "generate waveforms"}`, true);
      } else {
        showToast(kind === "import" ? `Imported ${group.total} audio file${group.total === 1 ? "" : "s"}` : `Generated waveforms for ${group.total} clip${group.total === 1 ? "" : "s"}`);
      }
    }
  }

  private emit(): void {
    for (const listener of this.listeners) listener();
  }
}

type BatchTaskOptions = Required<Pick<EnqueueOptions, "importTaskId" | "peakTaskId" | "batchTotal">>;

function taskProgressCurrent(taskId: string): number {
  return useBackgroundTaskStore.getState().tasks[taskId]?.progress?.current ?? 0;
}

function mimeFromName(name: string): string {
  const lower = name.toLowerCase();
  if (lower.endsWith(".wav")) return "audio/wav";
  if (lower.endsWith(".mp3")) return "audio/mpeg";
  return "audio/*";
}

export const audioImportQueue = new AudioImportQueue();
