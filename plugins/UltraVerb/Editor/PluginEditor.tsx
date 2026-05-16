import { useRef } from "react";
import {
  clamp,
  normalizeUltraVerbParams,
  serializeUltraVerbParams,
  type UltraVerbMode,
  type UltraVerbParams,
} from "../Core";

type Props = {
  params: Record<string, number | string | boolean>;
  enabled: boolean;
  onParamsChange: (patch: Record<string, number | string | boolean>) => void;
  onToggleEnabled: () => void;
  onReset: () => void;
};

const MODES: { id: UltraVerbMode; label: string }[] = [
  { id: "room",  label: "Room"  },
  { id: "plate", label: "Plate" },
  { id: "hall",  label: "Hall"  },
  { id: "space", label: "Space" },
];

export function UltraVerbEditor({ params, enabled, onParamsChange, onToggleEnabled, onReset }: Props) {
  const model = normalizeUltraVerbParams(params);

  const update = (patch: Partial<UltraVerbParams>) => {
    onParamsChange(serializeUltraVerbParams({ ...model, ...patch }));
  };

  return (
    <div
      className="flex h-full max-h-[380px] min-h-[260px] w-[760px] max-w-[1200px] flex-col overflow-hidden rounded-[6px] text-[11px] text-daw-text"
      style={{
        background: "#171b22",
        border: "1px solid rgba(255,255,255,0.09)",
        boxShadow: "0 4px 28px rgba(0,0,0,0.6), 0 1px 0 rgba(255,255,255,0.04) inset",
      }}
    >
      {/* Header */}
      <div
        className="flex h-8 shrink-0 items-center gap-3 px-3"
        style={{
          background: "linear-gradient(180deg,#1c2030 0%,#181d29 100%)",
          borderBottom: "1px solid rgba(255,255,255,0.07)",
        }}
      >
        <PowerLED enabled={enabled} onToggle={onToggleEnabled} />
        <span className="font-semibold tracking-[0.07em]" style={{ color: "#d0d8e8", fontSize: "11.5px" }}>ULTRAVERB</span>
        <span className="text-[8.5px] uppercase tracking-[0.18em]" style={{ color: "rgba(160,175,200,0.4)" }}>Algorithmic Reverb</span>

        <div className="flex items-center gap-[3px]">
          {MODES.map((m) => (
            <button
              key={m.id}
              type="button"
              onClick={() => update({ mode: m.id })}
              className="rounded px-2 py-[3px] transition-colors"
              style={{
                fontSize: "9px",
                fontWeight: model.mode === m.id ? 600 : 400,
                color: model.mode === m.id ? "#7cc7ff" : "rgba(120,140,170,0.6)",
                background: model.mode === m.id ? "rgba(124,199,255,0.12)" : "transparent",
                border: `1px solid ${model.mode === m.id ? "rgba(124,199,255,0.35)" : "transparent"}`,
              }}
            >
              {m.label}
            </button>
          ))}
        </div>

        <div className="ml-auto flex items-center gap-1.5">
          <ResetButton onClick={onReset} />
        </div>
      </div>

      {/* Body */}
      <div className="flex min-h-0 flex-1">

        {/* Decay preview */}
        <div
          className="flex w-[160px] shrink-0 flex-col items-center justify-center gap-1.5 p-3"
          style={{ borderRight: "1px solid rgba(255,255,255,0.06)", background: "#0f1219" }}
        >
          <DecayCurve decay={model.decay} size={model.size} enabled={enabled && model.power} />
          <span className="mt-1 text-[8.5px] uppercase tracking-widest" style={{ color: "#2a3a4a" }}>
            Space
          </span>
        </div>

        {/* Controls */}
        <div className="flex min-w-0 flex-1 flex-col">
          {/* Primary row */}
          <div
            className="flex flex-1 items-center gap-1 px-3 py-2"
            style={{ borderBottom: "1px solid rgba(255,255,255,0.05)" }}
          >
            <SectionLabel>Primary</SectionLabel>
            <KnobRow>
              <Knob label="Mix"      display={`${model.mix.toFixed(0)}%`}      onDrag={(d) => update({ mix: clamp(model.mix + d * 0.6, 0, 100) })} />
              <Knob label="Decay"    display={`${model.decay.toFixed(1)}s`}     onDrag={(d) => update({ decay: clamp(model.decay + d * 0.08, 0.1, 20) })} />
              <Knob label="Size"     display={`${model.size.toFixed(0)}%`}      onDrag={(d) => update({ size: clamp(model.size + d * 0.6, 0, 100) })} />
              <Knob label="Pre Dly"  display={`${model.preDelayMs.toFixed(0)}ms`} onDrag={(d) => update({ preDelayMs: clamp(model.preDelayMs + d * 1.2, 0, 250) })} />
              <Knob label="Diff"     display={`${model.diffusion.toFixed(0)}%`} onDrag={(d) => update({ diffusion: clamp(model.diffusion + d * 0.6, 0, 100) })} />
              <Knob label="Damp"     display={`${model.damping.toFixed(0)}%`}   onDrag={(d) => update({ damping: clamp(model.damping + d * 0.6, 0, 100) })} />
            </KnobRow>
          </div>

          {/* Secondary row */}
          <div className="flex flex-1 items-center gap-1 px-3 py-2">
            <SectionLabel>Filter · Output</SectionLabel>
            <KnobRow>
              <Knob label="Lo Cut"   display={formatFreq(model.lowCutHz)}        onDrag={(d) => update({ lowCutHz:  clamp(model.lowCutHz  * Math.pow(1.01, d), 20, 1000) })} />
              <Knob label="Hi Cut"   display={formatFreq(model.highCutHz)}       onDrag={(d) => update({ highCutHz: clamp(model.highCutHz * Math.pow(1.01, d), 1000, 20000) })} />
              <Knob label="Width"    display={`${model.width.toFixed(0)}%`}      onDrag={(d) => update({ width: clamp(model.width + d * 0.8, 0, 150) })} />
              <Knob label="Mod"      display={`${model.modulation.toFixed(0)}%`} onDrag={(d) => update({ modulation: clamp(model.modulation + d * 0.6, 0, 100) })} />
              <Knob label="Rate"     display={`${model.modRateHz.toFixed(2)}Hz`} onDrag={(d) => update({ modRateHz: clamp(model.modRateHz + d * 0.02, 0.05, 5) })} />
              <Knob label="Early"    display={fmtDb(model.earlyLevel)}           onDrag={(d) => update({ earlyLevel: clamp(model.earlyLevel + d * 0.3, -60, 6) })} />
              <Knob label="Late"     display={fmtDb(model.lateLevel)}            onDrag={(d) => update({ lateLevel:  clamp(model.lateLevel  + d * 0.3, -60, 6) })} />
              <Knob label="Output"   display={fmtDb(model.outputDb)}             onDrag={(d) => update({ outputDb:  clamp(model.outputDb   + d * 0.2, -24, 12) })} />
              <FreezeButton active={model.freeze} onClick={() => update({ freeze: !model.freeze })} />
            </KnobRow>
          </div>
        </div>
      </div>
    </div>
  );
}

