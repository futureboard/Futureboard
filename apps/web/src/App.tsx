import { useEffect } from "react";
import { AppShell } from "./components/AppShell";
import { TransportBar } from "./components/TransportBar";
import { CommandPalette } from "./components/ui/CommandPalette";
import { ContextMenu } from "./components/ui/ContextMenu";
import { WindowHost } from "./components/windows/WindowHost";
import { audioEngine } from "./engine/AudioEngine";
import { transport } from "./engine/Transport";
import { metronomeScheduler } from "./engine/MetronomeScheduler";
import { webAudioEngineAdapter } from "./engine/WebAudioEngineAdapter";
import { useProjectStore } from "./store/projectStore";
import { useUIStore } from "./store/uiStore";
import { audioProcessingService } from "./audio/AudioProcessingService";
import { audioCacheManager } from "./audio/AudioCacheManager";
import { buildDecodedCacheKey } from "./audio/audioCacheKeys";
import { useMetronomeStore } from "./store/metronomeStore";
import { useKeyboardShortcuts } from "./hooks/useKeyboardShortcuts";
import { importAudioFilesAsNewTracks } from "./utils/importAudioToProject";
import { platform } from "./platform";
import { ToastContainer } from "./components/ui/Toast";
import { useRecentProjectsStore } from "./store/recentProjectsStore";
import "./App.css";

// Wire engine modules to app-layer state — runs once at module load time.
// Engine modules stay store-free; this adapter is the only crossing point.
transport.setTrackGetter(() => useProjectStore.getState().project.tracks);

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

export default function App() {
  const { setPeaks, loadLocal, project } = useProjectStore();
  useKeyboardShortcuts();

  useEffect(() => {
    webAudioEngineAdapter.init().catch(console.error);
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
    try {
      await platform.projectStorage.saveProject(useProjectStore.getState().project);
      useUIStore.getState().setSaveStatus("saved");
    } catch (e) {
      console.warn("[App] save project:", e);
      useUIStore.getState().setSaveStatus("error");
    }
  };

  // Load saved project metadata from localStorage on mount, then mark as saved.
  useEffect(() => {
    loadLocal();
    useUIStore.getState().setSaveStatus("saved");
    // Register the loaded project as a recent entry
    const { project: loaded } = useProjectStore.getState();
    useRecentProjectsStore.getState().addRecentProject({
      id: loaded.id,
      name: loaded.name,
      source: "browser",
    });
  }, [loadLocal]);

  // Mark status bar "unsaved" whenever the project data actually changes.
  // Using subscribe (not a hook) so this never causes a re-render of App.
  useEffect(() => {
    return useProjectStore.subscribe((state, prev) => {
      if (state.project !== prev.project) {
        useUIStore.getState().setSaveStatus("unsaved");
      }
    });
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
      };
      console.debug("[Debug] window.__futureboardAudioDebug installed");
    }
  }, []);

  // After project files are known, restore their AudioBuffers from IndexedDB
  useEffect(() => {
    for (const file of project.files) {
      if (audioEngine.getBuffer(file.id)) continue;   // already in memory
      audioEngine
        .restoreBuffer(file, (fid, peaks) => setPeaks(fid, peaks))
        .catch((e) => console.warn("[App] restoreBuffer:", e));
    }
  }, [project.files, setPeaks]);

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
    </div>
  );
}
