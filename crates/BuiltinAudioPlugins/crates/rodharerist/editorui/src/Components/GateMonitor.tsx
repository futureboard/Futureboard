import {
  useCallback,
  useEffect,
  useRef,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { SILENCE_DBFS } from "../globals";
import { subscribeSmoothedMeters } from "../state/meters";

/**
 * Live gate monitor: the real input level (the gate's sidechain) drawn against
 * a draggable threshold marker on a dB rail. Painted from the rAF-timed
 * ballistic meter view — level updates write straight to their own DOM nodes
 * and never re-render the editor. The threshold is the same `gate_thresh`
 * parameter the knob edits; this is a second view of it, not a second source
 * of truth.
 */

/** 0..1 position of a dB value on the rail. */
const railPct = (db: number, min: number, max: number): number =>
  Math.max(0, Math.min(1, (db - min) / (max - min)));

type GateMonitorProps = {
  paramId: string;
  threshold: number;
  min: number;
  max: number;
  onChange: (id: string, value: number) => void;
};

export function GateMonitor({
  paramId,
  threshold,
  min,
  max,
  onChange,
}: GateMonitorProps) {
  const railRef = useRef<HTMLDivElement>(null);
  const rmsRef = useRef<HTMLDivElement>(null);
  const peakRef = useRef<HTMLDivElement>(null);
  const stateRef = useRef<HTMLSpanElement>(null);
  const dragging = useRef(false);
  // Read by the meter subscription without re-subscribing on every edit.
  const thresholdRef = useRef(threshold);
  thresholdRef.current = threshold;

  useEffect(
    () =>
      subscribeSmoothedMeters((levels) => {
        // The smoothed view floors at the meter silence level; on the wider
        // gate scale that floor reads as "no signal", i.e. the rail's minimum.
        const rmsDb = levels.inRmsDb <= SILENCE_DBFS ? min : levels.inRmsDb;
        const peakDb = levels.inPeakDb <= SILENCE_DBFS ? min : levels.inPeakDb;
        if (rmsRef.current) {
          rmsRef.current.style.width = `${railPct(rmsDb, min, max) * 100}%`;
        }
        if (peakRef.current) {
          peakRef.current.style.left = `${railPct(peakDb, min, max) * 100}%`;
        }
        if (stateRef.current) {
          const open = peakDb >= thresholdRef.current;
          stateRef.current.textContent = open ? "Open" : "Closed";
          stateRef.current.classList.toggle("open", open);
        }
      }),
    [min, max],
  );

  const thresholdFromPointer = useCallback(
    (clientX: number) => {
      const rail = railRef.current;
      if (!rail) return;
      const rect = rail.getBoundingClientRect();
      if (rect.width <= 0) return;
      const pct = Math.max(0, Math.min(1, (clientX - rect.left) / rect.width));
      onChange(paramId, min + pct * (max - min));
    },
    [max, min, onChange, paramId],
  );

  const onPointerDown = useCallback(
    (e: ReactPointerEvent<HTMLDivElement>) => {
      e.preventDefault();
      e.currentTarget.setPointerCapture(e.pointerId);
      dragging.current = true;
      thresholdFromPointer(e.clientX);
    },
    [thresholdFromPointer],
  );

  const onPointerMove = useCallback(
    (e: ReactPointerEvent<HTMLDivElement>) => {
      if (!dragging.current) return;
      thresholdFromPointer(e.clientX);
    },
    [thresholdFromPointer],
  );

  const onPointerUp = useCallback((e: ReactPointerEvent<HTMLDivElement>) => {
    dragging.current = false;
    if (e.currentTarget.hasPointerCapture(e.pointerId)) {
      e.currentTarget.releasePointerCapture(e.pointerId);
    }
  }, []);

  // The smoothed subscription paints immediately on mount, so the initial
  // inline styles only cover the instant before that first callback.
  return (
    <div className="gate-monitor">
      <div className="gate-monitor-head">
        <span className="gate-monitor-title">Input vs Threshold</span>
        <span ref={stateRef} className="gate-state">
          Closed
        </span>
      </div>
      <div
        ref={railRef}
        className="gate-rail"
        role="slider"
        aria-label="Gate threshold"
        aria-valuemin={min}
        aria-valuemax={max}
        aria-valuenow={threshold}
        aria-valuetext={`${threshold.toFixed(0)} dB`}
        title="Click or drag to set the threshold"
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerUp}
      >
        <div ref={rmsRef} className="gate-level-rms" style={{ width: 0 }} />
        <div ref={peakRef} className="gate-level-peak" style={{ left: 0 }} />
        <div
          className="gate-thresh-handle"
          style={{ left: `${railPct(threshold, min, max) * 100}%` }}
        >
          <span className="gate-thresh-readout">{threshold.toFixed(0)} dB</span>
        </div>
      </div>
      <div className="gate-scale" aria-hidden>
        {[-80, -60, -40, -20, 0].map((db) => (
          <span key={db} style={{ left: `${railPct(db, min, max) * 100}%` }}>
            {db}
          </span>
        ))}
      </div>
    </div>
  );
}