// ── Sub-components ──────────────────────────────────────────────────────────

function PowerLED({ enabled, onToggle }: { enabled: boolean; onToggle: () => void }) {
  return (
    <button
      type="button"
      onClick={onToggle}
      title={enabled ? "Bypass" : "Enable"}
      className="h-[13px] w-[13px] shrink-0 rounded-full transition-all"
      style={
        enabled
          ? { background: "#7cc7ff", boxShadow: "0 0 8px rgba(124,199,255,0.8)", border: "1px solid rgba(124,199,255,0.5)" }
          : { background: "#1e2530", border: "1px solid rgba(255,255,255,0.12)" }
      }
    />
  );
}

function ResetButton({ onClick }: { onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="rounded px-2 py-[3px]"
      style={{ fontSize: "10px", color: "#7888a0", background: "#1a2030", border: "1px solid rgba(255,255,255,0.07)" }}
    >
      Reset
    </button>
  );
}

function SectionLabel({ children }: { children: string }) {
  return (
    <span
      className="w-[44px] shrink-0 text-center"
      style={{ fontSize: "7.5px", color: "#2a3a50", writingMode: "vertical-rl", textOrientation: "mixed", textTransform: "uppercase", letterSpacing: "0.12em" }}
    >
      {children}
    </span>
  );
}

