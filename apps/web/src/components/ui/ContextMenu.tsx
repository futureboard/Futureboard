import { useEffect, useRef } from "react";
import { useUIStore } from "../../store/uiStore";
import { runAction } from "../../menu/actionRunner";
import type { AppMenuItem } from "../../menu/menuItems";

function MenuItem({ item, onClose }: { item: AppMenuItem; onClose: () => void }) {
  if (item.type === "separator") {
    return <div className="mx-2 my-1 h-[1px] bg-white/[0.05]" />;
  }

  if (item.type === "submenu") {
    // Nested context menus are complex; just rendering it differently for now, 
    // or we can build a nested popover mechanism later.
    return (
      <div className="group relative">
        <button
          className="flex w-full items-center justify-between rounded px-3 py-1.5 text-left text-[12px] font-medium text-daw-text hover:bg-daw-accent hover:text-white"
        >
          <span>{item.label}</span>
          <span className="text-daw-faint group-hover:text-white/70">▶</span>
        </button>
      </div>
    );
  }

  const disabled = item.enabled === false;

  return (
    <button
      className={[
        "flex w-full items-center justify-between rounded px-3 py-1.5 text-left text-[12px] font-medium transition-colors",
        disabled
          ? "cursor-default text-daw-faint opacity-50"
          : "text-daw-text hover:bg-daw-accent hover:text-white",
        item.danger && !disabled ? "text-daw-red hover:bg-daw-red" : "",
      ].join(" ")}
      onClick={() => {
        if (disabled) return;
        if (item.action) runAction(item.action);
        onClose();
      }}
    >
      <span className="truncate">{item.label}</span>
      {item.accelerator && (
        <span className="ml-4 shrink-0 text-[10px] tracking-wide text-daw-dim opacity-70">
          {item.accelerator}
        </span>
      )}
    </button>
  );
}

export function ContextMenu() {
  const { contextMenuOpen, contextMenuPosition, contextMenuItems, setContextMenu } = useUIStore();
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!contextMenuOpen) return;

    const onPointerDown = (e: PointerEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setContextMenu(false);
      }
    };
    
    // Close on escape
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setContextMenu(false);
      }
    };

    // Use capturing phase so we can close before other elements handle the click
    window.addEventListener("pointerdown", onPointerDown, { capture: true });
    window.addEventListener("keydown", onKeyDown);
    
    return () => {
      window.removeEventListener("pointerdown", onPointerDown, { capture: true });
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [contextMenuOpen, setContextMenu]);

  if (!contextMenuOpen || contextMenuItems.length === 0) return null;

  // Simple boundary collision detection
  const x = Math.min(contextMenuPosition.x, window.innerWidth - 200); // approx 200px max width
  const y = Math.min(contextMenuPosition.y, window.innerHeight - 300); // approx max height

  return (
    <div
      ref={menuRef}
      className="fixed z-[1000] flex min-w-[180px] flex-col rounded-lg border border-white/[0.08] bg-[#1a1e26] p-1 shadow-[0_4px_16px_rgba(0,0,0,0.4),0_0_0_1px_rgba(0,0,0,0.5)] app-no-drag"
      style={{ left: x, top: y }}
      onContextMenu={(e) => e.preventDefault()}
    >
      {contextMenuItems.map((item, i) => (
        <MenuItem
          key={`${item.id}-${i}`}
          item={item}
          onClose={() => setContextMenu(false)}
        />
      ))}
    </div>
  );
}
