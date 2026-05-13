import { useCallback, useEffect, useRef } from "react";

type Props = {
  value: number;     // 0–1
  onChange: (v: number) => void;
  onChangeEnd?: (v: number) => void;
  accent?: string;   // hex colour e.g. "#56C7C9"
};

const TRACK_W  = 3;
const THUMB_R  = 11;
const PAD      = THUMB_R + 4;   // top/bottom padding so thumb never clips

function hexToRgb(hex: string): [number, number, number] {
  const h = hex.replace("#", "");
  if (h.length < 6) return [72, 209, 204];
  return [
    parseInt(h.slice(0, 2), 16),
    parseInt(h.slice(2, 4), 16),
    parseInt(h.slice(4, 6), 16),
  ];
}

export function VerticalFader({ value, onChange, onChangeEnd, accent = "#48d1cc" }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const canvasRef    = useRef<HTMLCanvasElement>(null);

  // Keep latest value/accent in a ref so draw() doesn't go stale
  const latest = useRef({ value, accent });
  latest.current = { value, accent };

  // ── geometry helpers ──────────────────────────────────────────────────────

  const getMetrics = useCallback(() => {
    const el = containerRef.current;
    if (!el) return null;
    const W = el.offsetWidth;
    const H = el.offsetHeight;
    const trackTop    = PAD;
    const trackBottom = H - PAD;
    const trackH      = Math.max(1, trackBottom - trackTop);
    return { W, H, cx: W / 2, trackTop, trackBottom, trackH };
  }, []);

  // ── draw ─────────────────────────────────────────────────────────────────

  const draw = useCallback(() => {
    const canvas = canvasRef.current;
    const m = getMetrics();
    if (!canvas || !m) return;

    const { W, H, cx, trackTop, trackBottom, trackH } = m;
    const { value: v, accent: col } = latest.current;
    const dpr = window.devicePixelRatio || 1;

    // Resize backing store only when needed
    const bw = Math.round(W * dpr);
    const bh = Math.round(H * dpr);
    if (canvas.width !== bw || canvas.height !== bh) {
      canvas.width  = bw;
      canvas.height = bh;
      canvas.style.width  = `${W}px`;
      canvas.style.height = `${H}px`;
    }

    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, W, H);

    const thumbY = trackBottom - v * trackH;
    const [r, g, b] = hexToRgb(col);

    // ── track background ──
    ctx.fillStyle = "rgba(255,255,255,0.08)";
    ctx.fillRect(cx - TRACK_W / 2, trackTop, TRACK_W, trackH);

    // ── track fill (below thumb — dimmer) ──
    ctx.fillStyle = "rgba(255,255,255,0.04)";
    ctx.fillRect(cx - TRACK_W / 2, thumbY, TRACK_W, trackBottom - thumbY);

    // ── thumb drop shadow ──
    ctx.save();
    ctx.shadowColor   = "rgba(0,0,0,0.55)";
    ctx.shadowBlur    = 10;
    ctx.shadowOffsetY = 3;
    ctx.beginPath();
    ctx.arc(cx, thumbY, THUMB_R, 0, Math.PI * 2);
    ctx.fillStyle = "rgba(0,0,0,0.01)"; // transparent, just for shadow
    ctx.fill();
    ctx.restore();

    // ── thumb body (radial gradient from top-left highlight) ──
    const grad = ctx.createRadialGradient(
      cx - THUMB_R * 0.3, thumbY - THUMB_R * 0.3, 0,
      cx, thumbY, THUMB_R,
    );
    grad.addColorStop(0.00, "rgba(255,255,255,0.55)");
    grad.addColorStop(0.35, `rgba(${r},${g},${b},0.95)`);
    grad.addColorStop(1.00, `rgba(${r},${g},${b},0.55)`);

    ctx.beginPath();
    ctx.arc(cx, thumbY, THUMB_R, 0, Math.PI * 2);
    ctx.fillStyle = grad;
    ctx.fill();

    // ── thumb rim ──
    ctx.beginPath();
    ctx.arc(cx, thumbY, THUMB_R, 0, Math.PI * 2);
    ctx.strokeStyle = "rgba(255,255,255,0.20)";
    ctx.lineWidth   = 1;
    ctx.stroke();

    // ── centre grip dot ──
    ctx.beginPath();
    ctx.arc(cx, thumbY, 2.5, 0, Math.PI * 2);
    ctx.fillStyle = "rgba(255,255,255,0.88)";
    ctx.fill();
  }, [getMetrics]);

  // Redraw whenever value or accent prop changes
  useEffect(() => { draw(); }, [value, accent, draw]);

  // Resize observer — redraw when container changes size
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => draw());
    ro.observe(el);
    draw();
    return () => ro.disconnect();
  }, [draw]);

  // ── pointer handling ──────────────────────────────────────────────────────

  const dragRef = useRef<{ startY: number; startV: number } | null>(null);

  const valueFromY = useCallback((y: number) => {
    const m = getMetrics();
    if (!m) return latest.current.value;
    const clamped = Math.max(m.trackTop, Math.min(m.trackBottom, y));
    return 1 - (clamped - m.trackTop) / m.trackH;
  }, [getMetrics]);

  const onPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    const rect = containerRef.current!.getBoundingClientRect();
    const localY = e.clientY - rect.top;
    const m = getMetrics();
    if (!m) return;
    const thumbY = m.trackBottom - latest.current.value * m.trackH;

    if (Math.abs(localY - thumbY) <= THUMB_R + 8) {
      // Grabbed the thumb → drag from current value
      dragRef.current = { startY: e.clientY, startV: latest.current.value };
    } else {
      // Clicked track → jump + start drag from new position
      const jumped = Math.min(1, Math.max(0, valueFromY(localY)));
      onChange(Math.round(jumped * 1000) / 1000);
      dragRef.current = { startY: e.clientY, startV: jumped };
    }
  };

  const onPointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!dragRef.current) return;
    const m = getMetrics();
    if (!m) return;
    const deltaV = -(e.clientY - dragRef.current.startY) / m.trackH;
    const next   = Math.min(1, Math.max(0, dragRef.current.startV + deltaV));
    onChange(Math.round(next * 1000) / 1000);
  };

  const onPointerUp = () => {
    if (dragRef.current && onChangeEnd) {
      onChangeEnd(latest.current.value);
    }
    dragRef.current = null;
  };

  return (
    <div
      ref={containerRef}
      className="flex-1 select-none overflow-hidden"
      style={{ cursor: "ns-resize" }}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
    >
      <canvas ref={canvasRef} className="pointer-events-none block" />
    </div>
  );
}
