import { memo, useLayoutEffect, useRef, useEffect } from "react";
import type { WaveformStatus } from "../../types/daw";
import type { PeakLevelMeta } from "../../store/projectStore";
import { HEADER_WIDTH } from "../../theme";
import {
  CHUNK_PEAKS,
  requestChunk,
  updateCanvasPixels,
} from "../../engine/peakChunkCache";
import { readPeakChunk } from "../../engine/peakChunkStore";
import {
  subscribeScroll,
  subscribeScrollIdle,
  isScrollingNow,
  getScrollX,
} from "../../engine/scrollController";

/**
 * Each tile covers at most TILE_SIZE CSS pixels of the clip.
 * Two tiles are maintained so the visible region is always fully covered,
 * even when the viewport straddles a tile boundary.
 */
const TILE_SIZE = 2048;
const MAX_PEAKS_PER_COLUMN = 16;
const DENSE_WAVEFORM_THRESHOLD = 6;
const WAVEFORM_CONTEXT_OPTIONS: CanvasRenderingContext2DSettings = {
  alpha: false,
  desynchronized: true,
  willReadFrequently: false,
};

type Props = {
  fileId?: string;
  levelMeta?: PeakLevelMeta;
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
  /**
   * Absolute left position of this clip within the timeline scroll content
   * (= clip.startTime × pixelsPerSecond, without HEADER_WIDTH).
   * When provided, WaveformCanvas tracks the viewport and renders only
   * the visible portion via two TILE_SIZE-wide tiles.
   */
  clipStartPx?: number;
};

