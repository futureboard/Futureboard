import { useEffect, useRef, useState, type ReactNode } from "react";
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

type ViewMode = "macro" | "expert";
type SectionId = "tone" | "space" | "motion" | "output";
type SliderCurve = "linear" | "log";

const EDITOR_WIDTH = 750;
const EDITOR_HEIGHT = 300;

const C = {
  bg: "#080b11",
  bg2: "#0b1018",
  panel: "#111722",
  panel2: "#151c28",
  surface: "#1a2230",
  surface2: "#0d121b",
  border: "rgba(255,255,255,0.08)",
  borderStrong: "rgba(255,255,255,0.14)",
  text: "#dce7f3",
  dim: "#91a1b6",
  faint: "#566579",
  mute: "#303a48",
  cyan: "#72d7d7",
  cyanSoft: "rgba(114,215,215,0.14)",
  cyanBorder: "rgba(114,215,215,0.38)",
  purple: "#a78bfa",
  purpleSoft: "rgba(167,139,250,0.14)",
  green: "#80d18a",
  danger: "#ef6b6b",
};

const MODES: { id: UltraVerbMode; label: string }[] = [
  { id: "room", label: "Room" },
  { id: "plate", label: "Plate" },
  { id: "hall", label: "Hall" },
  { id: "space", label: "Space" },
];

const PRESETS: { id: string; label: string; patch: Partial<UltraVerbParams> }[] = [
  {
    id: "vocal-hall",
    label: "Vocal Hall",
    patch: {
      mode: "hall",
      mix: 24,
      size: 58,
      decay: 2.8,
      preDelayMs: 34,
      diffusion: 76,
      damping: 44,
      lowCutHz: 160,
      highCutHz: 11800,
      width: 112,
      modulation: 10,
      modRateHz: 0.28,
      earlyLevel: -9,
      lateLevel: 0,
      outputDb: 0,
    },
  },
  {
    id: "glass-plate",
    label: "Glass Plate",
    patch: {
      mode: "plate",
      mix: 18,
      size: 42,
      decay: 1.9,
      preDelayMs: 12,
      diffusion: 88,
      damping: 28,
      lowCutHz: 220,
      highCutHz: 15000,
      width: 126,
      modulation: 18,
      modRateHz: 0.54,
      earlyLevel: -11,
      lateLevel: -1,
      outputDb: -0.5,
    },
  },
  {
    id: "small-room",
    label: "Small Room",
    patch: {
      mode: "room",
      mix: 14,
      size: 28,
      decay: 0.8,
      preDelayMs: 6,
      diffusion: 52,
      damping: 62,
      lowCutHz: 110,
      highCutHz: 9000,
      width: 92,
      modulation: 4,
      modRateHz: 0.18,
      earlyLevel: -5,
      lateLevel: -3,
      outputDb: 0,
    },
  },
  {
    id: "deep-space",
    label: "Deep Space",
    patch: {
      mode: "space",
      mix: 38,
      size: 86,
      decay: 9.5,
      preDelayMs: 74,
      diffusion: 64,
      damping: 22,
      lowCutHz: 260,
      highCutHz: 7200,
      width: 145,
      modulation: 36,
      modRateHz: 0.16,
      earlyLevel: -16,
      lateLevel: 1.5,
      outputDb: -1.5,
    },
  },
];

const SECTION_LABEL: Record<SectionId, string> = {
  tone: "Tone",
  space: "Space",
  motion: "Motion",
  output: "Output",
};

const FIELD_POINTS = Array.from({ length: 72 }, (_, i) => {
  const a = Math.sin(i * 12.9898) * 43758.5453;
  const b = Math.sin((i + 4.7) * 78.233) * 24634.6345;
  const c = Math.sin((i + 9.1) * 31.416) * 12412.917;
  return {
    x: fract(a),
    y: fract(b),
    z: fract(c),
    s: 0.45 + fract(a + b) * 1.35,
  };
});

