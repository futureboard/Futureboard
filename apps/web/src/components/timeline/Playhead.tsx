import { useEffect, useRef } from "react";
import { transport } from "../../engine/Transport";
import { useTransportStore } from "../../store/transportStore";
import { useUIStore } from "../../store/uiStore";
import { HEADER_WIDTH } from "../../theme";

export function Playhead() {
  const lineRef = useRef<HTMLDivElement>(null);
  const headRef = useRef<HTMLDivElement>(null);
  const rafRef  = useRef<number>(0);
  const lastStore = useRef(0);
  const { pixelsPerSecond } = useUIStore();
  const setPlayheadTime = useTransportStore((s) => s.setPlayheadTime);

  useEffect(() => {
    const tick = () => {
      const t = transport.projectTime;
      const x = HEADER_WIDTH + t * pixelsPerSecond;
      if (lineRef.current) lineRef.current.style.transform = `translateX(${x}px)`;
      if (headRef.current) headRef.current.style.transform = `translateX(${x - 4}px)`;
      const now = performance.now();
      if (now - lastStore.current > 100) { setPlayheadTime(Math.round(t * 100) / 100); lastStore.current = now; }
      rafRef.current = requestAnimationFrame(tick);
    };
    rafRef.current = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(rafRef.current);
  }, [pixelsPerSecond, setPlayheadTime]);

  return (
    <>
      {/* Playhead triangle (sits on ruler) */}
      <div ref={headRef} className="absolute top-0 left-0 pointer-events-none z-20 will-change-transform">
        <svg width={11} height={11} viewBox="0 0 11 11" className="block drop-shadow">
          <polygon points="0,0 11,0 5.5,11" fill="#5aa7ff" />
        </svg>
      </div>
      {/* Vertical line through tracks */}
      <div
        ref={lineRef}
        className="absolute top-0 bottom-0 left-0 pointer-events-none z-10 will-change-transform"
        style={{ width: 1, background: "rgba(90,167,255,0.82)" }}
      />
    </>
  );
}
