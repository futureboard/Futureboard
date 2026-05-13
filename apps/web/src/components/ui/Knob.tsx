import { useRef, useCallback } from "react";

type Props = {
  value: number;
  min?: number;
  max?: number;
  size?: number;
  label?: string;
  color?: string;
  bipolar?: boolean;   // arc draws from center outward (for pan / EQ knobs)
  onChange: (v: number) => void;
  onChangeEnd?: (v: number) => void;
};

export function Knob({
  value, min = 0, max = 1, size = 38,
  label, color = "#48a6a7", bipolar = false, onChange, onChangeEnd,
}: Props) {
  const startY  = useRef(0);
  const startVal = useRef(0);

  const pct = (value - min) / (max - min);
  const angle = pct * 270 - 135;
  const rad   = (angle * Math.PI) / 180;
  const cx = size / 2;
  const cy = size / 2;
  const r  = size / 2 - 3;

  // indicator dot position
  const ix = cx + r * Math.sin(rad);
  const iy = cy - r * Math.cos(rad);

  // arc geometry
  let arcPath: string | null = null;

  if (bipolar) {
    // zero of the range in normalised units
    const zeroPct   = (0 - min) / (max - min);
    const zeroAngle = zeroPct * 270 - 135;
    const zeroRad   = (zeroAngle * Math.PI) / 180;
    const zx = cx + r * Math.sin(zeroRad);
    const zy = cy - r * Math.cos(zeroRad);

    const delta = pct - zeroPct;
    if (Math.abs(delta) > 0.005) {
      const sweep     = delta > 0 ? 1 : 0;   // clockwise for right / CCW for left
      const largeArc  = Math.abs(delta) > 0.5 ? 1 : 0;
      arcPath = `M ${zx} ${zy} A ${r} ${r} 0 ${largeArc} ${sweep} ${ix} ${iy}`;
    }
  } else {
    // unipolar: arc from min to current
    const startRad = (-135 * Math.PI) / 180;
    const sx = cx + r * Math.sin(startRad);
    const sy = cy - r * Math.cos(startRad);
    if (pct > 0.005) {
      const largeArc = pct > 0.5 ? 1 : 0;
      arcPath = `M ${sx} ${sy} A ${r} ${r} 0 ${largeArc} 1 ${ix} ${iy}`;
    }
  }

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    startY.current   = e.clientY;
    startVal.current = value;
    
    let latestValue = value;

    const onMove = (ev: MouseEvent) => {
      const delta = ((startY.current - ev.clientY) / 150) * (max - min);
      latestValue = Math.round(Math.min(max, Math.max(min, startVal.current + delta)) * 1000) / 1000;
      onChange(latestValue);
    };

    const onUp = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      if (onChangeEnd && latestValue !== startVal.current) {
        onChangeEnd(latestValue);
      }
    };

    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }, [value, min, max, onChange, onChangeEnd]);

  return (
    <div className="flex flex-col items-center gap-0.5 select-none" style={{ width: size }}>
      <svg width={size} height={size} onMouseDown={handleMouseDown} className="cursor-ns-resize block">
        {/* track ring */}
        <circle cx={cx} cy={cy} r={r} fill="#17191d" stroke="#3a424c" strokeWidth={1.5} />
        {/* center tick for bipolar */}
        {bipolar && <line x1={cx} y1={cy - r} x2={cx} y2={cy - r + 3} stroke="#56616e" strokeWidth={1.5} strokeLinecap="round" />}
        {/* arc fill */}
        {arcPath && (
          <path d={arcPath} fill="none" stroke={color} strokeWidth={2} strokeLinecap="round" />
        )}
        {/* indicator dot */}
        <circle cx={ix} cy={iy} r={2} fill={color} />
        {/* centre dot */}
        <circle cx={cx} cy={cy} r={2.5} fill="#56616e" />
      </svg>
      {label && <span className="text-[10px] text-daw-faint">{label}</span>}
    </div>
  );
}
