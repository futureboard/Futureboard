import { useRef, type ReactNode } from "react";
import {
  clamp,
  estimateGainReductionDb,
  normalizeFB2AParams,
  serializeFB2AParams,
  type FB2AMode,
  type FB2AParams,
} from "../Core";

type Props = {
  params: Record<string, number | string | boolean>;
  enabled: boolean;
  onParamsChange: (patch: Record<string, number | string | boolean>) => void;
  onToggleEnabled: () => void;
  onReset: () => void;
};

const ACCENT = "#e8a84a";
const ACCENT_SOFT = "rgba(232,168,74,0.12)";
const ACCENT_BORDER = "rgba(232,168,74,0.38)";
const PANEL = "#111017";
const SURFACE = "#17151d";
const BORDER = "rgba(255,255,255,0.08)";
const TEXT = "#e4d9c9";
const MUTED = "#8e8170";
const FAINT = "#554b42";
const EDITOR_WIDTH = 820;
const EDITOR_HEIGHT = 300;

export function FB2ACompEditor({ params, enabled, onParamsChange, onToggleEnabled, onReset }: Props) {
  const model = normalizeFB2AParams(params);
  const active = enabled && model.power;

  const update = (patch: Partial<FB2AParams>) => {
    onParamsChange(serializeFB2AParams({ ...model, ...patch }));
  };

  return (
    <div
      className="flex shrink-0 flex-col overflow-hidden rounded-[8px] text-[11px]"
      style={{
        width: EDITOR_WIDTH,
        minWidth: EDITOR_WIDTH,
        height: EDITOR_HEIGHT,
        minHeight: EDITOR_HEIGHT,
        flex: `0 0 ${EDITOR_WIDTH}px`,
        color: TEXT,
        background: "#0b0a0f",
        border: `1px solid ${BORDER}`,
        boxShadow: "0 10px 34px rgba(0,0,0,0.68), 0 1px 0 rgba(255,255,255,0.04) inset",
      }}
    >
      <TopBar
        model={model}
        enabled={enabled}
        active={active}
        onMode={(mode) => update({ mode })}
        onToggleEnabled={onToggleEnabled}
        onReset={onReset}
      />

      <div className="flex min-h-0 flex-1">
        <PrimaryControls model={model} active={active} onUpdate={update} />
        <SecondaryControls model={model} onUpdate={update} />
      </div>
    </div>
  );
}

function TopBar({
  model,
  enabled,
  active,
  onMode,
  onToggleEnabled,
  onReset,
}: {
  model: FB2AParams;
  enabled: boolean;
  active: boolean;
  onMode: (mode: FB2AMode) => void;
  onToggleEnabled: () => void;
  onReset: () => void;
}) {
  return (
    <div
      className="flex h-[34px] shrink-0 items-center gap-2.5 px-3"
      style={{ background: "#14121a", borderBottom: `1px solid ${BORDER}` }}
    >
      <span
        className="h-[9px] w-[9px] shrink-0 rounded-full"
        style={{
          background: active ? ACCENT : enabled ? "#5a4a32" : "#29242a",
          boxShadow: active ? `0 0 10px ${ACCENT}99` : "none",
        }}
      />
      <div className="min-w-[118px] leading-none">
        <div className="text-[12px] font-semibold tracking-[0.12em]" style={{ color: TEXT }}>
          FP-2A COMP
        </div>
      </div>

      <ModeSwitch mode={model.mode} onMode={onMode} />

      <div className="ml-auto flex items-center gap-1.5">
        <StatusReadout model={model} active={active} />
        <HeaderButton active={enabled} onClick={onToggleEnabled}>
          {enabled ? "Bypass" : "Enable"}
        </HeaderButton>
        <HeaderButton active={false} onClick={onReset}>
          Reset
        </HeaderButton>
      </div>
    </div>
  );
}

