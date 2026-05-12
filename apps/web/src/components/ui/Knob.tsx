import { useRef, useCallback } from "react";

type Props = {
  value: number;
  min?: number;
  max?: number;
  size?: number;
  label?: string;
  color?: string;
  onChange: (v: number) => void;
};

export function Knob({ value, min = 0, max = 1, size = 32, label, color = "#5aa7ff", onChange }: Props) {
  const startY = useRef(0);
  const startVal = useRef(0);

  const pct = (value - min) / (max - min);
  const angle = pct * 270 - 135;
  const rad = (angle * Math.PI) / 180;
  const cx = size / 2;
  const cy = size / 2;
  const r = size / 2 - 3;
  const ix = cx + r * Math.sin(rad);
  const iy = cy - r * Math.cos(rad);
  const startRad = (-135 * Math.PI) / 180;
  const sx = cx + r * Math.sin(startRad);
  const sy = cy - r * Math.cos(startRad);
  const largeArc = pct > 0.5 ? 1 : 0;

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    startY.current = e.clientY;
    startVal.current = value;
    const onMove = (ev: MouseEvent) => {
      const delta = ((startY.current - ev.clientY) / 150) * (max - min);
      onChange(Math.round(Math.min(max, Math.max(min, startVal.current + delta)) * 1000) / 1000);
    };
    const onUp = () => { window.removeEventListener("mousemove", onMove); window.removeEventListener("mouseup", onUp); };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }, [value, min, max, onChange]);

  return (
    <div className="flex flex-col items-center gap-0.5 select-none" style={{ width: size }}>
      <svg width={size} height={size} onMouseDown={handleMouseDown} className="cursor-ns-resize block">
        <circle cx={cx} cy={cy} r={r} fill="none" stroke="#303943" strokeWidth={2} />
        {pct > 0 && (
          <path d={`M ${sx} ${sy} A ${r} ${r} 0 ${largeArc} 1 ${ix} ${iy}`} fill="none" stroke={color} strokeWidth={2} strokeLinecap="round" />
        )}
        <circle cx={ix} cy={iy} r={2} fill={color} />
        <circle cx={cx} cy={cy} r={2.5} fill="#3d4854" />
      </svg>
      {label && <span className="text-[10px] text-daw-faint">{label}</span>}
    </div>
  );
}
