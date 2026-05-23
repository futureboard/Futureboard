import { useEffect, useRef } from "react";
import { useUIStore } from "../../store/uiStore";
import { runAction } from "../../menu/actionRunner";
import type { AppMenuItem } from "../../menu/menuItems";

// ── Item renderer ─────────────────────────────────────────────────────────────

function MenuItem({ item, onClose }: { item: AppMenuItem; onClose: () => void }) {
  if (item.type === "separator") {
    return <div className="mx-2 my-1 h-[1px] bg-white/[0.05]" />;
  }

  if (item.type === "submenu") {
    const disabled = item.enabled === false;
    return (
      <div className="group relative">
        <button
          disabled={disabled}
          className="flex w-full items-center justify-between rounded px-3 py-1.5 text-left text-[12px] font-medium text-daw-text hover:bg-daw-accent hover:text-white disabled:cursor-default disabled:text-daw-faint disabled:opacity-50"
        >
          <span>{item.label}</span>
          <span className="text-[9px] text-daw-faint group-hover:text-white/70">▶</span>
        </button>

        {!disabled && (
          <div className="invisible group-hover:visible absolute left-full top-0 z-[1001] ml-1 min-w-[160px] rounded-lg border border-white/[0.08] bg-[#1a1e26] p-1 shadow-[0_4px_16px_rgba(0,0,0,0.4),0_0_0_1px_rgba(0,0,0,0.5)]">
            {item.children.map((child, ci) => (
              <MenuItem key={`${child.id}-${ci}`} item={child} onClose={onClose} />
            ))}
          </div>
        )}
      </div>
    );
  }

  const disabled = item.enabled === false;

  return (
    <button
      disabled={disabled}
      className={[
        "flex w-full items-center gap-2 rounded px-3 py-1.5 text-left text-[12px] font-medium transition-colors",
        disabled
          ? "cursor-default text-daw-faint opacity-50"
          : item.danger
            ? "text-daw-red hover:bg-daw-red hover:text-white"
            : "text-daw-text hover:bg-daw-accent hover:text-white",
      ].join(" ")}
      onClick={() => {
        if (disabled) return;
        if (item.action) runAction(item.action);
        onClose();
      }}
    >
      {item.dot && (
        <span
          className="h-2.5 w-2.5 shrink-0 rounded-full"
          style={{ background: item.dot }}
        />
      )}
      <span className="flex-1 truncate">{item.label}</span>
      {item.accelerator && (
        <span className="ml-4 shrink-0 text-[10px] tracking-wide text-daw-dim opacity-70">
          {item.accelerator}
        </span>
      )}
    </button>
  );
}

// ── Root ContextMenu ──────────────────────────────────────────────────────────

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
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") setContextMenu(false);
    };

    window.addEventListener("pointerdown", onPointerDown, { capture: true });
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("pointerdown", onPointerDown, { capture: true });
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [contextMenuOpen, setContextMenu]);

  if (!contextMenuOpen || contextMenuItems.length === 0) return null;

  // Keep the menu on-screen; leave 220px clearance on right for submenus
  const x = Math.min(contextMenuPosition.x, window.innerWidth - 220);
  const y = Math.min(contextMenuPosition.y, window.innerHeight - 320);

  return (
    <div
      ref={menuRef}
      className="fixed z-[1000] flex min-w-[190px] flex-col rounded-lg border border-white/[0.08] bg-[#1a1e26] p-1 shadow-[0_4px_16px_rgba(0,0,0,0.4),0_0_0_1px_rgba(0,0,0,0.5)] app-no-drag"
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
