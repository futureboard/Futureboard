import { useEffect, useRef } from "react";
import { useUIStore } from "../../store/uiStore";
import { useProjectStore } from "../../store/projectStore";
import { C, HEADER_WIDTH, RULER_HEIGHT } from "../../theme";
import {
  Magnet,
  Plus,
} from "lucide-react";
import {
  beatsPerBar,
  formatBarBeat,
  getGridIntervalBeats,
  getGridSubBeats,
  secondsPerBeat,
  snapTime,
} from "../../utils/musicalTime";
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
  const { pixelsPerSecond, scrollX } = useUIStore();
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
      const pixelsPerBeat = pixelsPerSecond * spb;
      const bpb = beatsPerBar(timeSig);
      const startBeat = scrollX / pixelsPerBeat;
      const endBeat = (scrollX + W) / pixelsPerBeat;
      const intervalBeats = getGridIntervalBeats(pixelsPerBeat, timeSig);
      const subBeats = getGridSubBeats(pixelsPerBeat, timeSig);

      ctx.font = "11px Inter Variable, ui-sans-serif, system-ui, sans-serif";
      ctx.textBaseline = "middle";

      // Sub-ticks
      ctx.strokeStyle = C.surfaceHigh;
      ctx.lineWidth   = 1;
      const subStart = Math.floor(startBeat / subBeats) * subBeats;
      for (let beat = subStart; beat <= endBeat; beat += subBeats) {
        const x = Math.round(beat * pixelsPerBeat - scrollX);
        ctx.beginPath();
        ctx.moveTo(x, RULER_HEIGHT - 5);
        ctx.lineTo(x, RULER_HEIGHT);
        ctx.stroke();
      }

      // Major ticks + labels
      const majStart = Math.floor(startBeat / intervalBeats) * intervalBeats;
      for (let beat = majStart; beat <= endBeat + intervalBeats; beat += intervalBeats) {
        const rb = Math.round(beat * 1000) / 1000;
        const x  = Math.round(rb * pixelsPerBeat - scrollX);
        const isBar = Math.abs(rb % bpb) < 0.001;
        ctx.strokeStyle = isBar ? C.borderHard : C.border;
        ctx.lineWidth = isBar ? 1.5 : 1;
        ctx.beginPath();
        ctx.moveTo(x, 0);
        ctx.lineTo(x, RULER_HEIGHT);
        ctx.stroke();
        ctx.fillStyle = isBar ? C.text : C.dim;
        ctx.fillText(formatBarBeat(rb * spb, bpm, timeSig), x + 4, RULER_HEIGHT / 2);
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
      <div ref={wrapRef} className="flex-1 overflow-hidden cursor-crosshair" onPointerDown={handlePointerDown}>
        <canvas ref={canvasRef} className="block pointer-events-none" />
      </div>
    </div>
  );
}
