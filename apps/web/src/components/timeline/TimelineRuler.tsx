import { useEffect, useRef } from "react";
import { useUIStore } from "../../store/uiStore";
import { useProjectStore } from "../../store/projectStore";
import { C, HEADER_WIDTH, RULER_HEIGHT } from "../../theme";
import {
  Magnet,
  Plus,
} from "lucide-react";
import {
  formatBarBeat,
  secondsPerBeat,
  snapTime,
} from "../../utils/musicalTime";
import { getArrangementGridLines } from "../../utils/musicalGrid";
import { transport } from "../../engine/Transport";
import type { TimeSignature } from "../../utils/musicalTime";

type TimelineRulerProps = {
  width: number;
  onAddTrack: () => void;
  snapToGrid: boolean;
  onToggleSnapToGrid: () => void;
};

export function TimelineRuler({ width, onAddTrack, snapToGrid, onToggleSnapToGrid }: TimelineRulerProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const wrapRef  = useRef<HTMLDivElement>(null);
  const { pixelsPerSecond, scrollX, loopEnabled, loopStart, loopEnd, setLoopStart, setLoopEnd } = useUIStore();
  const { bpm, timeSignature } = useProjectStore((s) => s.project);
  const timeSig: TimeSignature = timeSignature ?? { numerator: 4, denominator: 4 };

  useEffect(() => {
    const canvas = canvasRef.current;
    const wrap   = wrapRef.current;
    if (!canvas || !wrap) return;
    let resizeObserver: ResizeObserver | null = null;

    const draw = () => {
      if (!canvas || !wrap) return;
      const W = wrap.offsetWidth || 2000;
      const dpr = window.devicePixelRatio || 1;
      canvas.width  = W * dpr;
      canvas.height = RULER_HEIGHT * dpr;
      canvas.style.width = `${W}px`;
      canvas.style.height = `${RULER_HEIGHT}px`;
      const ctx = canvas.getContext("2d");
      if (!ctx) return;
      ctx.scale(dpr, dpr);

      ctx.fillStyle = C.surface;
      ctx.fillRect(0, 0, W, RULER_HEIGHT);

      const spb = secondsPerBeat(bpm);

      ctx.font = "11px Inter Variable, ui-sans-serif, system-ui, sans-serif";
      ctx.textBaseline = "middle";

      // Single pass over all grid lines, tagged by level.
      // Tick heights: sub = 4 px bottom, beat = 9 px bottom, bar = full height.
      const lines = getArrangementGridLines(pixelsPerSecond, bpm, timeSig, scrollX, W);
      for (const line of lines) {
        const { x, level, showLabel, beat } = line;

        if (level === "sub") {
          ctx.strokeStyle = C.surfaceHigh;
          ctx.lineWidth   = 1;
          ctx.beginPath();
          ctx.moveTo(x, RULER_HEIGHT - 4);
          ctx.lineTo(x, RULER_HEIGHT);
          ctx.stroke();
        } else if (level === "beat") {
          ctx.strokeStyle = C.border;
          ctx.lineWidth   = 1;
          ctx.beginPath();
          ctx.moveTo(x, RULER_HEIGHT - 9);
          ctx.lineTo(x, RULER_HEIGHT);
          ctx.stroke();
        } else {
          // bar — full height tick
          ctx.strokeStyle = C.borderHard;
          ctx.lineWidth   = 1.5;
          ctx.beginPath();
          ctx.moveTo(x, 0);
          ctx.lineTo(x, RULER_HEIGHT);
          ctx.stroke();
        }

        if (showLabel) {
          ctx.fillStyle = level === "bar" ? C.text : C.dim;
          ctx.fillText(formatBarBeat(beat * spb, bpm, timeSig), x + 4, RULER_HEIGHT / 2);
        }
      }
    };

    draw();

    if (wrap) {
      resizeObserver = new ResizeObserver(() => draw());
      resizeObserver.observe(wrap);
    }

    return () => {
      if (resizeObserver) resizeObserver.disconnect();
    };
  }, [bpm, timeSig, pixelsPerSecond, scrollX, width]);

  const handlePointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!wrapRef.current) return;

    const updateTime = (clientX: number) => {
      const rect = wrapRef.current!.getBoundingClientRect();
      const x = clientX - rect.left;
      const { scrollX: sx, pixelsPerSecond: pps, snapToGrid } = useUIStore.getState();
      const rawSeconds = Math.max(0, (x + sx) / pps);

      if (snapToGrid) {
        const spb = secondsPerBeat(bpm);
        transport.seek(snapTime(rawSeconds, bpm, timeSig, pps * spb));
      } else {
        transport.seek(rawSeconds);
      }
    };

    updateTime(e.clientX);

    const onMove = (ev: PointerEvent) => updateTime(ev.clientX);
    const onUp = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  };

  return (
    <div className="flex shrink-0 border-b border-daw-border bg-daw-surface" style={{ height: RULER_HEIGHT }}>
      <div
        className="sticky left-0 z-30 flex shrink-0 items-center gap-2 border-r border-daw-border bg-daw-surface px-2 shadow-[8px_0_18px_rgba(0,0,0,0.28)]"
        style={{ width: HEADER_WIDTH, minWidth: HEADER_WIDTH }}
      >
        <div className="pointer-events-none absolute bottom-0 right-[-12px] top-0 z-0 w-3 bg-gradient-to-r from-daw-surface to-transparent" />
        <span className="relative z-10 min-w-0 flex-1 truncate text-[11px] font-semibold text-daw-text">
          Arrangement
        </span>
        <button
          type="button"
          onClick={onAddTrack}
          title="Add track"
          className="relative z-10 flex h-6 shrink-0 items-center gap-1.5 rounded-md border border-daw-border bg-daw-bg px-2 text-[11px] font-semibold text-daw-dim transition-colors hover:border-daw-border-light hover:bg-daw-surface-high hover:text-daw-text"
        >
          <Plus size={12} />
          Add
        </button>
        <button
          type="button"
          onClick={onToggleSnapToGrid}
          title={snapToGrid ? "Snap to grid: ON [N]" : "Snap to grid: OFF [N]"}
          className={`relative z-10 flex h-6 w-6 shrink-0 items-center justify-center rounded-md border transition-colors ${
            snapToGrid
              ? "border-daw-accent bg-daw-accent text-daw-ink hover:bg-daw-accent-h"
              : "border-daw-border bg-daw-bg text-daw-dim hover:border-daw-border-light hover:bg-daw-surface-high hover:text-daw-text"
          }`}
        >
          <Magnet size={12} />
        </button>
        <span className="relative z-10 shrink-0 rounded-md border border-daw-border bg-daw-bg px-1.5 py-0.5 text-[10px] text-daw-faint">
          bar.beat
        </span>
      </div>
      <div ref={wrapRef} className="flex-1 overflow-hidden cursor-crosshair relative" onPointerDown={handlePointerDown}>
        <canvas ref={canvasRef} className="block pointer-events-none" />

        {/* Loop Region overlay */}
        <div 
          className="absolute top-0 bottom-0 pointer-events-none transition-colors"
          style={{
            left: loopStart * pixelsPerSecond - scrollX,
            width: (loopEnd - loopStart) * pixelsPerSecond,
            background: loopEnabled ? "rgba(123, 216, 143, 0.08)" : "rgba(255, 255, 255, 0.02)",
            borderLeft: `1.5px solid ${loopEnabled ? "#7bd88f" : "rgba(255,255,255,0.2)"}`,
            borderRight: `1.5px solid ${loopEnabled ? "#7bd88f" : "rgba(255,255,255,0.2)"}`,
            zIndex: 10,
          }}
        />

        {/* Loop Start Handle */}
        <div
          className="absolute top-0 w-3 h-3 cursor-ew-resize flex items-center justify-center z-20"
          style={{ left: loopStart * pixelsPerSecond - scrollX - 0.75 }}
          onPointerDown={(e) => {
            e.stopPropagation();
            const startX = e.clientX;
            const initialStart = loopStart;
            const onMove = (ev: PointerEvent) => {
              let newStart = Math.max(0, initialStart + (ev.clientX - startX) / pixelsPerSecond);
              if (useUIStore.getState().snapToGrid) {
                const spb = secondsPerBeat(bpm);
                newStart = snapTime(newStart, bpm, timeSig, pixelsPerSecond * spb);
              }
              setLoopStart(Math.min(newStart, loopEnd - 0.1));
            };
            const onUp = () => { window.removeEventListener("pointermove", onMove); window.removeEventListener("pointerup", onUp); };
            window.addEventListener("pointermove", onMove);
            window.addEventListener("pointerup", onUp);
          }}
        >
          <svg width="8" height="8" viewBox="0 0 8 8" className={`drop-shadow ${loopEnabled ? "text-[#7bd88f]" : "text-white/40"}`}>
            <polygon points="0,0 8,0 8,8" fill="currentColor" />
          </svg>
        </div>

        {/* Loop End Handle */}
        <div
          className="absolute top-0 w-3 h-3 cursor-ew-resize flex items-center justify-center z-20"
          style={{ left: loopEnd * pixelsPerSecond - scrollX - 6 + 0.75 }}
          onPointerDown={(e) => {
            e.stopPropagation();
            const startX = e.clientX;
            const initialEnd = loopEnd;
            const onMove = (ev: PointerEvent) => {
              let newEnd = Math.max(0, initialEnd + (ev.clientX - startX) / pixelsPerSecond);
              if (useUIStore.getState().snapToGrid) {
                const spb = secondsPerBeat(bpm);
                newEnd = snapTime(newEnd, bpm, timeSig, pixelsPerSecond * spb);
              }
              setLoopEnd(Math.max(loopStart + 0.1, newEnd));
            };
            const onUp = () => { window.removeEventListener("pointermove", onMove); window.removeEventListener("pointerup", onUp); };
            window.addEventListener("pointermove", onMove);
            window.addEventListener("pointerup", onUp);
          }}
        >
          <svg width="8" height="8" viewBox="0 0 8 8" className={`drop-shadow ${loopEnabled ? "text-[#7bd88f]" : "text-white/40"}`}>
            <polygon points="0,0 8,0 0,8" fill="currentColor" />
          </svg>
        </div>
      </div>
    </div>
  );
}