function PrimaryControls({
  model,
  active,
  onUpdate,
}: {
  model: FB2AParams;
  active: boolean;
  onUpdate: (patch: Partial<FB2AParams>) => void;
}) {
  return (
    <section
      className="flex w-[258px] shrink-0 flex-col border-r p-3"
      style={{ background: PANEL, borderColor: BORDER }}
    >
      <div className="mb-2 flex items-center justify-between">
        <span className="text-[8px] font-semibold uppercase tracking-[0.16em]" style={{ color: FAINT }}>
          Leveling Cell
        </span>
        <span className="text-[8px] tabular-nums" style={{ color: MUTED }}>
          {model.mode === "limit" ? "Fast clamp" : "Smooth level"}
        </span>
      </div>

      <div className="grid flex-1 grid-cols-2 gap-2">
        <OpticalKnob
          label="Peak Reduction"
          value={model.peakReduction}
          min={0}
          max={100}
          display={`${model.peakReduction.toFixed(0)}%`}
          accent={ACCENT}
          active={active}
          resetValue={35}
          onChange={(peakReduction) => onUpdate({ peakReduction })}
        />
        <OpticalKnob
          label="Gain"
          value={model.gainDb}
          min={-12}
          max={24}
          display={fmtDb(model.gainDb)}
          accent={ACCENT}
          active={active}
          resetValue={0}
          onChange={(gainDb) => onUpdate({ gainDb })}
        />
      </div>

      <div className="mt-2 rounded-[6px] p-2" style={{ background: "#0c0b11", border: `1px solid ${BORDER}` }}>
        <ModeSwitch mode={model.mode} onMode={(mode) => onUpdate({ mode })} wide />
        <div className="mt-2 flex items-center justify-between text-[8px] uppercase tracking-[0.12em]">
          <span style={{ color: FAINT }}>Input</span>
          <span style={{ color: ACCENT }}>Optical Cell</span>
          <span style={{ color: FAINT }}>Output</span>
        </div>
      </div>
    </section>
  );
}

function SecondaryControls({
  model,
  onUpdate,
}: {
  model: FB2AParams;
  onUpdate: (patch: Partial<FB2AParams>) => void;
}) {
  return (
    <section className="flex min-w-0 flex-1 flex-col p-3" style={{ background: PANEL }}>
      <div className="mb-2 flex items-center justify-between">
        <span className="text-[8px] font-semibold uppercase tracking-[0.16em]" style={{ color: FAINT }}>
          Tone & Behavior
        </span>
        <span className="text-[8px]" style={{ color: MUTED }}>
          Parallel optical path
        </span>
      </div>

      <div className="grid flex-1 grid-cols-2 gap-x-3 gap-y-2">
        <MiniSlider label="Emphasis" value={model.emphasis} min={0} max={100} display={`${model.emphasis.toFixed(0)}%`} onChange={(emphasis) => onUpdate({ emphasis })} />
        <MiniSlider label="Mix" value={model.mix} min={0} max={100} display={`${model.mix.toFixed(0)}%`} onChange={(mix) => onUpdate({ mix })} />
        <MiniSlider label="Color" value={model.color} min={0} max={100} display={`${model.color.toFixed(0)}%`} onChange={(color) => onUpdate({ color })} />
        <MiniSlider label="Stereo Link" value={model.stereoLink} min={0} max={100} display={`${model.stereoLink.toFixed(0)}%`} onChange={(stereoLink) => onUpdate({ stereoLink })} />
        <MiniSlider label="SC Cut" value={model.sidechainLowCutHz} min={20} max={500} display={`${Math.round(model.sidechainLowCutHz)}Hz`} onChange={(sidechainLowCutHz) => onUpdate({ sidechainLowCutHz })} />
        <MiniSlider label="Trim" value={model.outputTrimDb} min={-12} max={12} display={fmtDb(model.outputTrimDb)} onChange={(outputTrimDb) => onUpdate({ outputTrimDb })} />
      </div>
    </section>
  );
}

