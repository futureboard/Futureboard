import { memo, useEffect, useRef } from "react";
import type { WaveformPeaks, WaveformStatus } from "../../types/daw";

type Props = {
  peaks?: WaveformPeaks;
  width: number;
  height: number;
  /** Source audio duration in seconds. Falls back to peaks.duration. */
  sourceDuration?: number;
  /** Source audio sample rate. Falls back to peaks.sampleRate. */
  sampleRate?: number;
  /** Clip's offset into the source audio. */
  clipOffset?: number;
  /** Clip's visible duration in seconds. */
  clipDuration?: number;
  color?: string;
  muted?: boolean;
  selected?: boolean;
  status?: WaveformStatus;
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
}: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  // ── Draw waveform when peaks/dimensions/clip range/color change ─────────────
  useEffect(() => {
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
    ctx.clearRect(0, 0, cssW, cssH);

    if (!peaks || peaks.peaks.length === 0) return;

    const channelCount = peaks.channelCount || 1;
    const totalPeaks = peaks.peaks.length / (channelCount * 2);
    if (totalPeaks <= 0) return;

    // Resolve clip range in source-time.
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

    // Walk columns; each column maps to a source-time range and aggregates peaks.
    for (let x = 0; x < cssW; x++) {
      const t0 = srcStart + (x / cssW) * visibleSeconds;
      const t1 = srcStart + ((x + 1) / cssW) * visibleSeconds;
      const p0 = Math.max(0, Math.min(totalPeaks - 1, Math.floor((t0 * sr) / peaks.samplesPerPeak)));
      const p1 = Math.max(p0, Math.min(totalPeaks - 1, Math.floor((t1 * sr) / peaks.samplesPerPeak)));

      let min = 0;
      let max = 0;
      // Combine across channels (mono visual mix-down for v0.1).
      for (let p = p0; p <= p1; p++) {
        for (let ch = 0; ch < channelCount; ch++) {
          const base = (p * channelCount + ch) * 2;
          const lo = peaks.peaks[base];
          const hi = peaks.peaks[base + 1];
          if (lo < min) min = lo;
          if (hi > max) max = hi;
        }
      }

      const y1 = mid - max * amp;
      const y2 = mid - min * amp;
      ctx.fillRect(x, y1, 1, Math.max(1, y2 - y1));
    }

    ctx.globalAlpha = 1;
  }, [peaks, width, height, color, muted, selected, clipOffset, clipDuration, sampleRate, sourceDuration]);

  // ── Loading / error / missing overlays ──────────────────────────────────────
  const isReady = status === "ready" || (!status && !!peaks && peaks.peaks.length > 0);
  const showLoading = !isReady && (status === "loading" || (!status && !peaks));
  const showError = status === "error";

  return (
    <div className="relative" style={{ width, height }}>
      <canvas
        ref={canvasRef}
        className="block"
        style={{ width, height, opacity: muted ? 0.55 : 1 }}
      />
      {!isReady && (
        <div
          className="pointer-events-none absolute inset-0 flex items-center justify-center"
          aria-hidden
        >
          {showError ? (
            <span
              className="text-[9px] font-medium tracking-wide"
              style={{ color: "rgba(240,122,114,0.85)" }}
            >
              waveform error
            </span>
          ) : showLoading ? (
            <LoadingBars color={color} width={width} height={height} />
          ) : (
            <PlaceholderBars color={color} width={width} height={height} />
          )}
        </div>
      )}
    </div>
  );
});

// ── Subtle shimmer / placeholder visuals ──────────────────────────────────────

function LoadingBars({ color, width, height }: { color: string; width: number; height: number }) {
  const bars = Math.max(8, Math.floor(width / 6));
  return (
    <div className="flex h-full w-full items-center justify-between px-1">
      {Array.from({ length: bars }).map((_, i) => {
        const h = 0.25 + 0.45 * Math.abs(Math.sin(i * 0.7));
        return (
          <span
            key={i}
            className="block waveform-shimmer"
            style={{
              width: 2,
              height: `${Math.round(height * h)}px`,
              background: color,
              opacity: 0.18,
              borderRadius: 1,
              animationDelay: `${(i % 8) * 80}ms`,
            }}
          />
        );
      })}
    </div>
  );
}

function PlaceholderBars({ color, width, height }: { color: string; width: number; height: number }) {
  const bars = Math.max(6, Math.floor(width / 10));
  return (
    <div className="flex h-full w-full items-center justify-between px-1 opacity-50">
      {Array.from({ length: bars }).map((_, i) => (
        <span
          key={i}
          className="block"
          style={{
            width: 1,
            height: `${Math.round(height * 0.15)}px`,
            background: color,
            opacity: 0.22,
          }}
        />
      ))}
    </div>
  );
}
