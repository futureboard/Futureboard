import { useEffect, useState } from "react";
import { AppShell } from "./components/AppShell";
import { TransportBar } from "./components/TransportBar";
import { CommandPalette } from "./components/ui/CommandPalette";
import { ContextMenu } from "./components/ui/ContextMenu";
import { WindowHost } from "./components/windows/WindowHost";
import { audioEngine } from "./engine/AudioEngine";
import { audioAssetManager } from "./engine/AudioAssetManager";
import { mixer } from "./engine/Mixer";
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
import { audioDeviceService } from "./engine/AudioDeviceService";
import { midiDeviceService } from "./engine/MidiDeviceService";
import { ToastContainer } from "./components/ui/Toast";
import { PerfMonitor } from "./components/PerfMonitor";
import { useRecentProjectsStore } from "./store/recentProjectsStore";
import "./App.css";

// Wire engine modules to app-layer state — runs once at module load time.
// Engine modules stay store-free; this adapter is the only crossing point.
transport.setTrackGetter(() => useProjectStore.getState().project.tracks);
transport.setFileGetter(() => useProjectStore.getState().project.files);
transport.setPeaksCallback((fileId, peaks) => useProjectStore.getState().setPeaks(fileId, peaks));

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
  const { setWaveformStatus, loadLocal, project } = useProjectStore();
  const [perfVisible, setPerfVisible] = useState(false);
  useKeyboardShortcuts();

  useEffect(() => {
    webAudioEngineAdapter.init().catch(console.error);
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

  // Re-sync the audio engine whenever a different project is loaded.
  // project.id changes only on load/new-project, not on every edit.
  useEffect(() => {
    webAudioEngineAdapter.loadProject(project).catch(console.error);
  }, [project.id]); // eslint-disable-line react-hooks/exhaustive-deps

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

  // Sync insert plugin chain into the audio engine whenever tracks or inserts change
  useEffect(() => {
    const bpm = project.bpm ?? 120;
    for (const track of project.tracks) {
      if (track.inserts && track.inserts.length > 0) {
        mixer.syncTrackInserts(track.id, track.inserts, bpm);
      }
    }
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
