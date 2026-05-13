import { useEffect, useRef } from "react";
import { useUIStore } from "../../store/uiStore";
import { useProjectStore } from "../../store/projectStore";
import { HEADER_WIDTH } from "../../theme";
import { beatsPerBar, getGridIntervalBeats, getGridSubBeats, secondsPerBeat } from "../../utils/musicalTime";
import type { TimeSignature } from "../../utils/musicalTime";

export function TimelineGrid() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const wrapRef = useRef<HTMLDivElement>(null);
  const { pixelsPerSecond, scrollX } = useUIStore();
  const { bpm, timeSignature } = useProjectStore((s) => s.project);
  const timeSig: TimeSignature = timeSignature ?? { numerator: 4, denominator: 4 };

  useEffect(() => {
    const canvas = canvasRef.current;
    const wrap = wrapRef.current;
    if (!canvas || !wrap) return;

    let resizeObserver: ResizeObserver | null = null;

    const draw = () => {
      if (!canvas || !wrap) return;
      const W = wrap.offsetWidth || 2000;
      const H = wrap.offsetHeight || 1000;

      const dpr = window.devicePixelRatio || 1;
      canvas.width = W * dpr;
      canvas.height = H * dpr;
      canvas.style.width = `${W}px`;
      canvas.style.height = `${H}px`;

      const ctx = canvas.getContext("2d");
      if (!ctx) return;
      ctx.scale(dpr, dpr);
      ctx.clearRect(0, 0, W, H);

      const spb = secondsPerBeat(bpm);
      const pixelsPerBeat = pixelsPerSecond * spb;
      const bpb = beatsPerBar(timeSig);
      const startBeat = scrollX / pixelsPerBeat;
      const endBeat = (scrollX + W) / pixelsPerBeat;
      const intervalBeats = getGridIntervalBeats(pixelsPerBeat, timeSig);
      const subBeats = getGridSubBeats(pixelsPerBeat, timeSig);

      ctx.lineWidth = 1;

      // Sub-beat lines
      const subStart = Math.floor(startBeat / subBeats) * subBeats;
      for (let beat = subStart; beat <= endBeat; beat += subBeats) {
        const rb = Math.round(beat * 1000) / 1000;
        const isBar = Math.abs(rb % bpb) < 0.001;
        if (isBar) continue; // drawn separately below
        const x = Math.round(rb * pixelsPerBeat - scrollX);
        ctx.strokeStyle = "rgba(86,97,110,0.10)";
        ctx.beginPath();
        ctx.moveTo(x, 0);
        ctx.lineTo(x, H);
        ctx.stroke();
      }

      // Major / bar lines
      const majStart = Math.floor(startBeat / intervalBeats) * intervalBeats;
      for (let beat = majStart; beat <= endBeat + intervalBeats; beat += intervalBeats) {
        const rb = Math.round(beat * 1000) / 1000;
        const x = Math.round(rb * pixelsPerBeat - scrollX);
        const isBar = Math.abs(rb % bpb) < 0.001;
        ctx.strokeStyle = isBar ? "rgba(86,97,110,0.32)" : "rgba(86,97,110,0.14)";
        ctx.beginPath();
        ctx.moveTo(x, 0);
        ctx.lineTo(x, H);
        ctx.stroke();
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