export function UltraVerbEditor({ params, enabled, onParamsChange, onToggleEnabled, onReset }: Props) {
  const model = normalizeUltraVerbParams(params);
  const active = enabled && model.power && !model.freeze;
  const [viewMode, setViewMode] = useState<ViewMode>("macro");
  const [expandedSection, setExpandedSection] = useState<SectionId>("tone");
  const [presetId, setPresetId] = useState("custom");

  const update = (patch: Partial<UltraVerbParams>) => {
    setPresetId("custom");
    onParamsChange(serializeUltraVerbParams({ ...model, ...patch }));
  };

  const applyPreset = (id: string) => {
    setPresetId(id);
    const preset = PRESETS.find((p) => p.id === id);
    if (!preset) return;
    onParamsChange(serializeUltraVerbParams({ ...model, ...preset.patch }));
  };

  const selectSection = (section: SectionId) => {
    setExpandedSection(section);
    setViewMode("expert");
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
        color: C.text,
        background: C.bg,
        border: `1px solid ${C.border}`,
        boxShadow: "0 10px 34px rgba(0,0,0,0.68), 0 1px 0 rgba(255,255,255,0.04) inset",
      }}
    >
      <TopBar
        model={model}
        enabled={enabled}
        presetId={presetId}
        viewMode={viewMode}
        onMode={(mode) => update({ mode })}
        onPreset={applyPreset}
        onViewMode={setViewMode}
        onLive={() => update({ freeze: !model.freeze })}
        onReset={() => {
          setPresetId("custom");
          onReset();
        }}
        onToggleEnabled={onToggleEnabled}
      />

      <div className="flex min-h-0 flex-1">
        <div className="flex min-w-0 flex-1 flex-col">
          <div className="flex min-h-0 flex-1">
            <div
              className="relative min-w-0 flex-1"
              style={{ background: "#090d14", borderRight: `1px solid ${C.border}` }}
            >
              <SpaceCanvas
                active={active}
                mode={model.mode}
                size={model.size}
                decay={model.decay}
                diffusion={model.diffusion}
                mix={model.mix}
              />
              <VisualReadout model={model} active={active} />
            </div>

            <div
              className="flex w-[230px] shrink-0 flex-col"
              style={{ background: C.panel, borderRight: `1px solid ${C.border}` }}
            >
              <div className="grid flex-1 grid-cols-2 gap-2 p-2.5">
                <PrimaryKnob
                  label="Size"
                  value={model.size}
                  min={0}
                  max={100}
                  display={`${model.size.toFixed(0)}%`}
                  active={active}
                  onChange={(size) => update({ size })}
                />
                <PrimaryKnob
                  label="Decay"
                  value={model.decay}
                  min={0.1}
                  max={20}
                  display={`${model.decay.toFixed(1)}s`}
                  active={active}
                  accent={C.purple}
                  onChange={(decay) => update({ decay })}
                />
                <PrimaryKnob
                  label="Pre-delay"
                  value={model.preDelayMs}
                  min={0}
                  max={250}
                  display={`${model.preDelayMs.toFixed(0)}ms`}
                  active={active}
                  onChange={(preDelayMs) => update({ preDelayMs })}
                />
                <PrimaryKnob
                  label="Mix"
                  value={model.mix}
                  min={0}
                  max={100}
                  display={`${model.mix.toFixed(0)}%`}
                  active={active}
                  accent={C.purple}
                  onChange={(mix) => update({ mix })}
                />
              </div>

              {viewMode === "macro" && (
                <SectionDock activeSection={expandedSection} onSelect={selectSection} model={model} />
              )}
            </div>
          </div>

          {viewMode === "expert" && (
            <ExpertStrip
              model={model}
              activeSection={expandedSection}
              onSection={setExpandedSection}
              onUpdate={update}
            />
          )}
        </div>

        <OutputMeters model={model} active={active} />
      </div>
    </div>
  );
}

function TopBar({
  model,
  enabled,
  presetId,
  viewMode,
  onMode,
  onPreset,
  onViewMode,
  onLive,
  onReset,
  onToggleEnabled,
}: {
  model: UltraVerbParams;
  enabled: boolean;
  presetId: string;
  viewMode: ViewMode;
  onMode: (mode: UltraVerbMode) => void;
  onPreset: (id: string) => void;
  onViewMode: (mode: ViewMode) => void;
  onLive: () => void;
  onReset: () => void;
  onToggleEnabled: () => void;
}) {
  return (
    <div
      className="flex h-[34px] shrink-0 items-center gap-2 px-2.5"
      style={{
        background: "#121925",
        borderBottom: `1px solid ${C.border}`,
      }}
    >
      <div className="flex min-w-[92px] items-center gap-2">
        {/*<span
          className="h-[9px] w-[9px] rounded-full"
          style={{
            background: active ? C.cyan : enabled ? C.purple : C.mute,
            boxShadow: active ? `0 0 10px ${C.cyan}` : enabled ? `0 0 8px ${C.purple}66` : "none",
          }}
        />*/}
        <div className="leading-none">
          <div className="text-[12px] font-semibold tracking-[0.13em]" style={{ color: C.text }}>
            ULTRAVERB
          </div>
        </div>
      </div>

      <div className="flex h-[23px] items-center rounded-[5px] p-[2px]" style={{ background: "#0c111a", border: `1px solid ${C.border}` }}>
        {MODES.map((mode) => (
          <button
            key={mode.id}
            type="button"
            onClick={() => onMode(mode.id)}
            className="h-[17px] rounded-[3px] px-1.5 text-[8.5px] font-semibold transition-colors"
            style={{
              color: model.mode === mode.id ? C.cyan : C.faint,
              background: model.mode === mode.id ? C.cyanSoft : "transparent",
              border: `1px solid ${model.mode === mode.id ? C.cyanBorder : "transparent"}`,
            }}
          >
            {mode.label}
          </button>
        ))}
      </div>

      <select
        value={presetId}
        onChange={(event) => onPreset(event.target.value)}
        aria-label="Preset selector"
        className="h-[23px] w-[104px] rounded-[5px] px-2 text-[9px] outline-none"
        style={{
          color: C.dim,
          background: C.surface2,
          border: `1px solid ${C.border}`,
          fontVariantNumeric: "tabular-nums",
        }}
      >
        <option value="custom" style={{ background: C.panel }}>
          Custom
        </option>
        {PRESETS.map((preset) => (
          <option key={preset.id} value={preset.id} style={{ background: C.panel }}>
            {preset.label}
          </option>
        ))}
      </select>

      <div className="ml-auto flex items-center gap-1">
        <SegmentButton active={viewMode === "macro"} onClick={() => onViewMode("macro")}>
          Macro
        </SegmentButton>
        <SegmentButton active={viewMode === "expert"} onClick={() => onViewMode("expert")}>
          Expert
        </SegmentButton>
        <TopButton active={!model.freeze} onClick={onLive} title={model.freeze ? "Resume live reverb" : "Freeze reverb tail"}>
          Live
        </TopButton>
        <TopButton onClick={onReset}>Reset</TopButton>
        <TopButton danger={!enabled} active={!enabled} onClick={onToggleEnabled}>
          Bypass
        </TopButton>
      </div>
    </div>
  );
}