export const WaveformCanvas = memo(function WaveformCanvas(props: Props) {
  const {
    fileId, levelMeta,
    width, height,
    sourceDuration, sampleRate,
    clipOffset = 0, clipDuration,
    color = "rgba(255,255,255,0.7)",
    muted = false, selected = false,
    status, progress = 0,
    clipStartPx,
  } = props;

  const tile0Ref = useRef<HTMLCanvasElement>(null);
  const tile1Ref = useRef<HTMLCanvasElement>(null);
  const scrollXRef = useRef(getScrollX());
  const rafRef = useRef<number | null>(null);
  const drawVersionRef = useRef(0);

  // Mirror all props into a ref so imperative draw callbacks never close over stale values.
  const propsRef = useRef(props);
  propsRef.current = props;

  // ── Tile draw ───────────────────────────────────────────────────────────────

  const drawTile = (
    canvas: HTMLCanvasElement | null,
    tileLeft: number,
    tileW: number,
    p: Props,
  ): void => {
    if (!canvas || tileW < 1) return;

    const w = p.width;
    const h = p.height;
    const dpr  = Math.max(1, Math.min(2, window.devicePixelRatio || 1));
    const cssW = Math.ceil(tileW);
    const cssH = Math.ceil(h);

    // Track canvas backing pixels for PerfMonitor.
    const prevPx = (canvas as HTMLCanvasElement & { _px?: number })._px ?? 0;
    const nextPx = cssW * dpr * cssH * dpr;
    if (prevPx !== nextPx) {
      (canvas as HTMLCanvasElement & { _px?: number })._px = nextPx;
      updateCanvasPixels(prevPx, nextPx);
    }

    if (canvas.width !== cssW * dpr || canvas.height !== cssH * dpr) {
      canvas.width  = cssW * dpr;
      canvas.height = cssH * dpr;
    }
    canvas.style.width   = `${cssW}px`;
    canvas.style.height  = `${cssH}px`;
    canvas.style.left    = `${tileLeft}px`;
    canvas.style.display = "block";

    const ctx = canvas.getContext("2d", WAVEFORM_CONTEXT_OPTIONS);
    if (!ctx) return;

    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.fillStyle = "#0b0f14";
    ctx.fillRect(0, 0, cssW, cssH);

    const { fileId, levelMeta } = p;
    if (!fileId || !levelMeta) {
      drawPlaceholder(ctx, cssW, cssH, p.color ?? "rgba(255,255,255,0.7)", !!p.muted, p.status === "error" || p.status === "missing");
      return;
    }

    const { spp, peakCount, channelCount } = levelMeta;
    const sr       = p.sampleRate ?? levelMeta.sampleRate ?? 48000;
    const srcDur   = p.sourceDuration ?? levelMeta.duration ?? (peakCount * spp) / sr;
    const srcStart = Math.max(0, Math.min(srcDur, p.clipOffset ?? 0));
    const srcEnd   = p.clipDuration === undefined
      ? srcDur
      : Math.max(srcStart, Math.min(srcDur, srcStart + p.clipDuration));
    const totalVisSecs = Math.max(1e-6, srcEnd - srcStart);

    // Determine peak index range covered by this tile.
    const tileStartFrac = w > 0 ? tileLeft / w : 0;
    const tileEndFrac   = w > 0 ? (tileLeft + tileW) / w : 1;
    // Peak indices corresponding to the audio that this tile represents.
    const peakIdxStart = Math.max(0, Math.floor(tileStartFrac * peakCount));
    const peakIdxEnd   = Math.min(peakCount - 1, Math.ceil(tileEndFrac * peakCount));

    if (peakIdxStart > peakIdxEnd) { return; }

    // Collect the chunks we need for this tile.
    const chunkStart = Math.floor(peakIdxStart / CHUNK_PEAKS);
    const chunkEnd   = Math.floor(peakIdxEnd   / CHUNK_PEAKS);
    const chunks = new Map<number, Int16Array>();
    let allChunksReady = true;
    for (let ci = chunkStart; ci <= chunkEnd; ci++) {
      const data = requestChunk(
        fileId, spp, ci,
        () => readPeakChunk(fileId, spp, ci),
        scheduleDraw,
      );
      if (data) {
        chunks.set(ci, data);
      } else {
        allChunksReady = false;
      }
    }

    if (!allChunksReady && chunks.size === 0) {
      // Nothing available yet — show placeholder and wait for chunks.
      drawPlaceholder(ctx, cssW, cssH, p.color ?? "rgba(255,255,255,0.7)", !!p.muted, false);
      return;
    }

    const mid = cssH / 2;
    const amp = cssH * 0.45;
    const peaksPerColumn = Math.max(1, (peakIdxEnd - peakIdxStart + 1) / Math.max(1, cssW));
    const dense = peaksPerColumn > DENSE_WAVEFORM_THRESHOLD;
    // During active scroll, skip the 4-step horizontal smear (5× fillRect → 1×
    // per pixel at close zoom). A refine redraw fires 100 ms after scroll stops.
    const scrolling = isScrollingNow();

    ctx.fillStyle = p.color ?? "rgba(255,255,255,0.7)";
    const baseAlpha = p.muted ? 0.4 : p.selected ? 1 : 0.9;

    for (let x = 0; x < cssW; x++) {
      // Map pixel to audio time then to peak index range.
      const xFrac0 = (tileLeft +  x      ) / w;
      const xFrac1 = (tileLeft + (x + 1) ) / w;
      const timeSec0 = srcStart + xFrac0 * totalVisSecs;
      const timeSec1 = srcStart + xFrac1 * totalVisSecs;
      const p0 = Math.max(0, Math.min(peakCount - 1, Math.floor((timeSec0 * sr) / spp)));
      const p1 = Math.max(p0, Math.min(peakCount - 1, Math.floor((timeSec1 * sr) / spp)));

      let lo = 0;
      let hi = 0;
      const peakSpan = p1 - p0 + 1;
      const stride = peakSpan > MAX_PEAKS_PER_COLUMN
        ? Math.ceil(peakSpan / MAX_PEAKS_PER_COLUMN)
        : 1;
      for (let pk = p0; pk <= p1; pk += stride) {
        const ci    = Math.floor(pk / CHUNK_PEAKS);
        const chunk = chunks.get(ci);
        if (!chunk) continue;
        const localIdx = pk % CHUNK_PEAKS;
        for (let ch = 0; ch < channelCount; ch++) {
          const base = (localIdx * channelCount + ch) * 2;
          if (base + 1 >= chunk.length) continue;
          const a = chunk[base]     / 32767;
          const b = chunk[base + 1] / 32767;
          if (a < lo) lo = a;
          if (b > hi) hi = b;
        }
      }

      const y1 = mid - hi * amp;
      const y2 = mid - lo * amp;
      const barH = Math.max(1, y2 - y1);

      if (!dense && !scrolling) {
        // Subtle horizontal smear makes close-up waveforms feel less
        // stroboscopic while keeping the real peak envelope centered and sharp.
        ctx.globalAlpha = baseAlpha * 0.14;
        ctx.fillRect(x - 2, y1, 1, barH);
        ctx.globalAlpha = baseAlpha * 0.22;
        ctx.fillRect(x - 1, y1, 1, barH);
      }
      ctx.globalAlpha = baseAlpha;
      ctx.fillRect(x, y1, 1, barH);
      if (!dense && !scrolling) {
        ctx.globalAlpha = baseAlpha * 0.16;
        ctx.fillRect(x + 1, y1, 1, barH);
      }
    }

    ctx.globalAlpha = 1;
  };

  const hideTile = (canvas: HTMLCanvasElement | null): void => {
    if (!canvas) return;
    canvas.style.display = "none";
    const prevPx = (canvas as HTMLCanvasElement & { _px?: number })._px ?? 0;
    if (prevPx > 0) {
      updateCanvasPixels(prevPx, 0);
      (canvas as HTMLCanvasElement & { _px?: number })._px = 0;
    }
    if (canvas.width !== 1 || canvas.height !== 1) {
      canvas.width  = 1;
      canvas.height = 1;
    }
  };

  // ── Core draw (two tiles) ────────────────────────────────────────────────────

  const drawCanvas = (): void => {
    const p = propsRef.current;
    const w = p.width;
    const h = p.height;
    if (w < 1 || h < 1) return;

    if (p.clipStartPx === undefined) {
      // Simple mode: no viewport tracking — fill first two tiles.
      drawTile(tile0Ref.current, 0, Math.min(w, TILE_SIZE), p);
      if (w > TILE_SIZE) {
        drawTile(tile1Ref.current, TILE_SIZE, Math.min(w - TILE_SIZE, TILE_SIZE), p);
      } else {
        hideTile(tile1Ref.current);
      }
      return;
    }

    // Viewport-aware mode.
    const scrollX    = scrollXRef.current;
    const contentLeft = HEADER_WIDTH + p.clipStartPx;
    const vpLeft  = Math.max(scrollX, HEADER_WIDTH);
    const vpRight = scrollX + window.innerWidth;

    const visLeft  = Math.max(0, vpLeft  - contentLeft);
    const visRight = Math.min(w, vpRight - contentLeft);

    if (visLeft >= visRight) {
      hideTile(tile0Ref.current);
      hideTile(tile1Ref.current);
      return;
    }

    // Tile 0: TILE_SIZE-aligned region containing visLeft.
    const t0Idx   = Math.floor(visLeft / TILE_SIZE);
    const t0Left  = t0Idx * TILE_SIZE;
    const t0Right = Math.min(w, t0Left + TILE_SIZE);
    drawTile(tile0Ref.current, t0Left, t0Right - t0Left, p);

    // Tile 1: next tile if visible area extends beyond tile 0.
    if (visRight > t0Right && t0Right < w) {
      const t1Left  = t0Right;
      const t1Right = Math.min(w, t1Left + TILE_SIZE);
      drawTile(tile1Ref.current, t1Left, t1Right - t1Left, p);
    } else {
      hideTile(tile1Ref.current);
    }
  };

  // Throttle to one redraw per animation frame.
  const scheduleDraw = (): void => {
    drawVersionRef.current++;
    const version = drawVersionRef.current;
    if (rafRef.current !== null) return;
    rafRef.current = requestAnimationFrame(() => {
      rafRef.current = null;
      if (version !== drawVersionRef.current) {
        scheduleDraw();
        return;
      }
      drawCanvas();
    });
  };

  // ── Imperative scroll tracking (no React rerender) ──────────────────────────
  useEffect(() => {
    if (clipStartPx === undefined) return;
    scrollXRef.current = getScrollX();

    const unsubScroll = subscribeScroll((x) => {
      scrollXRef.current = x;
      scheduleDraw();
    });
    // After scroll idle: refine redraw to restore smear and flush any
    // partially-loaded chunks that arrived during fast scroll.
    const unsubIdle = subscribeScrollIdle(() => scheduleDraw());

    return () => {
      unsubScroll();
      unsubIdle();
      if (rafRef.current !== null) { cancelAnimationFrame(rafRef.current); rafRef.current = null; }
    };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [clipStartPx !== undefined]);

  // ── Redraw when React props change ──────────────────────────────────────────
  useLayoutEffect(() => {
    scheduleDraw();
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fileId, levelMeta, width, height, color, muted, selected, clipOffset, clipDuration, sampleRate, sourceDuration, status, clipStartPx]);

  // ── Retry draw when a project finishes opening (projectWaveformReady) ────────
  // loadOpenedProject dispatches this event after committing all pre-warmed
  // peak metadata so waveforms that mounted before peaks arrived can retry.
  useEffect(() => {
    const onReady = () => scheduleDraw();
    window.addEventListener("projectWaveformReady", onReady);
    return () => window.removeEventListener("projectWaveformReady", onReady);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ── Cleanup canvas pixel tracking on unmount ─────────────────────────────────
  useEffect(() => {
    return () => {
      hideTile(tile0Ref.current);
      hideTile(tile1Ref.current);
    };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ── Status overlay logic ─────────────────────────────────────────────────────
  const isReady     = status === "ready" || (!status && !!levelMeta);
  const showLoading = !isReady && (
    status === "loading" || status === "idle" || status === "pending" ||
    status === "copying" || status === "indexing" || status === "generating-peaks" ||
    (!status && !levelMeta)
  );
  const showError = status === "error" || status === "missing";

  return (
    <div
      className="relative overflow-hidden"
      style={{ width, height, background: "rgba(8,12,16,0.72)" }}
    >
      <canvas ref={tile0Ref} className="absolute top-0" style={{ height, opacity: muted ? 0.55 : 1 }} />
      <canvas ref={tile1Ref} className="absolute top-0" style={{ height, opacity: muted ? 0.55 : 1 }} />
      {!isReady && (
        <div className="pointer-events-none absolute inset-0 flex items-center justify-center" aria-hidden>
          {showError ? (
            <span className="rounded border border-white/10 bg-black/20 px-1.5 py-0.5 text-[9px] font-medium tracking-wide text-red-300/80">
              {status === "missing" ? "missing audio" : "waveform error"}
            </span>
          ) : showLoading ? (
            <span className="rounded border border-white/10 bg-black/20 px-1.5 py-0.5 text-[9px] font-medium tabular-nums text-white/45">
              {status === "pending" || status === "copying" || status === "indexing"
                ? "Importing..."
                : progress > 0
                  ? `waveform ${Math.round(progress * 100)}%`
                  : "Waveform pending"}
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
  error: boolean,
) {
  const mid = height / 2;
  ctx.globalAlpha = muted ? 0.18 : 0.28;
  ctx.strokeStyle = error ? "rgba(240,122,114,0.45)" : color;
  ctx.lineWidth   = 1;
  ctx.beginPath();
  for (let x = 0; x < width; x++) {
    const y = mid + Math.sin(x * 0.07) * height * 0.08 + Math.sin(x * 0.017) * height * 0.04;
    if (x === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  }
  ctx.stroke();
  ctx.globalAlpha = 1;
}
