import { useEffect, useRef } from "react";
import { activeAudioEngine } from "../../engine/activeAudioEngine";
import { useTransportStore } from "../../store/transportStore";
import { useUIStore } from "../../store/uiStore";
import { C } from "../../theme";
import { TIMELINE_CONTENT_LEFT, timeToContentX } from "../../utils/musicalTime";
import { TIMELINE_Z } from "../../utils/timelineZ";
import { TimelineGpuPlayheadRenderer } from "./timelineGpuPlayheadRenderer";
import { shouldRunVisualFrame } from "../../utils/visualFrameRate";

/**
 * Renders the playhead inside a clip container that starts at the timeline
 * content origin (TIMELINE_CONTENT_LEFT = HEADER_WIDTH).  Both the line and
 * the marker share the same parent, the same z-index, and the same x
 * derivation (`timeToContentX`) — so they can never separate, and they can
 * never visually leak across the sticky track-header lane.
 *
 * Coordinates inside the wrapper are CONTENT pixels:
 *   line/marker x  =  timeToContentX(t, pps, scrollX)
 *
 * The vertical line is 2 px wide; both line and marker centre on the same
 * pixel column the canvas grid draws (Math.round(x) + 0.5).
 */
const LINE_W = 2;
const HEAD_W = 12;

export function Playhead() {
  const wrapRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const gpuRef = useRef<TimelineGpuPlayheadRenderer | null>(null);
  const rafRef   = useRef<number>(0);
  const lastDrawAt = useRef(0);
  const lastStore = useRef(0);
  const setPlayheadTime = useTransportStore((s) => s.setPlayheadTime);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (canvas) gpuRef.current = TimelineGpuPlayheadRenderer.create(canvas);

    const tick = () => {
      const now = performance.now();
      const { pixelsPerSecond: pps, scrollX, loopEnabled, loopStart, loopEnd } =
        useUIStore.getState();
      const t = activeAudioEngine.projectTime;

      if (activeAudioEngine.isPlaying && loopEnabled && t >= loopEnd) {
        activeAudioEngine.seekSeconds(loopStart);
        rafRef.current = requestAnimationFrame(tick);
        return;
      }

      // Content-area x — wrapper already begins at TIMELINE_CONTENT_LEFT,
      // so the line/marker never paint over the sticky track-header lane.
      const x = timeToContentX(t, pps, scrollX);
      if (shouldRunVisualFrame(lastDrawAt.current, now)) {
        lastDrawAt.current = now;
        const wrap = wrapRef.current;
        const canvas = canvasRef.current;
        if (wrap && canvas) {
          const width = wrap.clientWidth || 1;
          const height = wrap.clientHeight || 1;
          const dpr = window.devicePixelRatio || 1;

          if (gpuRef.current) {
            try {
              gpuRef.current.resize(width, height, dpr);
              gpuRef.current.render(x);
            } catch (error) {
              console.warn("[TimelineGPU] Playhead render failed; falling back to Canvas2D:", error);
              gpuRef.current.dispose();
              gpuRef.current = null;
            }
          }
          if (!gpuRef.current) {
            drawCanvasPlayhead(canvas, width, height, dpr, x);
          }
        }
      }

      if (now - lastStore.current > 100) {
        setPlayheadTime(Math.round(t * 100) / 100);
        lastStore.current = now;
      }
      rafRef.current = requestAnimationFrame(tick);
    };

    rafRef.current = requestAnimationFrame(tick);
    return () => {
      cancelAnimationFrame(rafRef.current);
      gpuRef.current?.dispose();
      gpuRef.current = null;
    };
  }, [setPlayheadTime]);

  return (
    // Clip container — starts at TIMELINE_CONTENT_LEFT, overflow-hidden so
    // the line and marker can never visually paint across the sticky
    // track-header lane on the left.  Single z-index for both children.
    <div
      ref={wrapRef}
      className="pointer-events-none absolute top-0 bottom-0 right-0 overflow-hidden"
      style={{ left: TIMELINE_CONTENT_LEFT, zIndex: TIMELINE_Z.playhead }}
      aria-hidden
    >
      <canvas ref={canvasRef} className="block h-full w-full" />
    </div>
  );
}

function drawCanvasPlayhead(
  canvas: HTMLCanvasElement,
  width: number,
  height: number,
  dpr: number,
  x: number,
): void {
  const ratio = Math.max(1, Math.min(2, dpr || 1));
  const bw = Math.ceil(width * ratio);
  const bh = Math.ceil(height * ratio);
  if (canvas.width !== bw || canvas.height !== bh) {
    canvas.width = bw;
    canvas.height = bh;
  }
  canvas.style.width = `${width}px`;
  canvas.style.height = `${height}px`;
  const ctx = canvas.getContext("2d", { alpha: true, desynchronized: true });
  if (!ctx) return;
  ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
  ctx.clearRect(0, 0, width, height);
  ctx.fillStyle = C.playhead + "cc";
  ctx.fillRect(Math.round(x) - LINE_W / 2, 0, LINE_W, height);
  ctx.fillStyle = C.accent;
  ctx.beginPath();
  ctx.moveTo(Math.round(x) - HEAD_W / 2, 0);
  ctx.lineTo(Math.round(x) + HEAD_W / 2, 0);
  ctx.lineTo(Math.round(x), 12);
  ctx.closePath();
  ctx.fill();
}
