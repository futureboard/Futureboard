import { useEffect, useRef } from "react";
import { useUIStore } from "../../store/uiStore";
import { useProjectStore } from "../../store/projectStore";
import { C, HEADER_WIDTH } from "../../theme";
import { getArrangementGridLines, type GridLineLevel } from "../../utils/musicalGrid";
import type { TimeSignature } from "../../utils/musicalTime";

// White-on-dark grid hierarchy: sub (faintest) → beat (medium) → bar (strongest).
const GRID_COLOR: Record<GridLineLevel, string> = {
  bar:  "rgba(255,255,255,0.26)",
  beat: C.gridMajor,   // rgba(255,255,255,0.095)
  sub:  C.gridMinor,   // rgba(255,255,255,0.045)
};

export function TimelineGrid() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const wrapRef   = useRef<HTMLDivElement>(null);
  const { pixelsPerSecond, scrollX } = useUIStore();
  const { bpm, timeSignature } = useProjectStore((s) => s.project);
  const timeSig: TimeSignature = timeSignature ?? { numerator: 4, denominator: 4 };

  useEffect(() => {
    const canvas = canvasRef.current;
    const wrap   = wrapRef.current;
    if (!canvas || !wrap) return;

    let ro: ResizeObserver | null = null;

    const draw = () => {
      if (!canvas || !wrap) return;
      const W   = wrap.offsetWidth  || 2000;
      const H   = wrap.offsetHeight || 1000;
      const dpr = window.devicePixelRatio || 1;

      canvas.width        = W * dpr;
      canvas.height       = H * dpr;
      canvas.style.width  = `${W}px`;
      canvas.style.height = `${H}px`;

      const ctx = canvas.getContext("2d");
      if (!ctx) return;
      ctx.scale(dpr, dpr);
      ctx.clearRect(0, 0, W, H);
      ctx.lineWidth = 1;

      const lines = getArrangementGridLines(pixelsPerSecond, bpm, timeSig, scrollX, W);
      for (const line of lines) {
        // line.x is already Math.round()'d — add 0.5 for crisp 1 px lines.
        const cx = line.x + 0.5;
        ctx.strokeStyle = GRID_COLOR[line.level];
        ctx.lineWidth = line.level === "bar" ? 1.5 : 1;
        ctx.beginPath();
        ctx.moveTo(cx, 0);
        ctx.lineTo(cx, H);
        ctx.stroke();
      }
    };

    draw();
    ro = new ResizeObserver(() => draw());
    ro.observe(wrap);

    return () => ro?.disconnect();
  }, [bpm, timeSig, pixelsPerSecond, scrollX]);

  return (
    <div
      ref={wrapRef}
      className="pointer-events-none sticky top-0 z-0 h-full min-h-full overflow-hidden"
      style={{ left: HEADER_WIDTH, width: `calc(100% - ${HEADER_WIDTH}px)` }}
    >
      <canvas ref={canvasRef} className="block" />
    </div>
  );
}
