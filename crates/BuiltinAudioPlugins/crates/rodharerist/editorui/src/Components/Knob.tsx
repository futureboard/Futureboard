import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { fmt } from "../data";

type KnobProps = {
  id: string;
  name: string;
  min: number;
  max: number;
  value: number;
  unit: string;
  defaultValue?: number;
  onChange: (id: string, value: number) => void;
};

const clamp01 = (v: number) => Math.max(0, Math.min(1, v));

/**
 * Rotary sweep, in SVG degrees (0 = +x, clockwise because y points down).
 * 135°..405° puts the dead zone at the bottom, the way a hardware pot reads.
 */
const ANGLE_START = 135;
const ANGLE_SWEEP = 270;
/** Pixels of vertical drag that span the whole range. */
const DRAG_SPAN_PX = 190;

const polar = (cx: number, cy: number, r: number, deg: number) => {
  const rad = (deg * Math.PI) / 180;
  return [cx + r * Math.cos(rad), cy + r * Math.sin(rad)] as const;
};

/** Arc path between two sweep angles on the knob's ring. */
function arc(pct: number, r: number): string {
  const from = ANGLE_START;
  const to = ANGLE_START + ANGLE_SWEEP * clamp01(pct);
  const [x1, y1] = polar(50, 50, r, from);
  const [x2, y2] = polar(50, 50, r, to);
  const large = to - from > 180 ? 1 : 0;
  return `M ${x1} ${y1} A ${r} ${r} 0 ${large} 1 ${x2} ${y2}`;
}

/** Rotary parameter knob: drag vertically, wheel, or type an exact value. */
export function Knob({
  id,
  name,
  min,
  max,
  value,
  unit,
  defaultValue,
  onChange,
}: KnobProps) {
  const [active, setActive] = useState(false);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const dialRef = useRef<HTMLDivElement>(null);
  // Drag origin, so a gesture is measured from where it started rather than
  // accumulating rounding error through the parameter round-trip.
  const dragRef = useRef<{ y: number; pct: number } | null>(null);

  const range = max - min;
  const pct = range === 0 ? 0 : clamp01((value - min) / range);
  const defPct =
    defaultValue !== undefined && range !== 0
      ? clamp01((defaultValue - min) / range)
      : null;

  const angle = ANGLE_START + ANGLE_SWEEP * pct;
  const [px1, py1] = polar(50, 50, 14, angle);
  const [px2, py2] = polar(50, 50, 33, angle);

  const onPointerDown = useCallback(
    (e: ReactPointerEvent<HTMLDivElement>) => {
      if (editing) return;
      e.preventDefault();
      e.currentTarget.setPointerCapture(e.pointerId);
      dragRef.current = { y: e.clientY, pct };
      setActive(true);
    },
    [editing, pct],
  );

  const onPointerMove = useCallback(
    (e: ReactPointerEvent<HTMLDivElement>) => {
      const start = dragRef.current;
      if (!start) return;
      // Up increases. Shift takes a fifth of the gesture for fine trimming.
      const dy = (start.y - e.clientY) / DRAG_SPAN_PX;
      const next = clamp01(start.pct + (e.shiftKey ? dy * 0.2 : dy));
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

  const resetDefault = useCallback(() => {
    if (defaultValue === undefined) return;
    onChange(id, Math.max(min, Math.min(max, defaultValue)));
  }, [defaultValue, id, max, min, onChange]);

  useEffect(() => {
    const el = dialRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const step = e.shiftKey ? 0.01 : 0.03;
      const next = clamp01(pct + (e.deltaY < 0 ? step : -step));
      onChange(id, min + next * range);
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [id, min, onChange, pct, range]);

  const startEdit = useCallback(() => {
    setDraft(String(Number(value.toFixed(2))));
    setEditing(true);
  }, [value]);

  const commitEdit = useCallback(() => {
    const parsed = parseFloat(draft.replace(/[^0-9.+-]/g, ""));
    if (!Number.isNaN(parsed)) {
      onChange(id, Math.max(min, Math.min(max, parsed)));
    }
    setEditing(false);
  }, [draft, id, max, min, onChange]);

  return (
    <div className={`knob${active ? " active" : ""}`}>
      <span className="knob-label">{name}</span>

      <div
        ref={dialRef}
        className="knob-dial"
        role="slider"
        aria-label={name}
        aria-valuemin={min}
        aria-valuemax={max}
        aria-valuenow={value}
        aria-valuetext={fmt(value, unit)}
        tabIndex={0}
        title={`${name} — drag up/down (Shift = fine), double-click to reset`}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerUp}
        onDoubleClick={resetDefault}
        onContextMenu={(e) => {
          e.preventDefault();
          resetDefault();
        }}
        onKeyDown={(e) => {
          const step = e.shiftKey ? 0.01 : 0.05;
          if (e.key === "ArrowRight" || e.key === "ArrowUp") {
            e.preventDefault();
            onChange(id, min + clamp01(pct + step) * range);
          } else if (e.key === "ArrowLeft" || e.key === "ArrowDown") {
            e.preventDefault();
            onChange(id, min + clamp01(pct - step) * range);
          } else if (e.key === "Home") {
            e.preventDefault();
            onChange(id, min);
          } else if (e.key === "End") {
            e.preventDefault();
            onChange(id, max);
          }
        }}
      >
        <svg viewBox="0 0 100 100" aria-hidden>
          <path className="knob-arc-bg" d={arc(1, 43)} />
          <path className="knob-arc" d={arc(pct, 43)} />
          {defPct !== null && (
            <line
              className="knob-tick"
              x1={polar(50, 50, 46, ANGLE_START + ANGLE_SWEEP * defPct)[0]}
              y1={polar(50, 50, 46, ANGLE_START + ANGLE_SWEEP * defPct)[1]}
              x2={polar(50, 50, 50, ANGLE_START + ANGLE_SWEEP * defPct)[0]}
              y2={polar(50, 50, 50, ANGLE_START + ANGLE_SWEEP * defPct)[1]}
            />
          )}
          <circle className="knob-body" cx="50" cy="50" r="33" />
          <line
            className="knob-pointer"
            x1={px1}
            y1={py1}
            x2={px2}
            y2={py2}
          />
        </svg>
      </div>

      {editing ? (
        <input
          className="knob-input"
          autoFocus
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commitEdit}
          onKeyDown={(e) => {
            if (e.key === "Enter") commitEdit();
            else if (e.key === "Escape") setEditing(false);
          }}
        />
      ) : (
        <button
          type="button"
          className="knob-value"
          title="Click to type a value"
          onClick={startEdit}
        >
          {fmt(value, unit)}
        </button>
      )}
    </div>
  );
}
