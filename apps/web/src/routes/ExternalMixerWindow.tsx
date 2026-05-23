import { useEffect, useState } from "react";
import { MixerPanel } from "../components/MixerPanel";
import { ToastContainer } from "../components/ui/Toast";
import { useProjectStore } from "../store/projectStore";
import { useUIStore } from "../store/uiStore";
import "../App.css";

function useViewportHeight() {
  const [height, setHeight] = useState(() => window.innerHeight);

  useEffect(() => {
    const onResize = () => setHeight(window.innerHeight);
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  return height;
}

export function ExternalMixerWindow() {
  const loadLocal = useProjectStore((state) => state.loadLocal);
  const projectName = useProjectStore((state) => state.project.name);
  const height = useViewportHeight();

  useEffect(() => {
    loadLocal();
    useUIStore.getState().setFocusedPanel("mixer");
    useUIStore.getState().setPanelLayout("mixer", {
      visible: true,
      dock: "bottom",
      size: Math.max(360, window.innerHeight),
    });
  }, [loadLocal]);

  useEffect(() => {
    document.title = `Mixer - ${projectName || "Futureboard"}`;
  }, [projectName]);

  return (
    <div className="flex h-screen min-h-0 flex-col overflow-hidden bg-daw-bg text-daw-text">
      <MixerPanel embedded externalWindow height={height} />
      <ToastContainer />
    </div>
  );
}
