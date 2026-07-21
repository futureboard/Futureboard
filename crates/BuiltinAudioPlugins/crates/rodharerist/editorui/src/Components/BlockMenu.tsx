import { useEffect, useLayoutEffect, useRef, useState } from "react";

export type MenuItem = {
  label: string;
  onSelect: () => void;
  disabled?: boolean;
  /** Renders above this item. */
  separatorBefore?: boolean;
  /** Styles the item as destructive and keeps it away from the safe actions. */
  destructive?: boolean;
};

type BlockMenuProps = {
  /** Accessible name for the trigger, e.g. "Amp block options". */
  label: string;
  items: MenuItem[];
};

/**
 * The three-dot options menu on a signal-chain block.
 *
 * Replaces the bare `×` that used to sit next to the power button: destructive
 * actions are now behind a deliberate second click and separated from the rest
 * of the menu, so a block cannot be dropped from the path by a mis-click.
 *
 * Closes on Escape, on outside pointer-down, and after any selection. The panel
 * is clamped to the viewport so it stays reachable near a window edge.
 */
export function BlockMenu({ label, items }: BlockMenuProps) {
  const [open, setOpen] = useState(false);
  const wrapRef = useRef<HTMLDivElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const firstItemRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!open) return;

    const onPointerDown = (e: PointerEvent) => {
      if (!wrapRef.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        setOpen(false);
      }
    };

    window.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("keydown", onKeyDown, true);
    return () => {
      window.removeEventListener("pointerdown", onPointerDown, true);
      window.removeEventListener("keydown", onKeyDown, true);
    };
  }, [open]);

  // Clamp to the viewport once the panel has a measured size, rather than
  // assuming there is room below-right of the trigger.
  useLayoutEffect(() => {
    if (!open) return;
    const panel = panelRef.current;
    if (!panel) return;
    panel.style.left = "0px";
    panel.style.top = "100%";
    const rect = panel.getBoundingClientRect();
    const overflowX = rect.right - (window.innerWidth - 8);
    if (overflowX > 0) panel.style.left = `${-overflowX}px`;
    const overflowY = rect.bottom - (window.innerHeight - 8);
    if (overflowY > 0) panel.style.top = `${-rect.height - 4}px`;
    firstItemRef.current?.focus();
  }, [open]);

  return (
    <div className="blk-menu" ref={wrapRef}>
      <button
        type="button"
        className="blk-menu-btn"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={label}
        title={label}
        onPointerDown={(e) => e.stopPropagation()}
        onClick={(e) => {
          e.stopPropagation();
          setOpen((v) => !v);
        }}
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden>
          <circle cx="12" cy="5" r="1.6" />
          <circle cx="12" cy="12" r="1.6" />
          <circle cx="12" cy="19" r="1.6" />
        </svg>
      </button>

      {open && (
        <div
          ref={panelRef}
          className="blk-menu-panel"
          role="menu"
          onPointerDown={(e) => e.stopPropagation()}
          onClick={(e) => e.stopPropagation()}
        >
          {items.map((item, i) => (
            <div key={item.label} className="blk-menu-row">
              {item.separatorBefore && <div className="blk-menu-sep" role="separator" />}
              <button
                ref={i === 0 ? firstItemRef : undefined}
                type="button"
                role="menuitem"
                className={`blk-menu-item${item.destructive ? " destructive" : ""}`}
                disabled={item.disabled}
                onClick={() => {
                  setOpen(false);
                  item.onSelect();
                }}
              >
                {item.label}
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