function SpaceCanvas({
  active,
  mode,
  size,
  decay,
  diffusion,
  mix,
}: {
  active: boolean;
  mode: UltraVerbMode;
  size: number;
  decay: number;
  diffusion: number;
  mix: number;
}) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const stateRef = useRef({ active, mode, size, decay, diffusion, mix });

  useEffect(() => {
    stateRef.current = { active, mode, size, decay, diffusion, mix };
  }, [active, decay, diffusion, mix, mode, size]);

  useEffect(() => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx) return;

    let raf = 0;
    let frame = 0;

    const draw = () => {
      const { active: isActive, mode: currentMode, size: roomSize, decay: decaySec, diffusion: diff, mix: wetMix } =
        stateRef.current;
      const rect = canvas.getBoundingClientRect();
      const dpr = Math.max(1, Math.min(2, window.devicePixelRatio || 1));
      const width = Math.max(1, Math.floor(rect.width * dpr));
      const height = Math.max(1, Math.floor(rect.height * dpr));
      if (canvas.width !== width || canvas.height !== height) {
        canvas.width = width;
        canvas.height = height;
      }

      const w = width / dpr;
      const h = height / dpr;
      const t = frame * 0.016;
      const sizePct = roomSize / 100;
      const decayPct = clamp(decaySec / 12, 0, 1);
      const diffusionPct = diff / 100;
      const mixPct = wetMix / 100;
      const energy = isActive ? 0.22 + mixPct * 0.56 + decayPct * 0.22 : 0.14;

      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, w, h);

      ctx.fillStyle = "#080c14";
      ctx.fillRect(0, 0, w, h);

      const cx = w * 0.51;
      const cy = h * 0.54;
      const roomWide = currentMode === "plate" ? 1.16 : currentMode === "space" ? 1.22 : 1;
      const roomTall = currentMode === "hall" ? 1.18 : currentMode === "plate" ? 0.72 : 1;
      const farW = w * (0.24 + sizePct * 0.12) * roomWide;
      const nearW = w * (0.68 + sizePct * 0.18) * roomWide;
      const farH = h * (0.22 + sizePct * 0.06) * roomTall;
      const nearH = h * (0.62 + decayPct * 0.14) * roomTall;
      const farY = cy - h * (0.12 + decayPct * 0.06);
      const nearY = cy + h * 0.07;

      ctx.save();
      ctx.fillStyle = `rgba(114,215,215,${0.035 + energy * 0.04})`;
      ctx.beginPath();
      ctx.ellipse(cx, cy, w * (0.22 + sizePct * 0.28), h * (0.12 + decayPct * 0.16), 0, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillStyle = `rgba(167,139,250,${0.025 + mixPct * 0.045})`;
      ctx.beginPath();
      ctx.ellipse(cx, cy + h * 0.03, w * (0.14 + sizePct * 0.24), h * (0.08 + decayPct * 0.12), 0, 0, Math.PI * 2);
      ctx.fill();
      ctx.restore();

      const far = rectPoints(cx, farY, farW, farH);
      const near = rectPoints(cx, nearY, nearW, nearH);

      drawRoomFace(ctx, near, far, energy);
      drawReflectionField(ctx, w, h, t, cx, cy, sizePct, decayPct, diffusionPct, mixPct, isActive);
      drawPropagationRings(ctx, cx, cy, w, h, t, sizePct, decayPct, diffusionPct, mixPct, isActive);
      drawRoomEdges(ctx, near, far, energy, currentMode);

      frame += 1;
      raf = requestAnimationFrame(draw);
    };

    draw();
    return () => cancelAnimationFrame(raf);
  }, []);

  return <canvas ref={canvasRef} className="absolute inset-0 h-full w-full" />;
}

