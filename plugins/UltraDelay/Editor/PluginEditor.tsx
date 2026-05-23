import { useRef } from "react";
import {
  clamp,
  normalizeUltraDelayParams,
  serializeUltraDelayParams,
  type DelayTimeDivision,
  type UltraDelayMode,
  type UltraDelayParams,
} from "../Core";

type Props = {
  params: Record<string, number | string | boolean>;
  enabled: boolean;
  onParamsChange: (patch: Record<string, number | string | boolean>) => void;
  onToggleEnabled: () => void;
  onReset: () => void;
};

const MODES: { id: UltraDelayMode; label: string }[] = [
  { id: "mono",     label: "Mono"   },
  { id: "stereo",   label: "Stereo" },
  { id: "pingpong", label: "Ping"   },
  { id: "dual",     label: "Dual"   },
];

const DIVISIONS: DelayTimeDivision[] = [
  "1/64","1/32","1/16","1/8","1/4","1/2","1/1",
  "1/16T","1/8T","1/4T","1/2T",
  "1/16D","1/8D","1/4D","1/2D",
];

export function UltraDelayEditor({ params, enabled, onParamsChange, onToggleEnabled, onReset }: Props) {
  const model = normalizeUltraDelayParams(params);

  const update = (patch: Partial<UltraDelayParams>) => {
    onParamsChange(serializeUltraDelayParams({ ...model, ...patch }));
  };

  const setTimeL = (div: DelayTimeDivision) =>
    update(model.link ? { timeL: div, timeR: div } : { timeL: div });

  const setTimeR = (div: DelayTimeDivision) => update({ timeR: div });

  return (
    <div
      className="flex h-full max-h-[380px] min-h-[260px] w-[800px] max-w-[1200px] flex-col overflow-hidden rounded-[6px] text-[11px] text-daw-text"
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
        <span className="font-semibold tracking-[0.07em]" style={{ color: "#d0d8e8", fontSize: "11.5px" }}>ULTRADELAY</span>
        <span className="text-[8.5px] uppercase tracking-[0.18em]" style={{ color: "rgba(160,175,200,0.4)" }}>Stereo Delay</span>

        {/* Mode selector */}
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
                color: model.mode === m.id ? "#80d4b0" : "rgba(120,140,170,0.6)",
                background: model.mode === m.id ? "rgba(128,212,176,0.12)" : "transparent",
                border: `1px solid ${model.mode === m.id ? "rgba(128,212,176,0.35)" : "transparent"}`,
              }}
            >
              {m.label}
            </button>
          ))}
        </div>

        {/* Sync toggle */}
        <button
          type="button"
          onClick={() => update({ sync: !model.sync })}
          className="rounded px-2 py-[3px]"
          style={{
            fontSize: "9px",
            fontWeight: 600,
            color: model.sync ? "#80d4b0" : "rgba(120,140,170,0.45)",
            background: model.sync ? "rgba(128,212,176,0.12)" : "#12161e",
            border: `1px solid ${model.sync ? "rgba(128,212,176,0.35)" : "rgba(255,255,255,0.07)"}`,
          }}
        >
          SYNC
        </button>

        <div className="ml-auto flex items-center gap-1.5">
          <ResetButton onClick={onReset} />
        </div>
      </div>

      {/* Body */}
      <div className="flex min-h-0 flex-1">

        {/* Left: Delay lanes */}
        <div
          className="flex w-[220px] shrink-0 flex-col justify-center gap-2 px-3 py-3"
          style={{ borderRight: "1px solid rgba(255,255,255,0.06)", background: "#0f1219" }}
        >
          <DelayLane
            label="L"
            sync={model.sync}
            division={model.timeL}
            timeMs={model.timeMsL}
            onDivision={setTimeL}
            onTimeMsDrag={(d) => update({ timeMsL: clamp(model.timeMsL + d * 10, 1, 4000) })}
            accent="#80d4b0"
          />

          {/* Link button */}
          <div className="flex items-center justify-center">
            <button
              type="button"
              onClick={() => update({ link: !model.link })}
              className="rounded px-3 py-[3px]"
              style={{
                fontSize: "8.5px",
                fontWeight: 600,
                letterSpacing: "0.08em",
                color: model.link ? "#80d4b0" : "#2a3a4a",
                background: model.link ? "rgba(128,212,176,0.1)" : "#0c0f15",
                border: `1px solid ${model.link ? "rgba(128,212,176,0.35)" : "rgba(255,255,255,0.06)"}`,
              }}
            >
              LINK
            </button>
          </div>

          <DelayLane
            label="R"
            sync={model.sync}
            division={model.timeR}
            timeMs={model.timeMsR}
            onDivision={setTimeR}
            onTimeMsDrag={(d) => update({ timeMsR: clamp(model.timeMsR + d * 10, 1, 4000) })}
            disabled={model.link}
            accent="#80d4b0"
          />
        </div>

        {/* Right: Controls */}
        <div className="flex min-w-0 flex-1 flex-col">
          {/* Primary */}
          <div
            className="flex flex-1 items-center gap-1 px-3 py-2"
            style={{ borderBottom: "1px solid rgba(255,255,255,0.05)" }}
          >
            <SectionLabel>Core</SectionLabel>
            <KnobRow>
              <Knob label="Fdbk"   display={`${model.feedback.toFixed(0)}%`}      onDrag={(d) => update({ feedback: clamp(model.feedback + d * 0.5, 0, 98) })} />
              <Knob label="Cross"  display={`${model.crossFeedback.toFixed(0)}%`}  onDrag={(d) => update({ crossFeedback: clamp(model.crossFeedback + d * 0.6, 0, 100) })} disabled={model.mode !== "pingpong"} />
              <Knob label="Width"  display={`${model.width.toFixed(0)}%`}          onDrag={(d) => update({ width: clamp(model.width + d * 0.8, 0, 150) })} />
              <Knob label="Mix"    display={`${model.mix.toFixed(0)}%`}            onDrag={(d) => update({ mix: clamp(model.mix + d * 0.6, 0, 100) })} />
              <Knob label="Output" display={fmtDb(model.outputDb)}                 onDrag={(d) => update({ outputDb: clamp(model.outputDb + d * 0.2, -24, 12) })} />
            </KnobRow>
          </div>

          {/* Secondary */}
          <div className="flex flex-1 items-center gap-1 px-3 py-2">
            <SectionLabel>Filter · FX</SectionLabel>
            <KnobRow>
              <Knob label="Lo Cut" display={formatFreq(model.lowCutHz)}         onDrag={(d) => update({ lowCutHz: clamp(model.lowCutHz * Math.pow(1.012, d), 20, 2000) })} />
              <Knob label="Hi Cut" display={formatFreq(model.highCutHz)}        onDrag={(d) => update({ highCutHz: clamp(model.highCutHz * Math.pow(1.01, d), 1000, 20000) })} />
              <Knob label="Sat"    display={`${model.saturation.toFixed(0)}%`}  onDrag={(d) => update({ saturation: clamp(model.saturation + d * 0.6, 0, 100) })} />
              <Knob label="Mod"    display={`${model.modulation.toFixed(0)}%`}  onDrag={(d) => update({ modulation: clamp(model.modulation + d * 0.6, 0, 100) })} />
              <Knob label="Rate"   display={`${model.modRateHz.toFixed(2)}Hz`}  onDrag={(d) => update({ modRateHz: clamp(model.modRateHz + d * 0.04, 0.01, 10) })} />
              <Knob label="Duck"   display={`${model.ducking.toFixed(0)}%`}     onDrag={(d) => update({ ducking: clamp(model.ducking + d * 0.6, 0, 100) })} />
              <FreezeButton active={model.freeze} onClick={() => update({ freeze: !model.freeze })} />
            </KnobRow>
          </div>
        </div>
      </div>
    </div>
  );
}

