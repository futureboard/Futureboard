import { useCallback, useRef } from "react";

/**
 * Speaker layout for each cab model, as a grid. Purely a drawing hint for the
 * stage diagram — the DSP models the cabinet as a whole, so these positions are
 * illustrative geometry, not per-speaker processing.
 */
const SPEAKER_GRID: Record<string, { cols: number; rows: number; label: string }> = {
  vintage_cab: { cols: 2, rows: 2, label: "4×12" },
  american_2x12: { cols: 2, rows: 1, label: "2×12" },
  tweed_1x12: { cols: 1, rows: 1, label: "1×12" },
  modern_412: { cols: 2, rows: 2, label: "4×12" },
};

/**
 * Physical range the `cab_dist` 0..100 % parameter spans, for display only.
 * The DSP treats distance as a normalized roll-off amount; showing it in cm
 * gives the control a meaningful scale without claiming a measured model.
 */
const DISTANCE_CM_MIN = 0;
const DISTANCE_CM_MAX = 30;

export function distanceCm(pct: number): number {
  return DISTANCE_CM_MIN + (pct / 100) * (DISTANCE_CM_MAX - DISTANCE_CM_MIN);
}

/** `cab_mic` 0 % = dead centre (on-axis), 100 % = speaker edge (off-axis). */
export function positionLabel(pct: number): string {
  if (pct < 12) return "Centre";
  if (pct > 78) return "Edge";
  return "Off-centre";
}

type CabinetStageProps = {
  modelId: string;
  /** `cab_mic` — 0..100 %, centre → edge. */
  position: number;
  /** `cab_dist` — 0..100 %, close → far. */
  distance: number;
  bypassed: boolean;
  onParamChange: (id: string, value: number) => void;
};

/**
 * Interactive cabinet + mic placement stage.
 *
 * A front elevation of the selected cabinet with a draggable mic marker. The
 * marker's horizontal offset from the speaker centre is `cab_mic`; its vertical
 * offset from the grille is `cab_dist`. Both are the *same* two DSP parameters
 * the numeric inspector edits — this is a second view of them, not a second set
 * of controls.
 *
 * The drawing is abstract and technical on purpose: no photo-realism, no
 * textures, no fake grille cloth.
 */
export function CabinetStage({
  modelId,
  position,
  distance,
  bypassed,
  onParamChange,
}: CabinetStageProps) {
  const stageRef = useRef<HTMLDivElement>(null);
  const draggingRef = useRef(false);
  const grid = SPEAKER_GRID[modelId] ?? SPEAKER_GRID.vintage_cab!;

  // The mic travels across the right half of the cab (centre → edge) and
  // outward from the grille. One conversion, shared by render and hit-test.
  const micX = 50 + (position / 100) * 34;
  const micY = 46 + (distance / 100) * 40;

  const setFromPointer = useCallback(
    (clientX: number, clientY: number, fine: boolean) => {
      const el = stageRef.current;
      if (!el) return;
      const rect = el.getBoundingClientRect();
      if (rect.width <= 0 || rect.height <= 0) return;

      const xPct = ((clientX - rect.left) / rect.width) * 100;
      const yPct = ((clientY - rect.top) / rect.height) * 100;

      const rawPos = Math.max(0, Math.min(100, ((xPct - 50) / 34) * 100));
      const rawDist = Math.max(0, Math.min(100, ((yPct - 46) / 40) * 100));

      onParamChange(
        "cab_mic",
        fine ? position + (rawPos - position) * 0.15 : rawPos,
      );
      onParamChange(
        "cab_dist",
        fine ? distance + (rawDist - distance) * 0.15 : rawDist,
      );
    },
    [distance, onParamChange, position],
  );

  const speakers = [];
  for (let r = 0; r < grid.rows; r++) {
    for (let c = 0; c < grid.cols; c++) {
      speakers.push(
        <div
          key={`${r}-${c}`}
          className="cab-speaker"
          style={{
            gridColumn: c + 1,
            gridRow: r + 1,
          }}
        >
          <span className="cab-cone" />
          <span className="cab-dust" />
        </div>,
      );
    }
  }

  return (
    <div className={`cab-stage${bypassed ? " bypassed" : ""}`}>
      <div className="cab-stage-head">
        <span className="cab-stage-title">Mic Placement</span>
        <span className="cab-stage-hint">
          Drag the mic · Shift = fine · double-click to reset
        </span>
      </div>

      <div
        ref={stageRef}
        className="cab-stage-canvas"
        onPointerDown={(e) => {
          e.preventDefault();
          e.currentTarget.setPointerCapture(e.pointerId);
          draggingRef.current = true;
          setFromPointer(e.clientX, e.clientY, e.shiftKey);
        }}
        onPointerMove={(e) => {
          if (draggingRef.current) setFromPointer(e.clientX, e.clientY, e.shiftKey);
        }}
        onPointerUp={(e) => {
          draggingRef.current = false;
          if (e.currentTarget.hasPointerCapture(e.pointerId)) {
            e.currentTarget.releasePointerCapture(e.pointerId);
          }
        }}
        onPointerCancel={() => {
          draggingRef.current = false;
        }}
        onDoubleClick={() => {
          onParamChange("cab_mic", 20);
          onParamChange("cab_dist", 40);
        }}
      >
        <div
          className="cab-box"
          style={{
            ["--cab-cols" as string]: String(grid.cols),
            ["--cab-rows" as string]: String(grid.rows),
          }}
        >
          {speakers}
        </div>

        {/* Centre reference so "on-axis" is readable, not just implied. */}
        <span className="cab-axis" aria-hidden />

        {/* Distance leader line from the grille to the mic. */}
        <svg className="cab-leader" aria-hidden>
          <line
            x1={`${micX}%`}
            y1="46%"
            x2={`${micX}%`}
            y2={`${micY}%`}
            stroke="currentColor"
            strokeWidth="1"
            strokeDasharray="3 3"
          />
        </svg>

        <div
          className="cab-mic"
          role="slider"
          tabIndex={0}
          aria-label="Microphone placement"
          aria-valuetext={`${positionLabel(position)}, ${distanceCm(distance).toFixed(1)} cm`}
          style={{ left: `${micX}%`, top: `${micY}%` }}
          title={`Mic — ${position.toFixed(0)}% off centre, ${distanceCm(distance).toFixed(1)} cm`}
          onKeyDown={(e) => {
            const step = e.shiftKey ? 1 : 5;
            const clamp = (v: number) => Math.max(0, Math.min(100, v));
            if (e.key === "ArrowRight") {
              e.preventDefault();
              onParamChange("cab_mic", clamp(position + step));
            } else if (e.key === "ArrowLeft") {
              e.preventDefault();
              onParamChange("cab_mic", clamp(position - step));
            } else if (e.key === "ArrowDown") {
              e.preventDefault();
              onParamChange("cab_dist", clamp(distance + step));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              onParamChange("cab_dist", clamp(distance - step));
            }
          }}
        >
          <span className="cab-mic-body" aria-hidden />
        </div>

        <div className="cab-stage-readout">
          <span>
            <b>{positionLabel(position)}</b> {position.toFixed(0)}%
          </span>
          <span>{distanceCm(distance).toFixed(1)} cm</span>
          <span className="cab-stage-model">{grid.label}</span>
        </div>
      </div>
    </div>
  );
}
