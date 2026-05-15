import { useCallback } from "react";
import type { AutomationLane, AutomationPoint } from "../../types/daw";
import { HEADER_WIDTH } from "../../theme";
import { useUIStore } from "../../store/uiStore";
import { useProjectStore } from "../../store/projectStore";
import { secondsPerBeat } from "../../utils/musicalTime";
import { sortAutomationPoints, automationValueToY, yToAutomationValue, clampAutomationValue } from "../../utils/automationEval";
import { formatAutomationValue } from "../../utils/automationTargets";

type Props = {
  lane: AutomationLane;
  trackColor: string;
  width: number;
};

const POINT_RADIUS = 4;

export function AutomationLaneView({ lane, trackColor, width }: Props) {
  const pixelsPerSecond = useUIStore((s) => s.pixelsPerSecond);
  const scrollX = useUIStore((s) => s.scrollX);
  const bpm = useProjectStore((s) => s.project.bpm);
  const store = useProjectStore.getState();

  const spb = secondsPerBeat(bpm);
  const timelineWidth = width - HEADER_WIDTH;
  const h = lane.height;
  const target = lane.target;

  const beatToX = (beat: number) => beat * spb * pixelsPerSecond - scrollX;
  const sorted = sortAutomationPoints(lane.points);

  // Build SVG polyline points string
  const linePoints = sorted
    .map((p) => `${beatToX(p.beat).toFixed(1)},${automationValueToY(p.value, target, h).toFixed(1)}`)
    .join(" ");

  // Extend line to left/right edges at first/last values
  const leftVal = sorted.length > 0 ? sorted[0].value : target.defaultValue;
  const rightVal = sorted.length > 0 ? sorted[sorted.length - 1].value : target.defaultValue;
  const leftY = automationValueToY(leftVal, target, h).toFixed(1);
  const rightY = automationValueToY(rightVal, target, h).toFixed(1);
  const edgePoints = `0,${leftY} ${linePoints} ${timelineWidth},${rightY}`;

  const handleLaneClick = useCallback(
    (e: React.MouseEvent<SVGElement>) => {
      const currentTool = useUIStore.getState().currentTool;
      if (currentTool !== "pen" && currentTool !== "automation") return;

      const rect = e.currentTarget.getBoundingClientRect();
      const localX = e.clientX - rect.left;
      const localY = e.clientY - rect.top;
      const beat = Math.max(0, (localX + scrollX) / (spb * pixelsPerSecond));
      const value = clampAutomationValue(yToAutomationValue(localY, target, h), target);

      const point: AutomationPoint = {
        id: crypto.randomUUID(),
        beat,
        value,
        curve: "linear",
        selected: false,
      };
      store.addAutomationPoint(lane.trackId, lane.id, point);
    },
    [lane.id, lane.trackId, scrollX, spb, pixelsPerSecond, target, h, store],
  );

  const handlePointPointerDown = useCallback(
    (e: React.PointerEvent<SVGCircleElement>, point: AutomationPoint) => {
      e.stopPropagation();
      const currentTool = useUIStore.getState().currentTool;
      if (currentTool !== "pointer" && currentTool !== "automation") return;

      const svg = e.currentTarget.closest("svg");
      if (!svg) return;

      const startX = e.clientX;
      const startY = e.clientY;
      const startBeat = point.beat;
      const startValue = point.value;

      e.currentTarget.setPointerCapture(e.pointerId);

      const onMove = (ev: PointerEvent) => {
        const dx = ev.clientX - startX;
        const dy = ev.clientY - startY;
        const newBeat = Math.max(0, startBeat + dx / (spb * pixelsPerSecond));
        const newValue = clampAutomationValue(
          startValue - (dy / h) * (target.max - target.min),
          target,
        );
        store.updateAutomationPoint(lane.trackId, lane.id, point.id, {
          beat: newBeat,
          value: newValue,
        });
      };

      const onUp = () => {
        svg.removeEventListener("pointermove", onMove);
        svg.removeEventListener("pointerup", onUp);
      };

      svg.addEventListener("pointermove", onMove);
      svg.addEventListener("pointerup", onUp);
    },
    [lane.id, lane.trackId, spb, pixelsPerSecond, target, h, store],
  );

  const handleRemoveLane = () => {
    store.removeAutomationLane(lane.trackId, lane.id);
  };

  const handleToggleVisible = () => {
    store.toggleAutomationLaneVisible(lane.trackId, lane.id);
  };

  return (
    <div
      style={{
        display: "flex",
        width: "100%",
        height: h,
        borderTop: "1px solid rgba(255,255,255,0.06)",
        background: "rgba(0,0,0,0.18)",
        flexShrink: 0,
        overflow: "hidden",
      }}
    >
      {/* Lane header */}
      <div
        style={{
          width: HEADER_WIDTH,
          minWidth: HEADER_WIDTH,
          display: "flex",
          alignItems: "center",
          padding: "0 8px 0 16px",
          gap: 6,
          borderRight: "1px solid rgba(255,255,255,0.06)",
          background: "rgba(0,0,0,0.12)",
          overflow: "hidden",
        }}
      >
        <div
          style={{
            width: 3,
            height: 28,
            borderRadius: 2,
            background: trackColor,
            flexShrink: 0,
          }}
        />
        <div style={{ flex: 1, overflow: "hidden" }}>
          <div
            style={{
              fontSize: 10,
              color: "rgba(255,255,255,0.42)",
              textTransform: "uppercase",
              letterSpacing: "0.06em",
              lineHeight: 1.2,
            }}
          >
            Automation
          </div>
          <div
            style={{
              fontSize: 11,
              color: "rgba(255,255,255,0.75)",
              fontWeight: 500,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
          >
            {target.label}
          </div>
        </div>
        <button
          onClick={handleToggleVisible}
          title={lane.visible ? "Hide lane" : "Show lane"}
          style={{
            background: "none",
            border: "none",
            cursor: "pointer",
            color: lane.visible ? "rgba(255,255,255,0.5)" : "rgba(255,255,255,0.25)",
            fontSize: 11,
            padding: "2px 4px",
          }}
        >
          {lane.visible ? "●" : "○"}
        </button>
        <button
          onClick={handleRemoveLane}
          title="Remove automation lane"
          style={{
            background: "none",
            border: "none",
            cursor: "pointer",
            color: "rgba(255,255,255,0.3)",
            fontSize: 13,
            padding: "2px 4px",
            lineHeight: 1,
          }}
        >
          ×
        </button>
      </div>

      {/* Lane canvas */}
      <div style={{ flex: 1, position: "relative", overflow: "hidden" }}>
        <svg
          width={timelineWidth}
          height={h}
          style={{ display: "block", cursor: "crosshair" }}
          onClick={handleLaneClick}
        >
          {/* Default value line */}
          <line
            x1={0}
            y1={automationValueToY(target.defaultValue, target, h)}
            x2={timelineWidth}
            y2={automationValueToY(target.defaultValue, target, h)}
            stroke="rgba(255,255,255,0.08)"
            strokeWidth={1}
            strokeDasharray="4 4"
          />

          {/* Automation curve */}
          {sorted.length > 0 && (
            <polyline
              points={edgePoints}
              fill="none"
              stroke={trackColor}
              strokeWidth={1.5}
              strokeLinejoin="round"
              opacity={0.85}
            />
          )}

          {/* Filled area under curve */}
          {sorted.length > 0 && (
            <polygon
              points={`0,${h} ${edgePoints} ${timelineWidth},${h}`}
              fill={trackColor}
              opacity={0.08}
            />
          )}

          {/* Automation points */}
          {sorted.map((point) => {
            const x = beatToX(point.beat);
            const y = automationValueToY(point.value, target, h);
            if (x < -POINT_RADIUS || x > timelineWidth + POINT_RADIUS) return null;
            return (
              <g key={point.id}>
                <circle
                  cx={x}
                  cy={y}
                  r={POINT_RADIUS + 4}
                  fill="transparent"
                  style={{ cursor: "grab" }}
                  onPointerDown={(e) => handlePointPointerDown(e, point)}
                />
                <circle
                  cx={x}
                  cy={y}
                  r={POINT_RADIUS}
                  fill={trackColor}
                  stroke="rgba(255,255,255,0.7)"
                  strokeWidth={1}
                  style={{ pointerEvents: "none" }}
                />
              </g>
            );
          })}
        </svg>

        {/* Value readout at right edge */}
        {sorted.length > 0 && (
          <div
            style={{
              position: "absolute",
              right: 4,
              top: "50%",
              transform: "translateY(-50%)",
              fontSize: 9,
              color: "rgba(255,255,255,0.35)",
              pointerEvents: "none",
              fontVariantNumeric: "tabular-nums",
            }}
          >
            {formatAutomationValue(rightVal, target)}
          </div>
        )}
      </div>
    </div>
  );
}
