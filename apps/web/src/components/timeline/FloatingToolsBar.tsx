import { useEffect } from "react";
import { useUIStore, type ArrangementTool } from "../../store/uiStore";
import { DawIcon, type DawIconName } from "../../icons/dawIcons";
import { TIMELINE_Z } from "../../utils/timelineZ";

type ToolDef = {
  id: ArrangementTool;
  label: string;
  icon: DawIconName;
  shortcut: string;
};

const TOOLS: ToolDef[] = [
  { id: "pointer",    label: "Select",     icon: "pointer",    shortcut: "V" },
  { id: "pen",        label: "Draw",       icon: "pen",        shortcut: "P" },
  { id: "cut",        label: "Cut",        icon: "cut",        shortcut: "C" },
  { id: "glue",       label: "Glue",       icon: "glue",       shortcut: "G" },
  { id: "mute",       label: "Mute",       icon: "mute",       shortcut: "U" },
  { id: "time",       label: "Stretch",    icon: "time",       shortcut: "T" },
  { id: "automation", label: "Automation", icon: "automation", shortcut: "A" },
];

// Map shortcut key → tool id (lowercase for comparison)
const SHORTCUT_MAP: Record<string, ArrangementTool> = Object.fromEntries(
  TOOLS.map((t) => [t.shortcut.toLowerCase(), t.id])
);

export function FloatingToolsBar() {
  const { currentTool, setCurrentTool } = useUIStore();

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // Ignore when typing in an input / textarea
      const tag = (e.target as HTMLElement).tagName;
      if (tag === "INPUT" || tag === "TEXTAREA" || e.ctrlKey || e.metaKey || e.altKey) return;

      const tool = SHORTCUT_MAP[e.key.toLowerCase()];
      if (tool) {
        e.preventDefault();
        setCurrentTool(tool);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [setCurrentTool]);

  return (
    <div
      className="absolute bottom-4 left-4"
      style={{ zIndex: TIMELINE_Z.floatingTools }}
      // Prevent clicks from propagating to the timeline (selection etc.)
      onMouseDown={(e) => e.stopPropagation()}
    >
      <div
        className="flex items-center gap-px rounded-lg border px-1 py-1 shadow-2xl"
        style={{
          background: "#171b22",
          borderColor: "rgba(255,255,255,0.1)",
          boxShadow: "0 4px 24px rgba(0,0,0,0.45), 0 1px 0 rgba(255,255,255,0.04) inset",
        }}
      >
        {TOOLS.map((tool, i) => {
          const active = currentTool === tool.id;

          // Separator after "pointer" and after "glue"
          const hasSeparatorAfter = i === 0 || i === 3;

          return (
            <span key={tool.id} className="flex items-center">
              <button
                type="button"
                title={`${tool.label} [${tool.shortcut}]`}
                onClick={() => setCurrentTool(tool.id)}
                className="relative flex h-7 w-7 items-center justify-center rounded-md transition-colors"
                style={{
                  background: active ? "rgba(86,199,201,0.15)" : "transparent",
                  color: active
                    ? "#56c7c9"
                    : "rgba(180,192,204,0.55)",
                }}
                onMouseEnter={(e) => {
                  if (!active)
                    (e.currentTarget as HTMLElement).style.background = "rgba(255,255,255,0.05)";
                }}
                onMouseLeave={(e) => {
                  if (!active)
                    (e.currentTarget as HTMLElement).style.background = "transparent";
                }}
              >
                <DawIcon name={tool.icon} size={13} />
                {active && (
                  <span
                    className="absolute inset-x-1.5 -bottom-[1px] h-px rounded-full"
                    style={{ background: "#56c7c9", opacity: 0.8 }}
                  />
                )}
              </button>
              {hasSeparatorAfter && (
                <span
                  className="mx-1 h-4 w-px"
                  style={{ background: "rgba(255,255,255,0.08)" }}
                />
              )}
            </span>
          );
        })}
      </div>
    </div>
  );
}