function VisualReadout({ model, active }: { model: UltraVerbParams; active: boolean }) {
  return (
    <div className="pointer-events-none absolute inset-x-2 bottom-2 flex items-end justify-between">
      <div className="p-2">
        <div className="text-[8px] font-semibold uppercase tracking-[0.18em]" style={{ color: C.faint }}>
          Space Field
        </div>
        <div className="mt-1 flex items-center gap-1.5">
          <SmallReadout label="Size" value={`${model.size.toFixed(0)}%`} />
          <SmallReadout label="Decay" value={`${model.decay.toFixed(1)}s`} />
          <SmallReadout label="Mix" value={`${model.mix.toFixed(0)}%`} />
        </div>
      </div>
      <span
        className="rounded-[4px] px-1.5 py-[3px] text-[8px] font-semibold uppercase tracking-[0.12em]"
        style={{
          color: active ? C.cyan : C.faint,
          background: active ? C.cyanSoft : "rgba(255,255,255,0.025)",
          border: `1px solid ${active ? C.cyanBorder : C.border}`,
        }}
      >
        {active ? "Live" : model.freeze ? "Frozen" : "Bypass"}
      </span>
    </div>
  );
}

function PrimaryKnob({
  label,
  value,
  min,
  max,
  display,
  active,
  accent = C.cyan,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  display: string;
  active: boolean;
  accent?: string;
  onChange: (value: number) => void;
}) {
  return (
    <KnobControl
      label={label}
      value={value}
      min={min}
      max={max}
      display={display}
      size={58}
      accent={accent}
      active={active}
      onChange={onChange}
    />
  );
}

