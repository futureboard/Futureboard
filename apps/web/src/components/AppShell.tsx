import { Timeline } from "./timeline/Timeline";
import { MixerPanel } from "./MixerPanel";
import { InspectorPanel } from "./InspectorPanel";
import { useUIStore } from "../store/uiStore";
import { BrowserPanel } from "./BrowserPanel";

export function AppShell({ onImport }: { onImport?: () => void }) {
  const { inspectorOpen, mixerOpen } = useUIStore();

  return (
    <div className="flex h-full flex-col overflow-hidden bg-daw-bg">
      <div className="flex min-h-0 flex-1 overflow-hidden">
        <BrowserPanel onImport={onImport} />
        <Timeline />
        {inspectorOpen && <InspectorPanel />}
      </div>
      {mixerOpen && <MixerPanel />}
    </div>
  );
}
