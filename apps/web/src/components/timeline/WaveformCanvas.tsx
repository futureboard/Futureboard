import { memo, useLayoutEffect, useRef } from "react";
import type { WaveformPeaks, WaveformStatus } from "../../types/daw";

type Props = {
  peaks?: WaveformPeaks;
  width: number;
  height: number;
  sourceDuration?: number;
  sampleRate?: number;
  clipOffset?: number;
  clipDuration?: number;
  color?: string;
  muted?: boolean;
  selected?: boolean;
  status?: WaveformStatus;
  progress?: number;
};

export const WaveformCanvas = memo(function WaveformCanvas({
  peaks,
  width,
  height,
  sourceDuration,
  sampleRate,
  clipOffset = 0,
  clipDuration,
  color = "rgba(255,255,255,0.7)",
  muted = false,
  selected = false,
  status,
  progress = 0,
}: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useLayoutEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || width < 1 || height < 1) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const dpr = Math.max(1, Math.min(2, window.devicePixelRatio || 1));
    const cssW = Math.ceil(width);
    const cssH = Math.ceil(height);
    canvas.width = cssW * dpr;
    canvas.height = cssH * dpr;
    canvas.style.width = `${cssW}px`;
    canvas.style.height = `${cssH}px`;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.fillStyle = "rgba(8,12,16,0.72)";
    ctx.fillRect(0, 0, cssW, cssH);

    if (!peaks || peaks.peaks.length === 0) {
      drawPlaceholder(ctx, cssW, cssH, color, muted, status === "error" || status === "missing");
      return;
    }

    const channelCount = peaks.channelCount || 1;
    const totalPeaks = peaks.peaks.length / (channelCount * 2);
    if (totalPeaks <= 0) {
      drawPlaceholder(ctx, cssW, cssH, color, muted, false);
      return;
    }

    const valueScale = peaks.peaks instanceof Int16Array ? 1 / 32767 : 1;
    const sr = sampleRate ?? peaks.sampleRate ?? 48000;
    const srcDur = sourceDuration ?? peaks.duration ?? (totalPeaks * peaks.samplesPerPeak) / sr;
    const srcStart = Math.max(0, Math.min(srcDur, clipOffset));
    const srcEnd = clipDuration === undefined
      ? srcDur
      : Math.max(srcStart, Math.min(srcDur, srcStart + clipDuration));
    const visibleSeconds = Math.max(1e-6, srcEnd - srcStart);

    const mid = cssH / 2;
    const amp = cssH * 0.45;

    ctx.fillStyle = color;
    ctx.globalAlpha = muted ? 0.4 : selected ? 1 : 0.9;

    for (let x = 0; x < cssW; x++) {
      const t0 = srcStart + (x / cssW) * visibleSeconds;
      const t1 = srcStart + ((x + 1) / cssW) * visibleSeconds;
      const p0 = Math.max(0, Math.min(totalPeaks - 1, Math.floor((t0 * sr) / peaks.samplesPerPeak)));
      const p1 = Math.max(p0, Math.min(totalPeaks - 1, Math.floor((t1 * sr) / peaks.samplesPerPeak)));

      let min = 0;
      let max = 0;
      for (let p = p0; p <= p1; p++) {
        for (let ch = 0; ch < channelCount; ch++) {
          const base = (p * channelCount + ch) * 2;
          const lo = peaks.peaks[base] * valueScale;
          const hi = peaks.peaks[base + 1] * valueScale;
          if (lo < min) min = lo;
          if (hi > max) max = hi;
        }
      }

      const y1 = mid - max * amp;
      const y2 = mid - min * amp;
      ctx.fillRect(x, y1, 1, Math.max(1, y2 - y1));
    }

    ctx.globalAlpha = 1;
  }, [peaks, width, height, color, muted, selected, clipOffset, clipDuration, sampleRate, sourceDuration, status]);

  const isReady = status === "ready" || (!status && !!peaks && peaks.peaks.length > 0);
  const showLoading = !isReady && (status === "loading" || status === "idle" || (!status && !peaks));
  const showError = status === "error" || status === "missing";

  return (
    <div className="relative overflow-hidden" style={{ width, height, background: "rgba(8,12,16,0.72)" }}>
      <canvas
        ref={canvasRef}
        className="block"
        style={{ width, height, opacity: muted ? 0.55 : 1, background: "rgba(8,12,16,0.72)" }}
      />
      {!isReady && (
        <div className="pointer-events-none absolute inset-0 flex items-center justify-center" aria-hidden>
          {showError ? (
            <span className="rounded border border-white/10 bg-black/20 px-1.5 py-0.5 text-[9px] font-medium tracking-wide text-red-300/80">
              {status === "missing" ? "missing audio" : "waveform error"}
            </span>
          ) : showLoading ? (
            <span className="rounded border border-white/10 bg-black/20 px-1.5 py-0.5 text-[9px] font-medium tabular-nums text-white/45">
              {progress > 0 ? `waveform ${Math.round(progress * 100)}%` : "Generating waveform..."}
            </span>
          ) : null}
        </div>
      )}
    </div>
  );
});

function drawPlaceholder(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  color: string,
  muted: boolean,
  error: boolean
) {
  const mid = height / 2;
  ctx.globalAlpha = muted ? 0.18 : 0.28;
  ctx.strokeStyle = error ? "rgba(240,122,114,0.45)" : color;
  ctx.lineWidth = 1;
  ctx.beginPath();
  for (let x = 0; x < width; x++) {
    const y = mid + Math.sin(x * 0.07) * height * 0.08 + Math.sin(x * 0.017) * height * 0.04;
    if (x === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  }
  ctx.stroke();
  ctx.globalAlpha = 1;
}
