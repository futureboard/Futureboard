import { useRef } from "react";
import { ChevronDown } from "lucide-react";
import { useUIStore } from "../store/uiStore";
import type { BottomPanelTab } from "../store/uiStore";
import { MixerPanel } from "./MixerPanel";
import { EditorPanel } from "./EditorPanel";
import { EffectEditorRack } from "./EffectEditorRack";
import { DawIcon, type DawIconName } from "../icons/dawIcons";

type TabDef = {
  id: BottomPanelTab;
  label: string;
  icon: DawIconName;
};

const TABS: TabDef[] = [
  { id: "mixer",         label: "Mixer",         icon: "mixer" },
  { id: "editor",        label: "Editor",         icon: "editor" },
  { id: "effect-editor", label: "Effect Editor", icon: "effect" },
];

export function BottomWorkspacePanel({ height }: { height?: number }) {
  const { panels, setPanelLayout, togglePanel, bottomPanelTab, setBottomPanelTab } = useUIStore();
  const panelHeight = height ?? panels.mixer?.size ?? 300;

  // height resize — owns the grip for the whole bottom workspace
  const hDragRef = useRef<{ startY: number; startH: number } | null>(null);
  const onHeightDragStart = (e: React.PointerEvent<HTMLDivElement>) => {
    e.currentTarget.setPointerCapture(e.pointerId);
    hDragRef.current = { startY: e.clientY, startH: panelHeight };
  };
  const onHeightDrag = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!hDragRef.current) return;
    const newH = Math.max(160, Math.min(600, hDragRef.current.startH + hDragRef.current.startY - e.clientY));
    setPanelLayout("mixer", { size: newH });
  };
  const onHeightDragEnd = () => { hDragRef.current = null; };

  return (
    <div
      className="flex shrink-0 flex-col overflow-hidden border-t border-daw-border bg-[#111418]"
      style={{ height: panelHeight, minHeight: panelHeight }}
    >
      {/* height resize grip */}
      <div
        className="group flex h-[5px] shrink-0 cursor-ns-resize items-center justify-center"
        onPointerDown={onHeightDragStart}
        onPointerMove={onHeightDrag}
        onPointerUp={onHeightDragEnd}
      >
        <div className="h-[2px] w-8 rounded-full bg-white/[0.06] transition-colors group-hover:bg-white/25" />
      </div>

      {/* tab bar */}
      <div className="flex h-7 shrink-0 items-center gap-0.5 border-b border-white/[0.06] bg-[#0f1318] px-2">
        {TABS.map((t) => {
          const active = bottomPanelTab === t.id;
          return (
            <button
              key={t.id}
              type="button"
              onClick={() => setBottomPanelTab(t.id)}
              className={[
                "relative flex h-6 items-center gap-1.5 rounded-md px-2 text-[11px] font-medium transition-colors",
                active
                  ? "bg-white/[0.06] text-daw-text"
                  : "text-daw-faint hover:bg-white/[0.04] hover:text-daw-dim",
              ].join(" ")}
            >
              <DawIcon name={t.icon} size={11} className={active ? "text-daw-accent-h" : "opacity-80"} />
              <span>{t.label}</span>
              {active && (
                <span
                  className="absolute inset-x-1.5 -bottom-[1px] h-[1px]"
                  style={{ background: "var(--color-daw-accent-h, #62c2c2)" }}
                />
              )}
            </button>
          );
        })}

        <div className="flex-1" />

        <button
          onClick={() => togglePanel("mixer")}
          className="flex h-5 w-5 items-center justify-center rounded text-daw-faint transition-colors hover:bg-white/[0.05] hover:text-daw-text"
          title="Collapse bottom panel [M]"
        >
          <ChevronDown size={11} />
        </button>
      </div>

      {/* tab content */}
      <div className="flex min-h-0 flex-1 overflow-hidden">
        {bottomPanelTab === "mixer" && <MixerPanel embedded />}
        {bottomPanelTab === "editor" && <EditorPanel />}
        {bottomPanelTab === "effect-editor" && <EffectEditorRack />}
      </div>
    </div>
  );
}

