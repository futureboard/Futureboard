import { useEffect, useRef, useState } from "react";
import { AppShell } from "./components/AppShell";
import { TransportBar } from "./components/TransportBar";
import { CommandPalette } from "./components/ui/CommandPalette";
import { ContextMenu } from "./components/ui/ContextMenu";
import { WindowHost } from "./components/windows/WindowHost";
import { audioEngine } from "./engine/AudioEngine";
import { audioAssetManager } from "./engine/AudioAssetManager";
import { transport } from "./engine/Transport";
import { metronomeScheduler } from "./engine/MetronomeScheduler";
import { activeAudioEngine } from "./engine/activeAudioEngine";
import { useProjectStore } from "./store/projectStore";
import { useUIStore } from "./store/uiStore";
import { audioProcessingService } from "./audio/AudioProcessingService";
import { audioCacheManager } from "./audio/AudioCacheManager";
import { buildDecodedCacheKey } from "./audio/audioCacheKeys";
import { useMetronomeStore } from "./store/metronomeStore";
import { useKeyboardShortcuts } from "./hooks/useKeyboardShortcuts";
import { importAudioFilesAsNewTracks } from "./utils/importAudioToProject";
import { audioImportQueue, IMPORT_LIMITS } from "./engine/AudioImportQueue";
import { runAction } from "./menu/actionRunner";
import { platform } from "./platform";
import { audioDeviceService } from "./engine/AudioDeviceService";
import { midiDeviceService } from "./engine/MidiDeviceService";
import { ToastContainer } from "./components/ui/Toast";
import { PerfMonitor } from "./components/PerfMonitor";
import type { DawProject, InsertDevice } from "./types/daw";
import { closeNativeInsertEditor, isNativeVst3Insert, openNativeInsertEditor } from "./components/plugins/nativePluginEditorLifecycle";
import { useSettingsStore } from "./store/settingsStore";
import { useBackgroundTaskStore } from "./store/backgroundTaskStore";
import { useDragWorkflowStore } from "./store/dragWorkflowStore";
import { useRecentProjectsStore } from "./store/recentProjectsStore";
import {
  PROJECT_ACTION_CHANNEL,
  guardUnsavedProject,
  openProjectFromPath,
  rememberSavedProject,
  type ProjectActionMessage,
} from "./utils/projectLifecycle";
import { openProjectWizardWindow } from "./utils/dialogWindows";
import "./App.css";

// Wire engine modules to app-layer state — runs once at module load time.
// Engine modules stay store-free; this adapter is the only crossing point.
transport.setTrackGetter(() => useProjectStore.getState().project.tracks);
transport.setFileGetter(() => useProjectStore.getState().project.files);
transport.setPeaksCallback((fileId, peaks) => {
  audioImportQueue.storePeakChunks(peaks);
  audioImportQueue.registerPeakMeta(fileId, peaks, peaks.duration ?? 0);
});

metronomeScheduler.setConfigGetter(() => {
  const { project } = useProjectStore.getState();
  const metro = useMetronomeStore.getState();
  return {
    bpm: project.bpm,
    timeSignature: project.timeSignature,
    enabled: metro.enabled,
    volume: metro.volume,
    accentVolume: metro.accentVolume,
    sound: metro.sound,
    subdivision: metro.subdivision,
  };
});

// Route metronome beat sync through the active engine so it tracks the native
// Rust transport position instead of the WebAudio transport clock.
metronomeScheduler.setProjectTimeGetter(() => activeAudioEngine.projectTime);

function eachInsert(project: DawProject, visit: (trackId: string, insert: InsertDevice) => void): void {
  for (const track of project.tracks) {
    for (const insert of track.inserts ?? []) {
      visit(track.id, insert);
    }
  }
}

type PendingEditorOpen = {
  trackId: string;
  insert: InsertDevice;
};

