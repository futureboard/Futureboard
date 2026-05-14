import { useEffect } from "react";
import { AppShell } from "./components/AppShell";
import { TransportBar } from "./components/TransportBar";
import { CommandPalette } from "./components/ui/CommandPalette";
import { ContextMenu } from "./components/ui/ContextMenu";
import { audioEngine } from "./engine/AudioEngine";
import { transport } from "./engine/Transport";
import { metronomeScheduler } from "./engine/MetronomeScheduler";
import { useProjectStore } from "./store/projectStore";
import { useMetronomeStore } from "./store/metronomeStore";
import { useKeyboardShortcuts } from "./hooks/useKeyboardShortcuts";
import { importAudioFilesAsNewTracks } from "./utils/importAudioToProject";
import { platform } from "./platform";
import { ToastContainer } from "./components/ui/Toast";
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
    try {
      await platform.projectStorage.saveProject(useProjectStore.getState().project);
    } catch (e) {
      console.warn("[App] save project:", e);
    }
  };

  // Load saved project metadata from localStorage on mount
  useEffect(() => {
    loadLocal();
  }, [loadLocal]);

  // Block browser / OS page zoom (Ctrl/Cmd + wheel, pinch). Timeline keeps its own zoom via a non-passive wheel listener.
  useEffect(() => {
    const blockRootZoom = (e: WheelEvent) => {
      if (e.ctrlKey || e.metaKey) e.preventDefault();
    };
    window.addEventListener("wheel", blockRootZoom, { passive: false, capture: true });
    return () => window.removeEventListener("wheel", blockRootZoom, { capture: true });
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
      <ToastContainer />
    </div>
  );
}
