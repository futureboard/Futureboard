import { useEffect, useRef } from "react";
import { useUIStore } from "../../store/uiStore";
import { useProjectStore } from "../../store/projectStore";
import { HEADER_WIDTH } from "../../theme";
import { getArrangementGridLines, pxPerBeat, type GridLineLevel } from "../../utils/musicalGrid";
import { beatsPerBar } from "../../utils/musicalTime";
import type { TimeSignature } from "../../utils/musicalTime";

// ── Color hierarchy: sub (ghost) → beat (medium) → bar (anchor) ───────────────
// Values chosen to be clearly readable on a dark surface without feeling harsh.
const GRID_ALPHA: Record<GridLineLevel, number> = {
  bar:  0.14,   // strongest — anchors the eye to bar boundaries
  beat: 0.062,  // medium — readable when zoomed in, invisible when far out
  sub:  0.026,  // ghost — only visible when zoomed close
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

      const ppb  = pxPerBeat(pixelsPerSecond, bpm);
      const bpb  = beatsPerBar(timeSig);
      const barW = bpb * ppb;

      // ── Pass 1: alternating bar shading ──────────────────────────────────────
      // Every other bar gets a very subtle fill.  Computed directly from geometry
      // so there's no dependency on the grid-line array (handles partial bars at
      // the left viewport edge correctly).
      if (barW >= 2) {
        const startBeat  = scrollX / ppb;
        const firstBar   = Math.floor(startBeat / bpb);
        ctx.fillStyle    = "rgba(255,255,255,0.022)";
        // Only shade even-indexed bars (0-based) so the pattern is stable
        for (let bar = firstBar; bar * barW - scrollX < W + barW; bar++) {
          if (bar % 2 === 0) {
            const bx = Math.round(bar * barW - scrollX);
            ctx.fillRect(bx, 0, Math.round(barW), H);
          }
        }
      }

      // ── Pass 2: grid lines ────────────────────────────────────────────────────
      const lines = getArrangementGridLines(pixelsPerSecond, bpm, timeSig, scrollX, W);

      // Draw sub and beat lines first (thin, faint), then bar lines on top.
      // Two sub-passes lets bar lines paint over beat/sub lines cleanly at their x.
      for (const line of lines) {
        if (line.level === "bar") continue;
        const cx = line.x + 0.5;
        ctx.strokeStyle = `rgba(255,255,255,${GRID_ALPHA[line.level]})`;
        ctx.lineWidth   = 1;
        ctx.beginPath();
        ctx.moveTo(cx, 0);
        ctx.lineTo(cx, H);
        ctx.stroke();
      }
      for (const line of lines) {
        if (line.level !== "bar") continue;
        const cx = line.x + 0.5;
        ctx.strokeStyle = `rgba(255,255,255,${GRID_ALPHA.bar})`;
        ctx.lineWidth   = 1;
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