function syncInsertDeltasToEngine(project: DawProject, previous: DawProject): PendingEditorOpen[] {
  const pendingEditorOpens: PendingEditorOpen[] = [];
  const previousByKey = new Map<string, { trackId: string; insert: InsertDevice }>();
  eachInsert(previous, (trackId, insert) => {
    previousByKey.set(`${trackId}:${insert.id}`, { trackId, insert });
  });

  eachInsert(project, (trackId, insert) => {
    const key = `${trackId}:${insert.id}`;
    const prev = previousByKey.get(key)?.insert;
    if (!prev) {
      activeAudioEngine.addInsertDevice(trackId, insert);
      if (isNativeVst3Insert(insert)) pendingEditorOpens.push({ trackId, insert });
      return;
    }

    if (prev.enabled !== insert.enabled) {
      activeAudioEngine.setInsertEnabled(trackId, insert.id, insert.enabled);
    }

    const prevParams = prev.params ?? {};
    const nextParams = insert.params ?? {};
    for (const [param, value] of Object.entries(nextParams)) {
      if (prevParams[param] !== value) {
        activeAudioEngine.setInsertParam(trackId, insert.id, param, value);
      }
    }
  });

  for (const [key, { trackId, insert }] of previousByKey) {
    const stillExists = project.tracks.some((track) =>
      key.startsWith(`${track.id}:`) && (track.inserts ?? []).some((next) => next.id === insert.id),
    );
    if (!stillExists) {
      activeAudioEngine.removeInsertDevice(trackId, insert.id);
      void closeNativeInsertEditor(trackId, insert.id);
    }
  }

  return pendingEditorOpens;
}

function getStartupRecentProjectPath(): string | null {
  if (platform.kind !== "electron" || !platform.folderProject.isSupported) return null;
  const recent = useRecentProjectsStore
    .getState()
    .recentProjects
    .filter((project) => project.source === "local" && project.storageMode === "folder" && project.projectFilePath)
    .sort((a, b) => b.lastOpenedAt - a.lastOpenedAt)[0];
  return recent?.projectFilePath ?? null;
}

