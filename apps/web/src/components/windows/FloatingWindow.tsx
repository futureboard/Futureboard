import { useRef, useCallback } from "react";
import type { AppWindowState } from "../../store/windowStore";
import { DawIcon } from "../../icons/dawIcons";

type Props = {
  window: AppWindowState;
  children?: React.ReactNode;
  onClose: () => void;
  onFocus: () => void;
  onBoundsChange: (bounds: Partial<Pick<AppWindowState, "x" | "y" | "width" | "height">>) => void;
  onDetach?: () => void;
};

export function FloatingWindow({ window: win, children, onClose, onFocus, onBoundsChange, onDetach }: Props) {
  const dragState = useRef<{ startX: number; startY: number; origX: number; origY: number } | null>(null);
  const resizeState = useRef<{ startX: number; startY: number; origW: number; origH: number } | null>(null);

  const handleTitlePointerDown = useCallback((e: React.PointerEvent) => {
    if ((e.target as HTMLElement).closest("button")) return;
    e.preventDefault();
    onFocus();
    dragState.current = { startX: e.clientX, startY: e.clientY, origX: win.x, origY: win.y };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }, [onFocus, win.x, win.y]);

  const handleTitlePointerMove = useCallback((e: React.PointerEvent) => {
    if (!dragState.current) return;
    const dx = e.clientX - dragState.current.startX;
    const dy = e.clientY - dragState.current.startY;
    const x = Math.max(0, Math.min(globalThis.window.innerWidth - win.width, dragState.current.origX + dx));
    const y = Math.max(0, Math.min(globalThis.window.innerHeight - 32, dragState.current.origY + dy));
    onBoundsChange({ x, y });
  }, [onBoundsChange, win.width]);

  const handleTitlePointerUp = useCallback(() => {
    dragState.current = null;
  }, []);

  const handleResizePointerDown = useCallback((e: React.PointerEvent) => {
    e.preventDefault();
    e.stopPropagation();
    onFocus();
    resizeState.current = { startX: e.clientX, startY: e.clientY, origW: win.width, origH: win.height };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }, [onFocus, win.width, win.height]);

  const handleResizePointerMove = useCallback((e: React.PointerEvent) => {
    if (!resizeState.current) return;
    const dx = e.clientX - resizeState.current.startX;
    const dy = e.clientY - resizeState.current.startY;
    const width = Math.max(win.minWidth ?? 200, resizeState.current.origW + dx);
    const height = Math.max(win.minHeight ?? 120, resizeState.current.origH + dy);
    onBoundsChange({ width, height });
  }, [onBoundsChange, win.minWidth, win.minHeight]);

  const handleResizePointerUp = useCallback(() => {
    resizeState.current = null;
  }, []);

  return (
    <div
      className="fixed flex flex-col bg-daw-panel border border-daw-border shadow-xl select-none"
      style={{ left: win.x, top: win.y, width: win.width, height: win.height, zIndex: win.zIndex }}
      onPointerDown={onFocus}
    >
      {/* Titlebar */}
      <div
        className="flex items-center h-8 px-2 gap-1 bg-daw-surface border-b border-daw-border cursor-grab active:cursor-grabbing flex-shrink-0"
        onPointerDown={handleTitlePointerDown}
        onPointerMove={handleTitlePointerMove}
        onPointerUp={handleTitlePointerUp}
      >
        <span className="flex-1 text-[11px] font-medium text-daw-text truncate">{win.title}</span>
        {onDetach && (
          <button
            className="w-5 h-5 flex items-center justify-center rounded text-daw-text-muted hover:text-daw-text hover:bg-white/10"
            onClick={onDetach}
            title="Detach to external window"
          >
            <DawIcon name="external-link" size={11} />
          </button>
        )}
        {win.closable !== false && (
          <button
            className="w-5 h-5 flex items-center justify-center rounded text-daw-text-muted hover:text-daw-text hover:bg-white/10"
            onClick={onClose}
            title="Close"
          >
            <DawIcon name="x" size={11} />
          </button>
        )}
      </div>

      {/* Content */}
      <div className="flex-1 overflow-auto min-h-0">
        {children}
      </div>

      {/* Resize handle */}
      {win.resizable !== false && (
        <div
          className="absolute bottom-0 right-0 w-4 h-4 cursor-se-resize"
          onPointerDown={handleResizePointerDown}
          onPointerMove={handleResizePointerMove}
          onPointerUp={handleResizePointerUp}
          style={{ touchAction: "none" }}
        >
          <svg className="absolute bottom-1 right-1 text-daw-text-muted opacity-50" width="8" height="8" viewBox="0 0 8 8">
            <path d="M7 1L1 7M7 4L4 7M7 7" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
          </svg>
        </div>
      )}
    </div>
  );
}
