import { useEffect, useState, useRef, useMemo } from "react";
import { Search } from "lucide-react";
import { useUIStore } from "../../store/uiStore";
import { flattenMenuItems } from "../../menu/menuHelper";
import { runAction } from "../../menu/actionRunner";

export function CommandPalette() {
  const { commandPaletteOpen, setCommandPaletteOpen } = useUIStore();
  const [query, setQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const allCommands = useMemo(() => flattenMenuItems(), []);

  const filteredCommands = useMemo(() => {
    if (!query) return allCommands;
    const lowerQuery = query.toLowerCase();
    return allCommands.filter(
      (cmd) =>
        cmd.label.toLowerCase().includes(lowerQuery) ||
        cmd.group.toLowerCase().includes(lowerQuery) ||
        (cmd.action && cmd.action.toLowerCase().includes(lowerQuery))
    );
  }, [allCommands, query]);

  useEffect(() => {
    if (commandPaletteOpen) {
      setQuery("");
      setSelectedIndex(0);
      setTimeout(() => inputRef.current?.focus(), 10);
    }
  }, [commandPaletteOpen]);

  useEffect(() => {
    setSelectedIndex(0);
  }, [query]);

  // Handle keyboard navigation
  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSelectedIndex((prev) => (prev + 1) % filteredCommands.length);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSelectedIndex((prev) => (prev - 1 + filteredCommands.length) % filteredCommands.length);
    } else if (e.key === "Enter") {
      e.preventDefault();
      const cmd = filteredCommands[selectedIndex];
      if (cmd && cmd.enabled !== false) {
        runAction(cmd.action);
      }
    } else if (e.key === "Escape") {
      e.preventDefault();
      setCommandPaletteOpen(false);
    }
  };

  useEffect(() => {
    // Scroll selected into view
    const list = listRef.current;
    if (list) {
      const selectedEl = list.children[selectedIndex] as HTMLElement;
      if (selectedEl) {
        selectedEl.scrollIntoView({ block: "nearest" });
      }
    }
  }, [selectedIndex]);

  if (!commandPaletteOpen) return null;

  return (
    <div
      className="fixed inset-0 z-[999] flex items-start justify-center bg-transparent px-4 pt-[15vh] app-no-drag"
      onMouseDown={() => setCommandPaletteOpen(false)}
    >
      <div 
        className="flex w-full max-w-[600px] flex-col overflow-hidden rounded-xl border border-white/[0.08] bg-[#1a1e26] shadow-[0_1px_0_rgba(255,255,255,0.05)_inset,0_0_0_1px_rgba(0,0,0,0.52),0_18px_44px_rgba(0,0,0,0.46),0_44px_120px_rgba(0,0,0,0.42)]"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className="flex h-11 items-center border-b border-white/[0.06] px-4">
          <Search size={15} className="mr-3 text-daw-faint" />
          <input
            ref={inputRef}
            className="flex-1 bg-transparent text-[12px] font-medium text-daw-text outline-none placeholder:text-daw-faint"
            placeholder="Type a command or search..."
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={handleKeyDown}
          />
        </div>
        
        <div ref={listRef} className="max-h-[60vh] overflow-y-auto p-2 outline-none">
          {filteredCommands.length === 0 ? (
            <div className="py-8 text-center text-[12px] text-daw-faint">No commands found.</div>
          ) : (
            filteredCommands.map((cmd, idx) => {
              const selected = idx === selectedIndex;
              const disabled = cmd.enabled === false;
              
              return (
                <div
                  key={cmd.id}
                  className={[
                    "flex cursor-pointer items-center justify-between rounded-lg px-3 py-2 text-[12px]",
                    selected ? "bg-daw-accent text-white" : "text-daw-text",
                    disabled ? "opacity-50" : "hover:bg-daw-surface-high",
                  ].join(" ")}
                  onClick={() => {
                    if (!disabled) runAction(cmd.action);
                  }}
                  onMouseEnter={() => setSelectedIndex(idx)}
                >
                  <div className="flex min-w-0 flex-col">
                    <span className={`truncate font-semibold leading-4 ${cmd.danger && !selected ? "text-daw-red" : ""}`}>
                      {cmd.label}
                    </span>
                    <span className={`truncate text-[10px] leading-4 ${selected ? "text-white/70" : "text-daw-faint"}`}>
                      {cmd.group}
                    </span>
                  </div>
                  {cmd.accelerator && (
                    <div className={`ml-4 shrink-0 rounded px-1.5 py-0.5 text-[9px] font-semibold tracking-wide ${selected ? "bg-black/20 text-white" : "bg-daw-surface-high text-daw-faint"}`}>
                      {cmd.accelerator}
                    </div>
                  )}
                </div>
              );
            })
          )}
        </div>
      </div>
    </div>
  );
}