export default function App() {
  const { setWaveformStatus, loadLocal, project } = useProjectStore();
  const [perfVisible, setPerfVisible] = useState(false);
  const startupHandledRef = useRef(false);
  useKeyboardShortcuts();

  useEffect(() => {
    return window.futureboard?.commands.onCommand((commandId) => {
      runAction(commandId);
    });
  }, []);

  useEffect(() => {
    if (typeof BroadcastChannel === "undefined") return;
    const channel = new BroadcastChannel(PROJECT_ACTION_CHANNEL);
    channel.onmessage = (event: MessageEvent<ProjectActionMessage>) => {
      const message = event.data;
      if (message?.type !== "openProjectPath" || typeof message.filePath !== "string") return;
      void guardUnsavedProject("open")
        .then((ok) => (ok ? openProjectFromPath(message.filePath) : false))
        .catch((error) => console.warn("[App] external project open:", error));
    };
    return () => channel.close();
  }, []);

  useEffect(() => {
    activeAudioEngine.init().catch(console.error);
    return () => activeAudioEngine.dispose();
  }, []);

  // Device services startup.
  // Electron: auto-scan immediately (Chromium usually grants silently).
  // Web: only listen for hotplug; user must explicitly grant permissions via UI.
  useEffect(() => {
    if (platform.kind === "electron") {
      // Try to get labeled device list; fall back to bare enumeration if denied.
      void audioDeviceService.requestAudioPermission().catch(() => {
        void audioDeviceService.refreshAudioDevices();
      });
      void midiDeviceService.requestMidiAccess();
    }
    audioDeviceService.listenForDeviceChanges();
    return () => audioDeviceService.stopListening();
  }, []);

  const handleImportClick = async () => {
    try {
      const files = await platform.fileSystem.pickAudioFiles();
      if (files.length === 0) return;
      await importAudioFilesAsNewTracks(files);
    } catch (e) {
      console.warn("[App] import audio:", e);
    }
  };

  const handleSaveProject = async () => {
    useUIStore.getState().setSaveStatus("saving");
    const taskId = useBackgroundTaskStore.getState().addTask({
      kind: "project-save",
      title: "Saving project",
      detail: "Writing project file",
      status: "running",
    });
    try {
      const result = await platform.projectStorage.saveProject(useProjectStore.getState().project);
      if (!result) {
        useUIStore.getState().setSaveStatus("unsaved");
        useBackgroundTaskStore.getState().completeTask(taskId, { detail: "Save cancelled" });
        return;
      }
      rememberSavedProject(useProjectStore.getState().project, result);
      useUIStore.getState().setSaveStatus("saved");
      useBackgroundTaskStore.getState().completeTask(taskId, { detail: "Project saved" });
    } catch (e) {
      console.warn("[App] save project:", e);
      useUIStore.getState().setSaveStatus("error");
      useBackgroundTaskStore.getState().failTask(taskId, e instanceof Error ? e.message : String(e));
    }
  };

  // Load startup project once. Electron folder projects must be opened by path
  // so waveform cache/chunk adapters receive the active project root.
  useEffect(() => {
    let cancelled = false;
    const startupBehavior = useSettingsStore.getState().startupBehavior;

    const openWizard = () => {
      if (startupHandledRef.current) return;
      startupHandledRef.current = true;
      void openProjectWizardWindow();
    };

    const restoreLocalProject = () => {
      if (startupBehavior === "newProject") {
        useProjectStore.getState().createNewProject();
      } else {
        loadLocal();
      }
      useUIStore.getState().setSaveStatus("saved");
    };

    if (startupBehavior === "lastProject") {
      const filePath = getStartupRecentProjectPath();
      if (filePath) {
        void openProjectFromPath(filePath)
          .then((opened) => {
            if (cancelled || opened) return;
            restoreLocalProject();
          })
          .catch((error) => {
            console.warn("[App] startup open recent:", error);
            if (!cancelled) restoreLocalProject();
          });
      } else {
        restoreLocalProject();
      }
    } else if (startupBehavior === "newProject") {
      // Start with a clean blank project immediately
      useProjectStore.getState().createNewProject();
      useUIStore.getState().setSaveStatus("saved");
    } else {
      // "wizard" — start with a blank canvas so the previous project
      // is never visible while the wizard is open.
      useProjectStore.getState().createNewProject();
      useUIStore.getState().setSaveStatus("saved");
      openWizard();
    }

    return () => {
      cancelled = true;
    };
  }, [loadLocal]);

  // Notify drag workflow when project data changes. Individual store mutations
  // call markDirty() directly, so we do NOT set "unsaved" here — that would
  // incorrectly fire during project loading and cause a false dirty state.
  useEffect(() => {
    return useProjectStore.subscribe((state, prev) => {
      if (state.project !== prev.project) {
        useDragWorkflowStore.getState().markProjectMutationDuringDrag();
      }
    });
  }, []);

  // Reload project when an external Electron window (Add Track, Mixer, etc.) writes to
  // localStorage. The storage event only fires in OTHER windows.
  // These cross-window edits are real user changes not yet saved to disk → mark unsaved.
  useEffect(() => {
    const onStorage = (e: StorageEvent) => {
      if (e.key === "mochi-daw-project" && e.newValue) {
        console.log("[App] storage event: syncing project from external window, marking unsaved");
        loadLocal();
        useUIStore.getState().setSaveStatus("unsaved");
      }
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, [loadLocal]);

  // Load the active audio backend whenever a different project is loaded.
  useEffect(() => {
    activeAudioEngine.loadProject(project).catch(console.error);
  }, [project.id]); // eslint-disable-line react-hooks/exhaustive-deps

  // Keep the active backend synced with edits.  Electron routes these snapshots
  // to Rust; browser keeps using the WebAudio adapter.
  // During mass import the subscription fires hundreds of times; debounce syncProject
  // so the native engine receives one snapshot after the burst settles.
  useEffect(() => {
    let syncTimer: ReturnType<typeof setTimeout> | null = null;
    let syncTaskId: string | null = null;

    const ensureSyncTask = (detail: string) => {
      if (!syncTaskId || !useBackgroundTaskStore.getState().tasks[syncTaskId]) {
        syncTaskId = useBackgroundTaskStore.getState().addTask({
          kind: "native-sync",
          title: "Native sync",
          detail,
          status: "queued",
        });
      } else {
        useBackgroundTaskStore.getState().updateTask(syncTaskId, { detail, status: "queued" });
      }
      return syncTaskId;
    };

    const runSync = (nextProject: DawProject) => {
      const taskId = ensureSyncTask("Applying project snapshot");
      useBackgroundTaskStore.getState().updateTask(taskId, { status: "running" });
      useDragWorkflowStore.getState().markNativeSyncDuringDrag();
      try {
        activeAudioEngine.syncProject(nextProject);
        useBackgroundTaskStore.getState().completeTask(taskId, { detail: "Native engine synced" });
      } catch (error) {
        useBackgroundTaskStore.getState().failTask(taskId, error instanceof Error ? error.message : String(error));
      } finally {
        syncTaskId = null;
      }
    };

    return useProjectStore.subscribe((state, prev) => {
      if (state.project !== prev.project) {
        const pendingEditorOpens = syncInsertDeltasToEngine(state.project, prev.project);
        const openPendingEditors = () => {
          for (const { trackId, insert } of pendingEditorOpens) {
            void openNativeInsertEditor(trackId, insert);
          }
        };

        if (audioImportQueue.isImporting) {
          ensureSyncTask("Waiting for import burst");
          if (syncTimer) clearTimeout(syncTimer);
          syncTimer = setTimeout(() => {
            syncTimer = null;
            runSync(useProjectStore.getState().project);
            openPendingEditors();
          }, IMPORT_LIMITS.nativeSyncDebounceMs);
        } else {
          if (syncTimer) {
            clearTimeout(syncTimer);
            syncTimer = null;
          }
          runSync(state.project);
          openPendingEditors();
        }
      }
    });
  }, []);

  // Ctrl+Shift+P toggles the performance/GPU monitor overlay.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key === "P") {
        e.preventDefault();
        setPerfVisible((v) => !v);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Block browser / OS page zoom (Ctrl/Cmd + wheel, pinch). Timeline keeps its own zoom via a non-passive wheel listener.
  useEffect(() => {
    const blockRootZoom = (e: WheelEvent) => {
      if (e.ctrlKey || e.metaKey) e.preventDefault();
    };
    window.addEventListener("wheel", blockRootZoom, { passive: false, capture: true });
    return () => window.removeEventListener("wheel", blockRootZoom, { capture: true });
  }, []);

  // Dev debug helper — only installed in development builds.
  useEffect(() => {
    if (import.meta.env.DEV) {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (window as any).__futureboardAudioDebug = {
        ...(window as any).__futureboardAudioDebug,
        testTsPitch: async (semitones = 12) => {
          const _semitones = semitones;
          const sine = new Float32Array(44100).map((_, i) => Math.sin(i * 0.01));
          const decoded = { fileId: "test", sampleRate: 44100, channels: 1, length: sine.length, duration: 1, channelData: [sine] };
          const { result } = await audioProcessingService.processClipAudio(decoded, { speedRatio: 1, pitchSemitones: _semitones, preservePitch: true, mode: "polyphonic", quality: "draft" });
          console.log(`[Debug] TS pitch ${_semitones}st → length ${result.length}`);
          return result;
        },
        testRustPitch: async (_semitones = 12) => {
          const { ensureRustDsp, isRustDspReady } = await import("./audio/RustDspProcessor");
          await ensureRustDsp();
          console.log("[Debug] Rust DSP ready:", isRustDspReady());
          return audioProcessingService.getProcessingCapabilities();
        },
        testProcessSelectedClip: async () => {
          const { project } = useProjectStore.getState();
          const { selectedClipIds } = useUIStore.getState();
          const clipId = selectedClipIds[0];
          if (!clipId) { console.warn("[Debug] No clip selected"); return; }
          const clip = project.tracks.flatMap((t) => t.clips).find((c) => c.id === clipId);
          if (!clip || !clip.audioProcess) { console.warn("[Debug] No audioProcess on clip"); return; }
          const loaded = audioEngine.getBuffer(clip.fileId);
          if (!loaded) { console.warn("[Debug] Buffer not loaded for", clip.fileId); return; }
          const key = buildDecodedCacheKey(clip.fileId, loaded.audioBuffer.sampleRate);
          const decoded = audioCacheManager.getDecodedAudio(key);
          if (!decoded) { console.warn("[Debug] No decoded audio in cache for", clip.fileId); return; }
          const params = { ...clip.audioProcess, mode: clip.audioProcess.mode ?? "polyphonic" as const };
          const { result } = await audioProcessingService.processClipAudio(decoded, params);
          console.log(`[Debug] Processed clip "${clip.name}": ${result.length} samples, ${result.duration.toFixed(3)}s`);
          return result;
        },
        cacheStats: () => audioCacheManager.getStats?.(),
        dumpQueue: () => audioImportQueue.dumpQueue(),
        importQueue: () => audioImportQueue.getDebugStats(),
      };
      console.debug("[Debug] window.__futureboardAudioDebug installed");
    }
  }, []);

  // Sync insert plugin chain into the audio engine whenever tracks or inserts change
  useEffect(() => {
    void activeAudioEngine.syncTrackInserts();
  }, [project.tracks, project.bpm]);

  // After project files are known, validate asset availability and hydrate cached peaks.
  useEffect(() => {
    audioAssetManager
      .restoreProjectAssets(project)
      .catch((e) => {
        console.warn("[App] restoreProjectAssets:", e);
        for (const file of project.files) setWaveformStatus(file.id, "error");
      });
  }, [project, setWaveformStatus]);

  return (
    <div className="flex h-full flex-col bg-daw-bg -space-y-[1px] text-daw-text">
      <TransportBar
        onImport={handleImportClick}
        onSave={handleSaveProject}
      />

      <div className="min-h-0 flex-1  overflow-hidden">
        <AppShell onImport={handleImportClick} />
      </div>
      <CommandPalette />
      <ContextMenu />
      <WindowHost />
      <ToastContainer />
      <PerfMonitor visible={perfVisible} />
    </div>
  );
}
