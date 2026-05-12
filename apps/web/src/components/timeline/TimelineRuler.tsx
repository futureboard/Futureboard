import { useEffect, useRef } from "react";
import { useUIStore } from "../../store/uiStore";
import { C, HEADER_WIDTH, RULER_HEIGHT } from "../../theme";

export function TimelineRuler({ width }: { width: number }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const wrapRef  = useRef<HTMLDivElement>(null);
  const { pixelsPerSecond, scrollX } = useUIStore();

  useEffect(() => {
    const canvas = canvasRef.current;
    const wrap   = wrapRef.current;
    if (!canvas || !wrap) return;
    const W = wrap.offsetWidth || 2000;
    canvas.width  = W;
    canvas.height = RULER_HEIGHT;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    ctx.fillStyle = C.sunken;
    ctx.fillRect(0, 0, W, RULER_HEIGHT);

    const startSec = scrollX / pixelsPerSecond;
    const endSec   = (scrollX + W) / pixelsPerSecond;
    const raw      = 80 / pixelsPerSecond;
    const niceList = [0.1, 0.25, 0.5, 1, 2, 5, 10, 15, 30, 60];
    const interval = niceList.find((n) => n >= raw) ?? 60;
    const sub      = interval / 5;

    ctx.font = "10px Inter Variable, ui-sans-serif, system-ui, sans-serif";
    ctx.textBaseline = "middle";

    // Sub-ticks
    ctx.strokeStyle = C.surfaceHigh;
    ctx.lineWidth   = 1;
    for (let s = Math.floor(startSec / sub) * sub; s <= endSec; s += sub) {
      const x = Math.round(s * pixelsPerSecond - scrollX);
      ctx.beginPath(); ctx.moveTo(x, RULER_HEIGHT - 5); ctx.lineTo(x, RULER_HEIGHT); ctx.stroke();
    }

    // Major ticks + labels
    ctx.strokeStyle = C.border;
    for (let s = Math.floor(startSec / interval) * interval; s <= endSec + interval; s += interval) {
      const rs = Math.round(s * 100) / 100;
      const x  = Math.round(rs * pixelsPerSecond - scrollX);
      ctx.beginPath(); ctx.moveTo(x, 0); ctx.lineTo(x, RULER_HEIGHT); ctx.stroke();
      ctx.fillStyle = C.text;
      ctx.fillText(fmtTime(rs), x + 3, RULER_HEIGHT / 2);
    }
  }, [pixelsPerSecond, scrollX, width]);

  return (
    <div className="flex shrink-0 border-b border-daw-border bg-daw-sunken" style={{ height: RULER_HEIGHT }}>
      <div
        className="flex shrink-0 items-center justify-between border-r border-daw-border bg-daw-sunken px-3"
        style={{ width: HEADER_WIDTH, minWidth: HEADER_WIDTH }}
      >
        <span className="text-[10px] font-medium text-daw-faint">Arrangement</span>
        <span className="text-[10px] text-daw-faint">sec</span>
      </div>
      <div ref={wrapRef} className="flex-1 overflow-hidden">
        <canvas ref={canvasRef} className="block" />
      </div>
    </div>
  );
}

function fmtTime(s: number) {
  if (s === 0) return "0";
  if (s < 60)  return `${Math.round(s * 10) / 10}s`;
  return `${Math.floor(s / 60)}:${String(Math.round(s % 60)).padStart(2, "0")}`;
}