function KnobControl({
  label,
  value,
  min,
  max,
  display,
  size,
  accent,
  active,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  display: string;
  size: number;
  accent: string;
  active: boolean;
  onChange: (value: number) => void;
}) {
  const dragRef = useRef<{ y: number; value: number } | null>(null);
  const pct = clamp((value - min) / (max - min), 0, 1);
  const start = 222;
  const sweep = 276;
  const angle = start + pct * sweep;
  const cx = size / 2;
  const cy = size / 2;
  const radius = size / 2 - 7;
  const trackPath = describeArc(cx, cy, radius, start, start + sweep);
  const valuePath = describeArc(cx, cy, radius, start, angle);

  return (
    <div
      className="flex cursor-ns-resize select-none flex-col items-center justify-center gap-[3px] rounded-[6px]"
      style={{
        background: "rgba(255,255,255,0.024)",
        border: `1px solid ${C.border}`,
      }}
      title={`${label}: ${display}`}
      onPointerDown={(event) => {
        dragRef.current = { y: event.clientY, value };
        event.currentTarget.setPointerCapture(event.pointerId);
      }}
      onPointerMove={(event) => {
        const drag = dragRef.current;
        if (!drag) return;
        const next = drag.value + ((drag.y - event.clientY) / 128) * (max - min);
        onChange(roundForRange(clamp(next, min, max), min, max));
      }}
      onPointerUp={(event) => {
        dragRef.current = null;
        event.currentTarget.releasePointerCapture(event.pointerId);
      }}
      onPointerCancel={(event) => {
        dragRef.current = null;
        event.currentTarget.releasePointerCapture(event.pointerId);
      }}
    >
      <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`} className="block">
        <path d={trackPath} fill="none" stroke="rgba(255,255,255,0.08)" strokeWidth="4" strokeLinecap="round" />
        <path
          d={valuePath}
          fill="none"
          stroke={active ? accent : C.faint}
          strokeWidth="4"
          strokeLinecap="round"
          style={{ filter: active ? `drop-shadow(0 0 4px ${accent}80)` : "none" }}
        />
        <circle cx={cx} cy={cy} r={size * 0.22} fill="#0c111a" stroke="rgba(255,255,255,0.10)" />
        <line
          x1={cx}
          y1={cy}
          x2={cx + size * 0.15 * Math.cos(toRad(angle - 90))}
          y2={cy + size * 0.15 * Math.sin(toRad(angle - 90))}
          stroke={active ? accent : C.faint}
          strokeWidth="2"
          strokeLinecap="round"
        />
      </svg>
      <div className="text-center leading-none">
        <div className="text-[11px] font-semibold tabular-nums" style={{ color: C.text }}>
          {display}
        </div>
        <div className="mt-[3px] text-[8px] font-semibold uppercase tracking-[0.09em]" style={{ color: C.faint }}>
          {label}
        </div>
      </div>
    </div>
  );
}

function SectionDock({
  activeSection,
  onSelect,
  model,
}: {
  activeSection: SectionId;
  onSelect: (section: SectionId) => void;
  model: UltraVerbParams;
}) {
  const summary: Record<SectionId, string> = {
    tone: `${formatFreq(model.lowCutHz)}-${formatFreq(model.highCutHz)}`,
    space: `${model.diffusion.toFixed(0)} / ${model.width.toFixed(0)}`,
    motion: `${model.modulation.toFixed(0)}% ${model.modRateHz.toFixed(2)}Hz`,
    output: `${model.mix.toFixed(0)}% ${fmtDb(model.outputDb)}`,
  };

  return (
    <div className="grid h-[43px] shrink-0 grid-cols-4 gap-1 border-t p-1.5" style={{ borderColor: C.border }}>
      {(["tone", "space", "motion", "output"] as SectionId[]).map((section) => (
        <button
          key={section}
          type="button"
          onClick={() => onSelect(section)}
          className="min-w-0 rounded-[5px] px-1 text-left transition-colors"
          style={{
            background: activeSection === section ? C.cyanSoft : "#0c111a",
            border: `1px solid ${activeSection === section ? C.cyanBorder : C.border}`,
          }}
        >
          <div className="truncate text-[8px] font-semibold uppercase tracking-[0.08em]" style={{ color: activeSection === section ? C.cyan : C.faint }}>
            {SECTION_LABEL[section]}
          </div>
          <div className="mt-[2px] truncate text-[8px] tabular-nums" style={{ color: C.dim }}>
            {summary[section]}
          </div>
        </button>
      ))}
    </div>
  );
}

function ExpertStrip({
  model,
  activeSection,
  onSection,
  onUpdate,
}: {
  model: UltraVerbParams;
  activeSection: SectionId;
  onSection: (section: SectionId) => void;
  onUpdate: (patch: Partial<UltraVerbParams>) => void;
}) {
  return (
    <div
      className="grid h-[90px] shrink-0 grid-cols-[145px_112px_112px_174px] border-t"
      style={{ background: "#0c1119", borderColor: C.border }}
    >
      <ExpertSection
        id="tone"
        active={activeSection === "tone"}
        onSelect={onSection}
        title="Tone"
      >
        <SliderParam label="Low Cut" value={model.lowCutHz} min={20} max={1000} curve="log" display={formatFreq(model.lowCutHz)} onChange={(lowCutHz) => onUpdate({ lowCutHz })} />
        <SliderParam label="High Cut" value={model.highCutHz} min={1000} max={20000} curve="log" display={formatFreq(model.highCutHz)} onChange={(highCutHz) => onUpdate({ highCutHz })} />
        <SliderParam label="Damping" value={model.damping} min={0} max={100} display={`${model.damping.toFixed(0)}%`} onChange={(damping) => onUpdate({ damping })} />
      </ExpertSection>

      <ExpertSection
        id="space"
        active={activeSection === "space"}
        onSelect={onSection}
        title="Space"
      >
        <SliderParam label="Diffusion" value={model.diffusion} min={0} max={100} display={`${model.diffusion.toFixed(0)}%`} onChange={(diffusion) => onUpdate({ diffusion })} />
        <SliderParam label="Width" value={model.width} min={0} max={150} display={`${model.width.toFixed(0)}%`} onChange={(width) => onUpdate({ width })} />
      </ExpertSection>

      <ExpertSection
        id="motion"
        active={activeSection === "motion"}
        onSelect={onSection}
        title="Motion"
      >
        <SliderParam label="Mod Depth" value={model.modulation} min={0} max={100} display={`${model.modulation.toFixed(0)}%`} onChange={(modulation) => onUpdate({ modulation })} />
        <SliderParam label="Mod Rate" value={model.modRateHz} min={0.05} max={5} display={`${model.modRateHz.toFixed(2)}Hz`} onChange={(modRateHz) => onUpdate({ modRateHz })} />
      </ExpertSection>

      <ExpertSection
        id="output"
        active={activeSection === "output"}
        onSelect={onSection}
        title="Output"
      >
        <div className="grid grid-cols-2 gap-x-2">
          <SliderParam label="Dry" value={100 - model.mix} min={0} max={100} display={`${(100 - model.mix).toFixed(0)}%`} onChange={(dry) => onUpdate({ mix: clamp(100 - dry, 0, 100) })} />
          <SliderParam label="ER" value={model.earlyLevel} min={-60} max={6} display={fmtDb(model.earlyLevel)} onChange={(earlyLevel) => onUpdate({ earlyLevel })} />
          <SliderParam label="Wet" value={model.lateLevel} min={-60} max={6} display={fmtDb(model.lateLevel)} onChange={(lateLevel) => onUpdate({ lateLevel })} />
          <SliderParam label="Gain" value={model.outputDb} min={-24} max={12} display={fmtDb(model.outputDb)} onChange={(outputDb) => onUpdate({ outputDb })} />
        </div>
      </ExpertSection>
    </div>
  );
}

function ExpertSection({
  id,
  title,
  active,
  onSelect,
  children,
}: {
  id: SectionId;
  title: string;
  active: boolean;
  onSelect: (section: SectionId) => void;
  children: ReactNode;
}) {
  return (
    <section
      className="min-w-0 border-r px-2 py-1.5"
      style={{
        borderColor: C.border,
        background: active ? "rgba(114,215,215,0.035)" : "transparent",
      }}
    >
      <button
        type="button"
        onClick={() => onSelect(id)}
        className="mb-1 flex h-[14px] w-full items-center justify-between"
      >
        <span className="text-[8px] font-semibold uppercase tracking-[0.14em]" style={{ color: active ? C.cyan : C.faint }}>
          {title}
        </span>
        <span className="h-[4px] w-[4px] rounded-full" style={{ background: active ? C.cyan : C.mute }} />
      </button>
      <div className="flex flex-col gap-[4px]">{children}</div>
    </section>
  );
}

function SliderParam({
  label,
  value,
  min,
  max,
  display,
  onChange,
  curve = "linear",
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  display: string;
  onChange: (value: number) => void;
  curve?: SliderCurve;
}) {
  const ref = useRef<HTMLDivElement | null>(null);
  const pct = valueToPct(value, min, max, curve);

  const setFromEvent = (clientX: number) => {
    const rect = ref.current?.getBoundingClientRect();
    if (!rect) return;
    const nextPct = clamp((clientX - rect.left) / Math.max(1, rect.width), 0, 1);
    const next = pctToValue(nextPct, min, max, curve);
    onChange(roundForRange(next, min, max));
  };

  return (
    <div className="min-w-0">
      <div className="mb-[2px] flex items-center justify-between gap-1">
        <span className="truncate text-[8px] uppercase tracking-[0.05em]" style={{ color: C.faint }}>
          {label}
        </span>
        <span className="shrink-0 text-[8px] tabular-nums" style={{ color: C.dim }}>
          {display}
        </span>
      </div>
      <div
        ref={ref}
        className="h-[5px] cursor-ew-resize rounded-full"
        style={{ background: "#070a10", border: `1px solid ${C.border}` }}
        onPointerDown={(event) => {
          setFromEvent(event.clientX);
          event.currentTarget.setPointerCapture(event.pointerId);
        }}
        onPointerMove={(event) => {
          if (event.buttons !== 1) return;
          setFromEvent(event.clientX);
        }}
        onPointerUp={(event) => event.currentTarget.releasePointerCapture(event.pointerId)}
        onPointerCancel={(event) => event.currentTarget.releasePointerCapture(event.pointerId)}
      >
        <div
          className="h-full rounded-full"
          style={{
            width: `${pct * 100}%`,
            background: C.cyan,
            boxShadow: `0 0 7px ${C.cyan}33`,
          }}
        />
      </div>
    </div>
  );
}

function OutputMeters({ model, active }: { model: UltraVerbParams; active: boolean }) {
  const wet = active ? model.mix / 100 : 0;
  const energy = clamp(wet * 0.72 + model.decay / 30 + dbToLinear(model.outputDb) * 0.08, 0, 1);
  const left = active ? clamp(0.18 + energy * 0.72, 0, 0.96) : 0.05;
  const right = active ? clamp(left * (0.9 + model.width / 750), 0, 0.98) : 0.04;

  return (
    <aside
      className="flex w-[56px] shrink-0 flex-col items-center border-l px-2 py-2"
      style={{ background: "#090d14", borderColor: C.border }}
    >
      <div className="text-[8px] font-semibold uppercase tracking-[0.16em]" style={{ color: C.faint }}>
        Out
      </div>
      <div className="mt-2 flex min-h-0 flex-1 items-stretch gap-1.5">
        <MeterBar label="L" level={left} />
        <MeterBar label="R" level={right} />
      </div>
      <div className="mt-1 text-center text-[8px] tabular-nums" style={{ color: C.dim }}>
        {fmtDb(model.outputDb)}
      </div>
    </aside>
  );
}

function MeterBar({ level, label }: { level: number; label: string }) {
  const pct = clamp(level, 0, 1);
  return (
    <div className="flex flex-col items-center gap-1">
      <div
        className="relative w-[9px] flex-1 overflow-hidden rounded-[3px]"
        style={{
          background: "#05070b",
          border: `1px solid ${C.border}`,
        }}
      >
        <div
          className="absolute bottom-0 left-0 right-0 rounded-[2px] transition-[height] duration-150"
          style={{
            height: `${pct * 100}%`,
            background: pct > 0.88 ? C.danger : pct > 0.72 ? "#e2b866" : C.cyan,
            boxShadow: pct > 0.12 ? `0 0 7px ${C.cyan}55` : "none",
          }}
        />
        <div className="absolute left-0 right-0 top-[22%] h-px bg-black/60" />
        <div className="absolute left-0 right-0 top-[48%] h-px bg-black/50" />
        <div className="absolute left-0 right-0 top-[74%] h-px bg-black/40" />
      </div>
      <span className="text-[7px] font-semibold" style={{ color: C.faint }}>
        {label}
      </span>
    </div>
  );
}

function TopButton({
  active,
  danger,
  title,
  onClick,
  children,
}: {
  active?: boolean;
  danger?: boolean;
  title?: string;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      title={title}
      onClick={onClick}
      className="h-[23px] rounded-[5px] px-2 text-[9px] font-semibold transition-colors"
      style={{
        color: danger ? C.danger : active ? C.cyan : C.dim,
        background: danger && active ? "rgba(239,107,107,0.11)" : active ? C.cyanSoft : "#0c111a",
        border: `1px solid ${danger && active ? "rgba(239,107,107,0.34)" : active ? C.cyanBorder : C.border}`,
      }}
    >
      {children}
    </button>
  );
}

function SegmentButton({ active, onClick, children }: { active: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="h-[23px] rounded-[5px] px-1.5 text-[8.5px] font-semibold transition-colors"
      style={{
        color: active ? C.cyan : C.faint,
        background: active ? C.cyanSoft : "#0c111a",
        border: `1px solid ${active ? C.cyanBorder : C.border}`,
      }}
    >
      {children}
    </button>
  );
}

function SmallReadout({ label, value }: { label: string; value: string }) {
  return (
    <div
      className="rounded-[4px] px-1.5 py-1"
      style={{
        background: "rgba(7,10,16,0.62)",
        border: `1px solid ${C.border}`,
        backdropFilter: "blur(8px)",
      }}
    >
      <div className="text-[7px] uppercase tracking-[0.09em]" style={{ color: C.faint }}>
        {label}
      </div>
      <div className="text-[9px] font-semibold tabular-nums" style={{ color: C.text }}>
        {value}
      </div>
    </div>
  );
}

function drawRoomFace(
  ctx: CanvasRenderingContext2D,
  near: RoomRect,
  far: RoomRect,
  energy: number,
) {
  ctx.save();
  ctx.beginPath();
  ctx.moveTo(near.tl.x, near.tl.y);
  ctx.lineTo(near.tr.x, near.tr.y);
  ctx.lineTo(far.tr.x, far.tr.y);
  ctx.lineTo(far.tl.x, far.tl.y);
  ctx.closePath();
  ctx.fillStyle = `rgba(167,139,250,${0.035 + energy * 0.055})`;
  ctx.fill();

  ctx.beginPath();
  ctx.moveTo(near.bl.x, near.bl.y);
  ctx.lineTo(near.br.x, near.br.y);
  ctx.lineTo(far.br.x, far.br.y);
  ctx.lineTo(far.bl.x, far.bl.y);
  ctx.closePath();
  ctx.fillStyle = `rgba(114,215,215,${0.03 + energy * 0.065})`;
  ctx.fill();

  ctx.restore();
}

function drawRoomEdges(
  ctx: CanvasRenderingContext2D,
  near: RoomRect,
  far: RoomRect,
  energy: number,
  mode: UltraVerbMode,
) {
  const edgeAlpha = 0.24 + energy * 0.42;
  ctx.save();
  ctx.lineWidth = mode === "plate" ? 1.2 : 1.05;
  ctx.strokeStyle = `rgba(114,215,215,${edgeAlpha})`;
  ctx.shadowColor = "rgba(114,215,215,0.36)";
  ctx.shadowBlur = 8;

  drawRectPath(ctx, near);
  drawRectPath(ctx, far);

  ctx.beginPath();
  ctx.moveTo(near.tl.x, near.tl.y);
  ctx.lineTo(far.tl.x, far.tl.y);
  ctx.moveTo(near.tr.x, near.tr.y);
  ctx.lineTo(far.tr.x, far.tr.y);
  ctx.moveTo(near.bl.x, near.bl.y);
  ctx.lineTo(far.bl.x, far.bl.y);
  ctx.moveTo(near.br.x, near.br.y);
  ctx.lineTo(far.br.x, far.br.y);
  ctx.stroke();

  ctx.shadowBlur = 0;
  ctx.strokeStyle = `rgba(167,139,250,${0.10 + energy * 0.16})`;
  ctx.lineWidth = 0.75;
  for (let i = 1; i <= 3; i++) {
    const t = i / 4;
    const r = interpolateRect(near, far, t);
    drawRectPath(ctx, r);
  }
  ctx.restore();
}

function drawReflectionField(
  ctx: CanvasRenderingContext2D,
  w: number,
  h: number,
  time: number,
  cx: number,
  cy: number,
  sizePct: number,
  decayPct: number,
  diffusionPct: number,
  mixPct: number,
  active: boolean,
) {
  const count = Math.round(20 + diffusionPct * 36);
  const reachX = w * (0.22 + sizePct * 0.42);
  const reachY = h * (0.18 + decayPct * 0.36);
  ctx.save();
  for (let i = 0; i < count; i++) {
    const p = FIELD_POINTS[i % FIELD_POINTS.length]!;
    const phase = (time * (0.05 + diffusionPct * 0.15) + p.z) % 1;
    const angle = p.x * Math.PI * 2 + time * (0.08 + p.z * 0.06);
    const radius = (0.16 + phase * 0.84) * (0.35 + p.s * 0.45);
    const x = cx + Math.cos(angle) * reachX * radius;
    const y = cy + Math.sin(angle * 1.23) * reachY * radius;
    const dot = 0.75 + p.s * 1.2;
    const alpha = active ? (0.035 + mixPct * 0.12) * (1 - phase * 0.55) : 0.025;
    ctx.fillStyle = i % 3 === 0 ? `rgba(167,139,250,${alpha})` : `rgba(114,215,215,${alpha})`;
    ctx.beginPath();
    ctx.arc(x, y, dot, 0, Math.PI * 2);
    ctx.fill();

    if (i % 5 === 0) {
      ctx.strokeStyle = `rgba(114,215,215,${alpha * 0.45})`;
      ctx.lineWidth = 0.65;
      ctx.beginPath();
      ctx.moveTo(cx + (x - cx) * 0.72, cy + (y - cy) * 0.72);
      ctx.lineTo(x, y);
      ctx.stroke();
    }
  }
  ctx.restore();
}

function drawPropagationRings(
  ctx: CanvasRenderingContext2D,
  cx: number,
  cy: number,
  w: number,
  h: number,
  time: number,
  sizePct: number,
  decayPct: number,
  diffusionPct: number,
  mixPct: number,
  active: boolean,
) {
  ctx.save();
  ctx.lineWidth = 1;
  for (let i = 0; i < 4; i++) {
    const phase = (time * (0.18 + diffusionPct * 0.18) + i * 0.25) % 1;
    const rx = w * (0.08 + sizePct * 0.17 + phase * (0.25 + sizePct * 0.16));
    const ry = h * (0.04 + decayPct * 0.12 + phase * (0.12 + decayPct * 0.12));
    const alpha = active ? (0.2 + mixPct * 0.25) * (1 - phase) : 0.07 * (1 - phase);
    ctx.strokeStyle = `rgba(114,215,215,${alpha})`;
    ctx.beginPath();
    ctx.ellipse(cx, cy, rx, ry, 0, 0, Math.PI * 2);
    ctx.stroke();
  }
  ctx.restore();
}

type Point = { x: number; y: number };
type RoomRect = { tl: Point; tr: Point; br: Point; bl: Point };

function rectPoints(cx: number, cy: number, width: number, height: number): RoomRect {
  return {
    tl: { x: cx - width / 2, y: cy - height / 2 },
    tr: { x: cx + width / 2, y: cy - height / 2 },
    br: { x: cx + width / 2, y: cy + height / 2 },
    bl: { x: cx - width / 2, y: cy + height / 2 },
  };
}

function interpolateRect(a: RoomRect, b: RoomRect, t: number): RoomRect {
  return {
    tl: lerpPoint(a.tl, b.tl, t),
    tr: lerpPoint(a.tr, b.tr, t),
    br: lerpPoint(a.br, b.br, t),
    bl: lerpPoint(a.bl, b.bl, t),
  };
}

function lerpPoint(a: Point, b: Point, t: number): Point {
  return { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t };
}

function drawRectPath(ctx: CanvasRenderingContext2D, rect: RoomRect) {
  ctx.beginPath();
  ctx.moveTo(rect.tl.x, rect.tl.y);
  ctx.lineTo(rect.tr.x, rect.tr.y);
  ctx.lineTo(rect.br.x, rect.br.y);
  ctx.lineTo(rect.bl.x, rect.bl.y);
  ctx.closePath();
  ctx.stroke();
}

function valueToPct(value: number, min: number, max: number, curve: SliderCurve): number {
  if (curve === "log") {
    const lo = Math.log(min);
    const hi = Math.log(max);
    return clamp((Math.log(clamp(value, min, max)) - lo) / (hi - lo), 0, 1);
  }
  return clamp((value - min) / (max - min), 0, 1);
}

function pctToValue(pct: number, min: number, max: number, curve: SliderCurve): number {
  if (curve === "log") {
    const lo = Math.log(min);
    const hi = Math.log(max);
    return Math.exp(lo + clamp(pct, 0, 1) * (hi - lo));
  }
  return min + clamp(pct, 0, 1) * (max - min);
}

function describeArc(cx: number, cy: number, r: number, startDeg: number, endDeg: number): string {
  const start = polar(cx, cy, r, endDeg);
  const end = polar(cx, cy, r, startDeg);
  const largeArc = endDeg - startDeg <= 180 ? 0 : 1;
  return `M ${start.x} ${start.y} A ${r} ${r} 0 ${largeArc} 0 ${end.x} ${end.y}`;
}

function polar(cx: number, cy: number, r: number, deg: number): Point {
  const rad = ((deg - 90) * Math.PI) / 180;
  return { x: cx + r * Math.cos(rad), y: cy + r * Math.sin(rad) };
}

function roundForRange(value: number, min: number, max: number): number {
  const range = max - min;
  if (range <= 6) return Math.round(value * 100) / 100;
  if (range <= 50) return Math.round(value * 10) / 10;
  return Math.round(value);
}

function toRad(deg: number): number {
  return (deg * Math.PI) / 180;
}

function fract(value: number): number {
  return value - Math.floor(value);
}

function dbToLinear(db: number): number {
  return Math.pow(10, db / 20);
}

function formatFreq(hz: number): string {
  if (hz >= 10000) return `${(hz / 1000).toFixed(0)}k`;
  if (hz >= 1000) return `${(hz / 1000).toFixed(1).replace(/\.0$/, "")}k`;
  return `${Math.round(hz)}`;
}

function fmtDb(db: number): string {
  if (db <= -59.5) return "-inf";
  return `${db >= 0 ? "+" : ""}${db.toFixed(1)}`;
}
