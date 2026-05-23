import { useEffect, useMemo } from "react";
import { MidiEditorPanel } from "../components/MidiEditorPanel";
import { ToastContainer } from "../components/ui/Toast";
import { useProjectStore } from "../store/projectStore";
import "../App.css";

export function ExternalPianoRollWindow() {
  const loadLocal = useProjectStore((s) => s.loadLocal);
  const project = useProjectStore((s) => s.project);

  const clipId = useMemo(() => {
    const params = new URLSearchParams(window.location.hash.split("?")[1] ?? "");
    return params.get("clipId") ?? "";
  }, []);

  useEffect(() => {
    loadLocal();
  }, [loadLocal]);

  // Re-sync when the main window saves to localStorage
  useEffect(() => {
    const onStorage = (e: StorageEvent) => {
      if (e.key === "mochi-daw-project" || e.key === null) {
        loadLocal();
      }
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, [loadLocal]);

  const clip = useMemo(
    () => project.tracks.flatMap((t) => t.clips).find((c) => c.id === clipId) ?? null,
    [project, clipId],
  );

  const track = useMemo(
    () => (clip ? project.tracks.find((t) => t.clips.some((c) => c.id === clip.id)) ?? null : null),
    [project, clip],
  );

  useEffect(() => {
    const name = clip?.name ?? "Piano Roll";
    document.title = `${name} – Futureboard`;
  }, [clip?.name]);

  if (!clip) {
    return (
      <div className="flex h-screen items-center justify-center bg-daw-bg text-daw-faint">
        <span className="text-[12px]">Clip not found</span>
      </div>
    );
  }

  return (
    <div className="flex h-screen min-h-0 flex-col overflow-hidden bg-daw-bg text-daw-text">
      <MidiEditorPanel clip={clip} track={track} />
      <ToastContainer />
    </div>
  );
}