// ── DelayLane ──────────────────────────────────────────────────────────────

function DelayLane({
  label, sync, division, timeMs, onDivision, onTimeMsDrag, disabled, accent,
}: {
  label: string;
  sync: boolean;
  division: DelayTimeDivision;
  timeMs: number;
  onDivision: (d: DelayTimeDivision) => void;
  onTimeMsDrag: (delta: number) => void;
  disabled?: boolean;
  accent: string;
}) {
  const dragRef = useRef<{ y: number } | null>(null);

  return (
    <div
      className={`flex items-center gap-2 rounded p-2 ${disabled ? "pointer-events-none opacity-30" : ""}`}
      style={{ background: "#0c0f15", border: "1px solid rgba(255,255,255,0.07)" }}
    >
      <span className="w-3 shrink-0 text-center text-[9px] font-bold" style={{ color: accent }}>{label}</span>

      {sync ? (
        <select
          value={division}
          onChange={(e) => onDivision(e.target.value as DelayTimeDivision)}
          className="flex-1 rounded bg-transparent outline-none"
          style={{ fontSize: "11px", color: "#c0ccd8", border: "none", cursor: "pointer" }}
        >
          {DIVISIONS.map((d) => (
            <option key={d} value={d} style={{ background: "#12161e" }}>{d}</option>
          ))}
        </select>
      ) : (
        <div
          className="flex-1 cursor-ns-resize tabular-nums"
          style={{ fontSize: "11px", color: "#c0ccd8" }}
          onPointerDown={(e) => { dragRef.current = { y: e.clientY }; e.currentTarget.setPointerCapture(e.pointerId); }}
          onPointerMove={(e) => { const s = dragRef.current; if (!s) return; onTimeMsDrag(s.y - e.clientY); dragRef.current = { y: e.clientY }; }}
          onPointerUp={(e) => { dragRef.current = null; e.currentTarget.releasePointerCapture(e.pointerId); }}
        >
          {timeMs < 1000 ? `${Math.round(timeMs)}ms` : `${(timeMs / 1000).toFixed(2)}s`}
        </div>
      )}
    </div>
  );
}

