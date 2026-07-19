import { useEffect, useRef } from "react";

export function ScopeBackdrop() {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const cv = canvasRef.current;
    if (!cv) return;
    const parent = cv.parentElement;
    if (!parent) return;
    const ctx = cv.getContext("2d");
    if (!ctx) return;

    let phase = 0;
    let raf = 0;
    let alive = true;

    const resize = () => {
      cv.width = parent.clientWidth;
      cv.height = parent.clientHeight;
    };
    resize();
    const ro = new ResizeObserver(resize);
    ro.observe(parent);

    const frame = () => {
      if (!alive) return;
      raf = requestAnimationFrame(frame);
      ctx.fillStyle = "rgba(11,13,18,0.2)";
      ctx.fillRect(0, 0, cv.width, cv.height);
      ctx.strokeStyle = getComputedStyle(document.documentElement)
        .getPropertyValue("--cat-color")
        .trim();
      ctx.lineWidth = 1.4;
      ctx.beginPath();
      for (let x = 0; x < cv.width; x++) {
        const y =
          cv.height / 2 +
          Math.sin(x * 0.012 + phase) * 16 * Math.cos(x * 0.003);
        if (x === 0) ctx.moveTo(x, y);
        else ctx.lineTo(x, y);
      }
      ctx.stroke();
      phase += 0.014;
    };
    frame();

    return () => {
      alive = false;
      cancelAnimationFrame(raf);
      ro.disconnect();
    };
  }, []);

  return (
    <div className="fp-scope">
      <canvas ref={canvasRef} />
    </div>
  );
}
