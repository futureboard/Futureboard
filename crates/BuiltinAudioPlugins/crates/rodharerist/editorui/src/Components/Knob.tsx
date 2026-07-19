import {
  useCallback,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { fmt, KNOB } from "../data";

type KnobProps = {
  id: string;
  name: string;
  min: number;
  max: number;
  value: number;
  unit: string;
  onChange: (id: string, value: number) => void;
};

export function Knob({ id, name, min, max, value, unit, onChange }: KnobProps) {
  const [active, setActive] = useState(false);
  const dragRef = useRef<{
    startY: number;
    startPct: number;
  } | null>(null);

  const range = max - min;
  const pct = range === 0 ? 0 : (value - min) / range;

  const onPointerDown = useCallback(
    (e: ReactPointerEvent<HTMLDivElement>) => {
      e.preventDefault();
      e.currentTarget.setPointerCapture(e.pointerId);
      dragRef.current = {
        startY: e.clientY,
        startPct: pct,
      };
      setActive(true);
    },
    [pct],
  );

  const onPointerMove = useCallback(
    (e: ReactPointerEvent<HTMLDivElement>) => {
      const drag = dragRef.current;
      if (!drag) return;
      let next = drag.startPct + (drag.startY - e.clientY) / 200;
      next = Math.max(0, Math.min(1, next));
      onChange(id, min + next * range);
    },
    [id, min, onChange, range],
  );

  const onPointerUp = useCallback((e: ReactPointerEvent<HTMLDivElement>) => {
    dragRef.current = null;
    setActive(false);
    if (e.currentTarget.hasPointerCapture(e.pointerId)) {
      e.currentTarget.releasePointerCapture(e.pointerId);
    }
  }, []);

  return (
    <div className="knob-unit">
      <div
        className={`knob${active ? " active" : ""}`}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerUp}
      >
        <svg className="knob-arc" viewBox="0 0 100 100">
          <circle
            className="bg"
            cx="50"
            cy="50"
            r="45"
            strokeDasharray={`${KNOB.arc} ${KNOB.C}`}
          />
          <circle
            className="val"
            cx="50"
            cy="50"
            r="45"
            strokeDasharray={`${KNOB.arc * pct} ${KNOB.C}`}
          />
        </svg>
        <div className="knob-cap">
          <div
            className="knob-pointer"
            style={{ transform: `rotate(${-135 + pct * 270}deg)` }}
          />
        </div>
      </div>
      <span className="knob-value">{fmt(value, unit)}</span>
      <span className="knob-label">{name}</span>
    </div>
  );
}