// ── Sub-components ──────────────────────────────────────────────────────────

function PowerLED({ enabled, onToggle }: { enabled: boolean; onToggle: () => void }) {
  return (
    <button
      type="button" onClick={onToggle}
      title={enabled ? "Bypass" : "Enable"}
      className="h-[13px] w-[13px] shrink-0 rounded-full transition-all"
      style={
        enabled
          ? { background: "#80d4b0", boxShadow: "0 0 8px rgba(128,212,176,0.8)", border: "1px solid rgba(128,212,176,0.5)" }
          : { background: "#1e2530", border: "1px solid rgba(255,255,255,0.12)" }
      }
    />
  );
}

function ResetButton({ onClick }: { onClick: () => void }) {
  return (
    <button type="button" onClick={onClick} className="rounded px-2 py-[3px]"
      style={{ fontSize: "10px", color: "#7888a0", background: "#1a2030", border: "1px solid rgba(255,255,255,0.07)" }}>
      Reset
    </button>
  );
}

function SectionLabel({ children }: { children: string }) {
  return (
    <span className="w-[44px] shrink-0 text-center"
      style={{ fontSize: "7.5px", color: "#2a3a50", writingMode: "vertical-rl", textTransform: "uppercase", letterSpacing: "0.12em" }}>
      {children}
    </span>
  );
}

function KnobRow({ children }: { children: React.ReactNode }) {
  return <div className="flex flex-1 items-center justify-evenly gap-1">{children}</div>;
}

function Knob({ label, display, onDrag, disabled }: { label: string; display: string; onDrag: (d: number) => void; disabled?: boolean }) {
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
      <div className="flex w-full items-center justify-center rounded px-1.5"
        style={{ height: "22px", background: "#0c0f15", border: "1px solid rgba(255,255,255,0.08)", fontSize: "11px", color: "#c0ccd8" }}>
        <span className="tabular-nums">{display}</span>
      </div>
    </div>
  );
}

function FreezeButton({ active, onClick }: { active: boolean; onClick: () => void }) {
  return (
    <div className="flex flex-col items-center gap-[4px]" style={{ minWidth: "48px" }}>
      <span className="text-center uppercase tracking-wide" style={{ fontSize: "8px", color: "#3a4c60" }}>Freeze</span>
      <button type="button" onClick={onClick} className="flex w-full items-center justify-center rounded"
        style={{
          height: "22px", fontSize: "9px", fontWeight: 600,
          background: active ? "rgba(128,212,176,0.18)" : "#0c0f15",
          border: `1px solid ${active ? "rgba(128,212,176,0.45)" : "rgba(255,255,255,0.08)"}`,
          color: active ? "#80d4b0" : "#3a4c60",
        }}>
        {active ? "ON" : "OFF"}
      </button>
    </div>
  );
}

function formatFreq(hz: number): string {
  if (hz >= 10000) return `${(hz / 1000).toFixed(0)}k`;
  if (hz >= 1000) return `${(hz / 1000).toFixed(1)}k`;
  return `${Math.round(hz)}`;
}

function fmtDb(db: number): string {
  return `${db >= 0 ? "+" : ""}${db.toFixed(1)}`;
}
