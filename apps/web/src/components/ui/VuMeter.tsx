import { memo, useEffect, useRef } from "react";
import { meterStore } from "../../store/meterStore";
import { shouldRunVisualFrame } from "../../utils/visualFrameRate";

// ── Segment config ─────────────────────────────────────────────────────────────

const SEGMENTS = 20;
const SEG_GAP  = 1.5; // px between segments

// Pre-allocate color lookup — no allocation on render/draw
const ON_COLORS: string[] = Array.from({ length: SEGMENTS }, (_, i) => {
  if (i >= SEGMENTS - 2)  return "#e9756e";
  if (i >= SEGMENTS - 5)  return "#e8be58";
  if (i >= SEGMENTS - 10) return "#56c7c9";
  return "#3a9fa1";
});
const OFF_COLOR = "rgba(255,255,255,0.045)";
const CONTEXT_OPTIONS: CanvasRenderingContext2DSettings = {
  alpha: true,
  desynchronized: true,
  willReadFrequently: false,
};
const ATTACK = 1.0;
const RELEASE = 0.26;

function rmsToMeter(rms: number): number {
  if (rms < 0.000001) return 0;
  const db = 20 * Math.log10(rms);
  return Math.max(0, Math.min(1, (db + 60) / 60));
}

function smooth(current: number, target: number): number {
  const coeff = target > current ? ATTACK : RELEASE;
  return current + coeff * (target - current);
}

// ── Draw helper ────────────────────────────────────────────────────────────────

function drawColumn(
  ctx: CanvasRenderingContext2D,
  level: number,
  x: number,
  colW: number,
  h: number,
): void {
  const clamped = level < 0 ? 0 : level > 1 ? 1 : level;
  const active  = Math.round(clamped * SEGMENTS);
  const segH    = (h - SEG_GAP * (SEGMENTS - 1)) / SEGMENTS;

  for (let i = 0; i < SEGMENTS; i++) {
    ctx.fillStyle = i < active ? ON_COLORS[i] : OFF_COLOR;
    // i = 0 → bottom segment
    const y = h - (i + 1) * segH - i * SEG_GAP;
    ctx.beginPath();
    ctx.roundRect(x, y, colW, segH, 1);
    ctx.fill();
  }
}

// ── Component ──────────────────────────────────────────────────────────────────

type Props = {
  mode?: "mono" | "stereo";
  levelL: number;
  levelR: number;
  meterTrackId?: string | "master";
  height?: number;
  /** Width of one meter column. */
  columnWidth?: number;
};

export const VuMeter = memo(function VuMeter({
  mode = "mono",
  levelL,
  levelR,
  meterTrackId,
  height,
  columnWidth = 5,
}: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const canvasRef    = useRef<HTMLCanvasElement>(null);
  const propLevelsRef = useRef({ l: levelL, r: levelR });
  const targetRef = useRef({ l: levelL, r: levelR });
  const smoothRef = useRef({ l: levelL, r: levelR });
  const sizeRef = useRef({ w: 0, h: 0, dpr: 0 });
  propLevelsRef.current = { l: levelL, r: levelR };

  const isStereo = mode === "stereo";
  const colGap   = isStereo ? 2 : 0;
  const canvasW  = isStereo ? columnWidth * 2 + colGap : columnWidth;

  useEffect(() => {
    const canvas    = canvasRef.current;
    const container = containerRef.current;
    if (!canvas || !container) return;

    const ctx = canvas.getContext("2d", CONTEXT_OPTIONS);
    if (!ctx) return;

    let rafId = 0;
    let lastDrawAt = 0;
    let unsubscribe: (() => void) | null = null;

    if (meterTrackId) {
      unsubscribe = meterStore.subscribe(meterTrackId, (raw) => {
        targetRef.current = {
          l: rmsToMeter(raw.peakL),
          r: rmsToMeter(raw.peakR),
        };
      });
    }

    const resize = () => {
      const dpr = Math.max(1, Math.min(2, window.devicePixelRatio || 1));
      const w = canvasW;
      const h = height ?? container.offsetHeight;
      if (h <= 0) return false;
      const bw = Math.round(w * dpr);
      const bh = Math.round(h * dpr);
      if (canvas.width !== bw || canvas.height !== bh) {
        canvas.width = bw;
        canvas.height = bh;
      }
      canvas.style.width = `${w}px`;
      canvas.style.height = height === undefined ? "100%" : `${h}px`;
      sizeRef.current = { w, h, dpr };
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      return true;
    };

    const draw = () => {
      const now = performance.now();
      if (!shouldRunVisualFrame(lastDrawAt, now)) {
        rafId = requestAnimationFrame(draw);
        return;
      }
      lastDrawAt = now;
      if (!meterTrackId) targetRef.current = propLevelsRef.current;
      const target = targetRef.current;
      const cur = smoothRef.current;
      const next = {
        l: smooth(cur.l, target.l),
        r: smooth(cur.r, target.r),
      };
      smoothRef.current = next;

      if (resize()) {
        const { w, h } = sizeRef.current;
        ctx.clearRect(0, 0, w, h);
        if (isStereo) {
          drawColumn(ctx, next.l, 0, columnWidth, h);
          drawColumn(ctx, next.r, columnWidth + colGap, columnWidth, h);
        } else {
          drawColumn(ctx, Math.max(next.l, next.r), 0, columnWidth, h);
        }
      }
      rafId = requestAnimationFrame(draw);
    };

    rafId = requestAnimationFrame(draw);
    return () => {
      unsubscribe?.();
      cancelAnimationFrame(rafId);
    };
  }, [canvasW, colGap, columnWidth, height, isStereo, meterTrackId]);

  return (
    <div
      ref={containerRef}
      style={{
        width:    canvasW,
        height:   height,
        flexShrink: 0,
        ...(height === undefined ? { alignSelf: "stretch" } : {}),
      }}
    >
      <canvas
        ref={canvasRef}
        style={{ display: "block", width: canvasW, height: height ?? "100%" }}
      />
    </div>
  );
});
