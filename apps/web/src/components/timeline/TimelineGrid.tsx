import { useEffect, useRef } from "react";
import { useUIStore } from "../../store/uiStore";
import { useProjectStore } from "../../store/projectStore";
import { HEADER_WIDTH } from "../../theme";
import { getArrangementGridLines, pxPerBeat, type GridLineLevel } from "../../utils/musicalGrid";
import { beatsPerBar } from "../../utils/musicalTime";
import type { SnapDivision, TimeSignature } from "../../utils/musicalTime";
import { TimelineGpuGridRenderer } from "./timelineGpuGridRenderer";
import { subscribeScroll, getScrollX } from "../../engine/scrollController";

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
  const rafRef = useRef<number | null>(null);
  const drawRef = useRef<(() => void) | null>(null);
  const rendererRef = useRef<TimelineGpuGridRenderer | null>(null);
  const stateRef = useRef({
    pixelsPerSecond: 0,
    bpm: 120,
    timeSig: { numerator: 4, denominator: 4 } as TimeSignature,
    gridDivision: "1/16" as SnapDivision,
  });
  // Subscribe only to pixelsPerSecond (not scrollX) — scrollX is handled by
  // subscribeScroll below to avoid React re-renders on every scroll event.
  const pixelsPerSecond = useUIStore((s) => s.pixelsPerSecond);
  const gridDivision = useUIStore((s) => s.arrangementGridDivision);
  const { bpm, timeSignature } = useProjectStore((s) => s.project);
  const timeSig: TimeSignature = timeSignature ?? { numerator: 4, denominator: 4 };
  stateRef.current = { pixelsPerSecond, bpm, timeSig, gridDivision };

  useEffect(() => {
    const canvas = canvasRef.current;
    const wrap   = wrapRef.current;
    if (!canvas || !wrap) return;

    let ro: ResizeObserver | null = null;
    rendererRef.current = TimelineGpuGridRenderer.create(canvas);

    const draw = () => {
      if (!canvas || !wrap) return;
      const W   = wrap.offsetWidth  || 2000;
      const H   = wrap.offsetHeight || 1000;
      const dpr = Math.max(1, Math.min(2, window.devicePixelRatio || 1));
      const { pixelsPerSecond, bpm, timeSig, gridDivision } = stateRef.current;
      // Read scrollX directly from controller — avoids Zustand round-trip.
      const scrollX = getScrollX();

      const ppb  = pxPerBeat(pixelsPerSecond, bpm);
      const bpb  = beatsPerBar(timeSig);
      const barW = bpb * ppb;
      const lines = getArrangementGridLines(pixelsPerSecond, bpm, timeSig, scrollX, W, gridDivision);

      if (rendererRef.current) {
        try {
          rendererRef.current.resize(W, H, dpr);
          rendererRef.current.render(lines, scrollX, ppb, bpb);
          return;
        } catch (error) {
          console.warn("[TimelineGPU] WebGL grid render failed; falling back to Canvas2D:", error);
          rendererRef.current.dispose();
          rendererRef.current = null;
        }
      }

      canvas.width        = W * dpr;
      canvas.height       = H * dpr;
      canvas.style.width  = `${W}px`;
      canvas.style.height = `${H}px`;

      const ctx = canvas.getContext("2d", { alpha: true, desynchronized: true });
      if (!ctx) {
        canvas.style.display = "none";
        return;
      }
      canvas.style.display = "block";
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, W, H);

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
    drawRef.current = draw;

    const scheduleDraw = () => {
      if (rafRef.current !== null) return;
      rafRef.current = requestAnimationFrame(() => {
        rafRef.current = null;
        draw();
      });
    };

    scheduleDraw();
    ro = new ResizeObserver(() => scheduleDraw());
    ro.observe(wrap);
    const unsubScroll = subscribeScroll(() => scheduleDraw());

    return () => {
      unsubScroll();
      ro?.disconnect();
      if (rafRef.current !== null) cancelAnimationFrame(rafRef.current);
      rendererRef.current?.dispose();
      rendererRef.current = null;
      drawRef.current = null;
    };
  }, []);

  useEffect(() => {
    if (rafRef.current !== null) return;
    rafRef.current = requestAnimationFrame(() => {
      rafRef.current = null;
      drawRef.current?.();
    });
  }, [bpm, timeSig, pixelsPerSecond, gridDivision]);

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
