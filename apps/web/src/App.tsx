import { useEffect, useRef } from "react";
import { AppShell } from "./components/AppShell";
import { TransportBar } from "./components/TransportBar";
import { audioEngine } from "./engine/AudioEngine";
import { useProjectStore } from "./store/projectStore";
import { useKeyboardShortcuts } from "./hooks/useKeyboardShortcuts";
import { importAudioFilesAsNewTracks } from "./utils/importAudioToProject";
import "./App.css";

export default function App() {
  const { setPeaks, loadLocal, saveLocal, project } = useProjectStore();
  const fileInputRef = useRef<HTMLInputElement>(null);
  useKeyboardShortcuts();

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

  const handleImport = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files;
    if (!files || files.length === 0) return;
    await importAudioFilesAsNewTracks(Array.from(files));
    if (fileInputRef.current) fileInputRef.current.value = "";
  };

  return (
    <div className="flex h-full flex-col bg-daw-bg -space-y-[1px] text-daw-text">
      <input
        ref={fileInputRef}
        id="audio-import"
        type="file"
        accept=".wav,.mp3,audio/wav,audio/mpeg"
        multiple
        style={{ display: "none" }}
        onChange={handleImport}
      />

      <TransportBar
        onImport={() => fileInputRef.current?.click()}
        onSave={saveLocal}
      />

      <div className="min-h-0 flex-1  overflow-hidden">
        <AppShell onImport={() => fileInputRef.current?.click()} />
      </div>
    </div>
  );
}