function OpticalKnob({
  label,
  value,
  min,
  max,
  display,
  accent,
  active,
  resetValue,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  display: string;
  accent: string;
  active: boolean;
  resetValue: number;
  onChange: (value: number) => void;
}) {
  const dragRef = useRef<{ y: number; value: number } | null>(null);
  const pct = clamp((value - min) / (max - min), 0, 1);
  const start = 222;
  const sweep = 276;
  const angle = start + pct * sweep;
  const cx = 45;
  const cy = 45;
  const radius = 34;
  const trackPath = describeArc(cx, cy, radius, start, start + sweep);
  const valuePath = describeArc(cx, cy, radius, start, angle);

  const setFromPointer = (event: React.PointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    if (!drag) return;
    const fine = event.shiftKey ? 0.22 : 1;
    const next = drag.value + ((drag.y - event.clientY) / 130) * (max - min) * fine;
    onChange(roundValue(clamp(next, min, max), min, max));
  };

  return (
    <div
      className="flex cursor-ns-resize select-none flex-col items-center justify-center rounded-[7px] p-2"
      style={{ background: SURFACE, border: `1px solid ${BORDER}` }}
      title={`${label}: ${display}`}
      onPointerDown={(event) => {
        dragRef.current = { y: event.clientY, value };
        event.currentTarget.setPointerCapture(event.pointerId);
      }}
      onPointerMove={setFromPointer}
      onPointerUp={(event) => {
        dragRef.current = null;
        event.currentTarget.releasePointerCapture(event.pointerId);
      }}
      onPointerCancel={(event) => {
        dragRef.current = null;
        event.currentTarget.releasePointerCapture(event.pointerId);
      }}
      onWheel={(event) => {
        event.preventDefault();
        const fine = event.shiftKey ? 0.2 : 1;
        onChange(roundValue(clamp(value - event.deltaY * (max - min) * 0.0015 * fine, min, max), min, max));
      }}
      onDoubleClick={() => onChange(resetValue)}
    >
      <svg width={90} height={90} viewBox="0 0 90 90">
        <path d={trackPath} fill="none" stroke="rgba(255,255,255,0.08)" strokeWidth="5" strokeLinecap="round" />
        <path
          d={valuePath}
          fill="none"
          stroke={active ? accent : FAINT}
          strokeWidth="5"
          strokeLinecap="round"
          style={{ filter: active ? `drop-shadow(0 0 5px ${accent}88)` : "none" }}
        />
        <circle cx={cx} cy={cy} r="17" fill="#0c0b11" stroke="rgba(255,255,255,0.10)" />
        <line
          x1={cx}
          y1={cy}
          x2={cx + 13 * Math.cos(toRad(angle - 90))}
          y2={cy + 13 * Math.sin(toRad(angle - 90))}
          stroke={active ? accent : MUTED}
          strokeWidth="2.2"
          strokeLinecap="round"
        />
      </svg>
      <div className="text-center leading-none">
        <div className="text-[15px] font-semibold tabular-nums" style={{ color: TEXT }}>
          {display}
        </div>
        <div className="mt-[4px] text-[8px] font-semibold uppercase tracking-[0.1em]" style={{ color: MUTED }}>
          {label}
        </div>
      </div>
    </div>
  );
}

function MiniSlider({
  label,
  value,
  min,
  max,
  display,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  display: string;
  onChange: (value: number) => void;
}) {
  const ref = useRef<HTMLDivElement | null>(null);
  const pct = clamp((value - min) / (max - min), 0, 1);

  const setFromClientX = (clientX: number) => {
    const rect = ref.current?.getBoundingClientRect();
    if (!rect) return;
    const next = min + clamp((clientX - rect.left) / Math.max(1, rect.width), 0, 1) * (max - min);
    onChange(roundValue(next, min, max));
  };

  return (
    <div className="min-w-0 rounded-[6px] p-2" style={{ background: "#0d0c12", border: `1px solid ${BORDER}` }}>
      <div className="mb-1 flex items-center justify-between gap-2">
        <span className="truncate text-[8px] font-semibold uppercase tracking-[0.08em]" style={{ color: MUTED }}>
          {label}
        </span>
        <span className="shrink-0 text-[9px] tabular-nums" style={{ color: TEXT }}>
          {display}
        </span>
      </div>
      <div
        ref={ref}
        className="h-[6px] cursor-ew-resize rounded-full"
        style={{ background: "#07060a", border: `1px solid ${BORDER}` }}
        onPointerDown={(event) => {
          setFromClientX(event.clientX);
          event.currentTarget.setPointerCapture(event.pointerId);
        }}
        onPointerMove={(event) => {
          if (event.buttons !== 1) return;
          setFromClientX(event.clientX);
        }}
        onPointerUp={(event) => event.currentTarget.releasePointerCapture(event.pointerId)}
        onPointerCancel={(event) => event.currentTarget.releasePointerCapture(event.pointerId)}
        onDoubleClick={() => onChange(min <= 0 && max >= 0 ? 0 : (min + max) / 2)}
      >
        <div
          className="h-full rounded-full"
          style={{
            width: `${pct * 100}%`,
            background: ACCENT,
            boxShadow: `0 0 7px ${ACCENT}44`,
          }}
        />
      </div>
    </div>
  );
}

