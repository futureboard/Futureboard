import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { fmt } from "../data";

type SliderProps = {
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

/** Helix-style horizontal parameter fader. */
export function Knob({
  id,
  name,
  min,
  max,
  value,
  unit,
  defaultValue,
  onChange,
}: SliderProps) {
  const [active, setActive] = useState(false);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const trackRef = useRef<HTMLDivElement>(null);

  const range = max - min;
  const pct = range === 0 ? 0 : (value - min) / range;
  const defPct =
    defaultValue !== undefined && range !== 0
      ? clamp01((defaultValue - min) / range)
      : null;

  const setFromClientX = useCallback(
    (clientX: number) => {
      const el = trackRef.current;
      if (!el) return;
      const rect = el.getBoundingClientRect();
      if (rect.width <= 0) return;
      const p = clamp01((clientX - rect.left) / rect.width);
      onChange(id, min + p * range);
    },
    [id, min, onChange, range],
  );

  const onPointerDown = useCallback(
    (e: ReactPointerEvent<HTMLDivElement>) => {
      if (editing) return;
      e.preventDefault();
      e.currentTarget.setPointerCapture(e.pointerId);
      setActive(true);
      setFromClientX(e.clientX);
    },
    [editing, setFromClientX],
  );

  const onPointerMove = useCallback(
    (e: ReactPointerEvent<HTMLDivElement>) => {
      if (!active) return;
      setFromClientX(e.clientX);
    },
    [active, setFromClientX],
  );

  const onPointerUp = useCallback((e: ReactPointerEvent<HTMLDivElement>) => {
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
    const el = trackRef.current;
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
    <div className={`fader${active ? " active" : ""}`}>
      <div className="fader-meta">
        <span className="fader-label">{name}</span>
        {editing ? (
          <input
            className="fader-input"
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
            className="fader-value"
            title="Click to type a value"
            onClick={startEdit}
          >
            {fmt(value, unit)}
          </button>
        )}
      </div>
      <div
        ref={trackRef}
        className="fader-track"
        role="slider"
        aria-label={name}
        aria-valuemin={min}
        aria-valuemax={max}
        aria-valuenow={value}
        tabIndex={0}
        title={`${name} — drag (Shift+wheel = fine), double-click to reset`}
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
        <div className="fader-fill" style={{ width: `${pct * 100}%` }} />
        {defPct !== null && (
          <span
            className="fader-tick"
            style={{ left: `${defPct * 100}%` }}
            aria-hidden
          />
        )}
        <div className="fader-thumb" style={{ left: `${pct * 100}%` }} />
      </div>
    </div>
  );
}
