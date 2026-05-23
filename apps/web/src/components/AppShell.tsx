import { Timeline } from "./timeline/Timeline";
import { BottomWorkspacePanel } from "./BottomWorkspacePanel";
import { InspectorPanel } from "./InspectorPanel";
import { useUIStore } from "../store/uiStore";
import { BrowserPanel } from "./BrowserPanel";
import { StatusBar } from "./StatusBar";

export function AppShell({ onImport }: { onImport?: () => void }) {
  const { panels } = useUIStore();

  const leftPanels = Object.values(panels).filter(p => p.visible && p.dock === "left");
  const rightPanels = Object.values(panels).filter(p => p.visible && p.dock === "right");
  const bottomPanels = Object.values(panels).filter(p => p.visible && p.dock === "bottom");

  return (
    <div className="flex h-full flex-col -space-y-[1px] overflow-hidden bg-daw-bg">
      <div className="flex min-h-0 flex-1 -space-x-[1px] overflow-hidden">
        {leftPanels.map(p => {
          if (p.id === "browser") return <BrowserPanel key={p.id} onImport={onImport} width={p.size} />;
          return null;
        })}
        
        <Timeline />
        
        {rightPanels.map(p => {
          if (p.id === "inspector") return <InspectorPanel key={p.id} width={p.size} />;
          return null;
        })}
      </div>
      
      {bottomPanels.map(p => {
        if (p.id === "mixer") return <BottomWorkspacePanel key={p.id} height={p.size} />;
        return null;
      })}

      <StatusBar />
    </div>
  );
}