function ModeSwitch({
  mode,
  onMode,
  wide,
}: {
  mode: FB2AMode;
  onMode: (mode: FB2AMode) => void;
  wide?: boolean;
}) {
  return (
    <div
      className={`flex h-[24px] items-center rounded-[5px] p-[2px] ${wide ? "w-full" : ""}`}
      style={{ background: "#0b0a10", border: `1px solid ${BORDER}` }}
    >
      {(["compress", "limit"] as FB2AMode[]).map((m) => (
        <button
          key={m}
          type="button"
          onClick={() => onMode(m)}
          className="h-[18px] flex-1 rounded-[3px] px-2 text-[8.5px] font-semibold capitalize transition-colors"
          style={{
            color: mode === m ? ACCENT : MUTED,
            background: mode === m ? ACCENT_SOFT : "transparent",
            border: `1px solid ${mode === m ? ACCENT_BORDER : "transparent"}`,
          }}
        >
          {m}
        </button>
      ))}
    </div>
  );
}

function StatusReadout({ model, active }: { model: FB2AParams; active: boolean }) {
  const reduction = active ? estimateGainReductionDb(model) : 0;
  return (
    <span
      className="rounded-[4px] px-2 py-[3px] text-[9px] tabular-nums"
      style={{ color: active ? ACCENT : MUTED, background: "#0b0a10", border: `1px solid ${BORDER}` }}
    >
      {active ? `GR -${reduction.toFixed(1)} dB` : "BYPASSED"}
    </span>
  );
}

function HeaderButton({ active, onClick, children }: { active: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="rounded-[4px] px-2 py-[3px] text-[9px] font-semibold transition-colors"
      style={{
        color: active ? ACCENT : MUTED,
        background: active ? ACCENT_SOFT : "#0b0a10",
        border: `1px solid ${active ? ACCENT_BORDER : BORDER}`,
      }}
    >
      {children}
    </button>
  );
}

function describeArc(cx: number, cy: number, r: number, startDeg: number, endDeg: number): string {
  const start = polar(cx, cy, r, endDeg);
  const end = polar(cx, cy, r, startDeg);
  const largeArc = endDeg - startDeg <= 180 ? 0 : 1;
  return `M ${start.x} ${start.y} A ${r} ${r} 0 ${largeArc} 0 ${end.x} ${end.y}`;
}

function polar(cx: number, cy: number, r: number, deg: number) {
  const rad = ((deg - 90) * Math.PI) / 180;
  return { x: cx + r * Math.cos(rad), y: cy + r * Math.sin(rad) };
}

function toRad(deg: number): number {
  return (deg * Math.PI) / 180;
}

function roundValue(value: number, min: number, max: number): number {
  const range = max - min;
  if (range <= 24) return Math.round(value * 10) / 10;
  if (range <= 120) return Math.round(value);
  return Math.round(value / 5) * 5;
}

function fmtDb(db: number): string {
  if (Math.abs(db) < 0.05) return "+0.0dB";
  return `${db > 0 ? "+" : ""}${db.toFixed(1)}dB`;
}