function KnobRow({ children }: { children: React.ReactNode }) {
  return <div className="flex flex-1 items-center justify-evenly gap-1">{children}</div>;
}

function Knob({ label, display, onDrag, disabled }: { label: string; display: string; onDrag: (delta: number) => void; disabled?: boolean }) {
  const ref = useRef<{ y: number } | null>(null);
  return (
    <div
      className={`flex flex-col items-center gap-[4px] ${disabled ? "pointer-events-none opacity-25" : "cursor-ns-resize"}`}
      style={{ minWidth: "48px" }}
      onPointerDown={(e) => { ref.current = { y: e.clientY }; e.currentTarget.setPointerCapture(e.pointerId); }}
      onPointerMove={(e) => { const s = ref.current; if (!s) return; onDrag(s.y - e.clientY); ref.current = { y: e.clientY }; }}
      onPointerUp={(e) => { ref.current = null; e.currentTarget.releasePointerCapture(e.pointerId); }}
    >
      <span className="text-center uppercase tracking-wide" style={{ fontSize: "8px", color: "#3a4c60" }}>{label}</span>
      <div
        className="flex w-full items-center justify-center rounded px-1.5"
        style={{ height: "22px", background: "#0c0f15", border: "1px solid rgba(255,255,255,0.08)", fontSize: "11px", color: "#c0ccd8" }}
      >
        <span className="tabular-nums">{display}</span>
      </div>
    </div>
  );
}

function FreezeButton({ active, onClick }: { active: boolean; onClick: () => void }) {
  return (
    <div className="flex flex-col items-center gap-[4px]" style={{ minWidth: "48px" }}>
      <span className="text-center uppercase tracking-wide" style={{ fontSize: "8px", color: "#3a4c60" }}>Freeze</span>
      <button
        type="button"
        onClick={onClick}
        className="flex w-full items-center justify-center rounded"
        style={{
          height: "22px",
          fontSize: "9px",
          fontWeight: 600,
          background: active ? "rgba(124,199,255,0.18)" : "#0c0f15",
          border: `1px solid ${active ? "rgba(124,199,255,0.45)" : "rgba(255,255,255,0.08)"}`,
          color: active ? "#7cc7ff" : "#3a4c60",
        }}
      >
        {active ? "ON" : "OFF"}
      </button>
    </div>
  );
}

function DecayCurve({ decay, size, enabled }: { decay: number; size: number; enabled: boolean }) {
  const w = 128;
  const h = 72;
  const t = clamp(decay / 20, 0, 1);
  const s = clamp(size / 100, 0, 1);

  // Build exponential decay SVG path
  const pts: string[] = [];
  for (let i = 0; i <= 40; i++) {
    const x = (i / 40) * w;
    const tVal = (i / 40);
    const y = h - h * Math.exp(-tVal * (5 - t * 4)) * (0.4 + s * 0.6);
    pts.push(`${i === 0 ? "M" : "L"}${x.toFixed(1)},${y.toFixed(1)}`);
  }
  const pathD = pts.join(" ");

  const color = enabled ? "#7cc7ff" : "#2a3a4a";
  const fillColor = enabled ? "rgba(124,199,255,0.08)" : "rgba(42,58,74,0.05)";

  return (
    <svg width={w} height={h} viewBox={`0 0 ${w} ${h}`} style={{ display: "block" }}>
      <path d={`${pathD} L${w},${h} L0,${h} Z`} fill={fillColor} />
      <path d={pathD} fill="none" stroke={color} strokeWidth="1.5" strokeLinecap="round" />
    </svg>
  );
}

function formatFreq(hz: number): string {
  if (hz >= 10000) return `${(hz / 1000).toFixed(0)}k`;
  if (hz >= 1000) return `${(hz / 1000).toFixed(1)}k`;
  return `${Math.round(hz)}`;
}

function fmtDb(db: number): string {
  if (db <= -59) return "-∞";
  return `${db >= 0 ? "+" : ""}${db.toFixed(1)}`;
}
