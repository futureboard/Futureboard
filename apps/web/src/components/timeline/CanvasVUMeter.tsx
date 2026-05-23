import { useEffect, useRef } from "react";
import { meterStore } from "../../store/meterStore";
import { shouldRunVisualFrame } from "../../utils/visualFrameRate";

/** Stereo VU bar meter rendered entirely on canvas. Zero React re-renders during playback. */
export function CanvasVUMeter({
  trackId,
  width = 12,
  height = 16,
}: {
  trackId: string;
  width?: number;
  height?: number;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const loggedDebugRef = useRef(false);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const dpr = window.devicePixelRatio || 1;
    canvas.width  = Math.round(width  * dpr);
    canvas.height = Math.round(height * dpr);
    canvas.style.width  = `${width}px`;
    canvas.style.height = `${height}px`;

    // Peak-hold state (no React state — purely imperative)
    let peakL = 0, peakR = 0;
    let holdFramesL = 0, holdFramesR = 0;
    const HOLD_FRAMES = 12;
    const DECAY = 0.82;

    let rafId = 0;
    let lastDrawAt = 0;
    const target = { l: 0, r: 0 };
    const unsubscribe = meterStore.subscribe(trackId, (meter) => {
      target.l = rmsToMeter(meter.peakL);
      target.r = rmsToMeter(meter.peakR);
      canvas.title = `TrackHeader trackId=${trackId} meter.trackId=${meter.trackId}`;
      if (import.meta.env.DEV && !loggedDebugRef.current && meter.updatedAt > 0) {
        loggedDebugRef.current = true;
        console.debug("[VU Meter]", { trackHeaderTrackId: trackId, meterTrackId: meter.trackId });
      }
    });

    const barW = Math.max(1, Math.floor((width - 2) / 2)); // L and R bar pixel width

    const draw = () => {
      const now = performance.now();
      if (!shouldRunVisualFrame(lastDrawAt, now)) {
        rafId = requestAnimationFrame(draw);
        return;
      }
      lastDrawAt = now;
      const ctx = canvas.getContext("2d");
      if (!ctx) { rafId = requestAnimationFrame(draw); return; }

      const { l, r } = target;

      // Peak hold with decay
      if (l >= peakL) { peakL = l; holdFramesL = HOLD_FRAMES; }
      else { holdFramesL--; if (holdFramesL <= 0) peakL *= DECAY; }

      if (r >= peakR) { peakR = r; holdFramesR = HOLD_FRAMES; }
      else { holdFramesR--; if (holdFramesR <= 0) peakR *= DECAY; }

      // Reset transform each frame so scales don't stack
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, width, height);

      drawBar(ctx, 0,          barW, height, l, peakL);
      drawBar(ctx, barW + 2,   barW, height, r, peakR);

      rafId = requestAnimationFrame(draw);
    };

    rafId = requestAnimationFrame(draw);
    return () => {
      unsubscribe();
      cancelAnimationFrame(rafId);
    };
  // Re-run only if trackId or dimensions change — color comes from fixed palette
  }, [trackId, width, height]);

  return (
    <canvas
      ref={canvasRef}
      className="shrink-0 rounded-[1px]"
      style={{ imageRendering: "pixelated" }}
    />
  );
}

// ── drawing helper ─────────────────────────────────────────────────────────────

function rmsToMeter(rms: number): number {
  if (rms < 0.000001) return 0;
  const db = 20 * Math.log10(rms);
  return Math.max(0, Math.min(1, (db + 60) / 60));
}

function drawBar(
  ctx: CanvasRenderingContext2D,
  x: number,
  w: number,
  h: number,
  level: number,   // 0–1 RMS
  peak: number,    // 0–1 peak hold
) {
  // Track background
  ctx.fillStyle = "rgba(255,255,255,0.05)";
  ctx.fillRect(x, 0, w, h);

  const levelH = Math.round(Math.min(level, 1) * h);

  if (levelH > 0) {
    const greenCut  = Math.round(h * 0.70); // bottom 70 % → green
    const yellowCut = Math.round(h * 0.90); // 70–90 % → yellow

    // Green segment
    const gH = Math.min(levelH, greenCut);
    ctx.fillStyle = "#85E0A3";
    ctx.fillRect(x, h - gH, w, gH);

    // Yellow segment
    if (levelH > greenCut) {
      const yH = Math.min(levelH - greenCut, yellowCut - greenCut);
      ctx.fillStyle = "#F4CF7A";
      ctx.fillRect(x, h - greenCut - yH, w, yH);
    }

    // Red segment
    if (levelH > yellowCut) {
      const rH = levelH - yellowCut;
      ctx.fillStyle = "#F4877F";
      ctx.fillRect(x, h - levelH, w, rH);
    }
  }

  // Peak-hold tick
  if (peak > 0.01) {
    const tickY = Math.round((1 - Math.min(peak, 1)) * h);
    ctx.fillStyle = peak > 0.90 ? "#F4877F" : peak > 0.70 ? "#F4CF7A" : "#85E0A3";
    ctx.fillRect(x, Math.max(0, tickY - 1), w, 1);
  }
}
