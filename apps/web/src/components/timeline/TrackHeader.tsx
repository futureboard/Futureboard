import {
  CornerDownLeft, Cpu, GitFork, GitMerge, GripVertical,
  Mic, Mic2, Music, Star, Trash2, Volume2, VolumeX,
} from "lucide-react";
import { useRef } from "react";
import type { HTMLAttributes } from "react";
import type { TrackType } from "../../types/daw";
import type { DawTrack } from "../../types/daw";
import { useProjectStore } from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
import { useHistoryStore } from "../../store/historyStore";
import {
  SetTrackMuteCommand, SetTrackSoloCommand,
  SetTrackVolumeCommand, SetTrackPanCommand,
  DeleteTrackCommand,
} from "../../commands";
import { HEADER_WIDTH, TRACK_HEIGHT } from "../../theme";
import { buildTrackContextMenu } from "../../menu/trackContextMenu";
import { CanvasVUMeter } from "./CanvasVUMeter";

const TYPE_ICONS: Record<TrackType, React.ElementType> = {
  audio:      Mic2,
  midi:       Music,
  instrument: Cpu,
  plugin:     Cpu,
  bus:        GitMerge,
  return:     CornerDownLeft,
  group:      GitFork,
  master:     Volume2,
};

function volumeToDb(v: number) {
  if (v <= 0.001) return "-∞";
  const db = 20 * Math.log10(v);
  return (db >= 0 ? `+${db.toFixed(1)}` : db.toFixed(1)) + " dB";
}

function panLabel(pan: number): string {
  if (Math.abs(pan) < 0.01) return "C";
  const pct = Math.round(Math.abs(pan) * 100);
  return pan < 0 ? `L${pct}` : `R${pct}`;
}

function trackTypeLabel(type: TrackType): string {
  switch (type) {
    case "midi":       return "MIDI";
    case "instrument": return "INST";
    case "plugin":     return "PLUG";
    case "bus":        return "BUS";
    case "return":     return "RET";
    case "group":      return "GRP";
    case "master":     return "MAS";
    case "audio":
    default:           return "AUD";
  }
}

function TrackBtn({ icon: Icon, active, activeColor, label, onClick }: {
  icon: React.ElementType; active: boolean; activeColor: string; label: string; onClick: () => void;
}) {
  return (
    <button
      onClick={(e) => { e.stopPropagation(); onClick(); }}
      title={label}
      className="flex h-5 w-5 shrink-0 items-center justify-center rounded transition-colors"
      style={{
        background: active ? activeColor : "rgba(255,255,255,0.05)",
        border: `1px solid ${active ? activeColor : "rgba(255,255,255,0.09)"}`,
        color: active ? "#101216" : "rgba(200,212,224,0.55)",
      }}
    >
      <Icon size={10} />
    </button>
  );
}

// ── Pan Knob ──────────────────────────────────────────────────────────────────
// SVG knob, -1 to +1. Horizontal pointer drag; double-click resets to center.

function PanKnob({
  value,
  color,
  onChange,
}: {
  value: number;
  color: string;
  onChange: (v: number) => void;
}) {
  const dragRef = useRef<{ startX: number; startVal: number } | null>(null);

  const onPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    e.stopPropagation();
    e.preventDefault();
    dragRef.current = { startX: e.clientX, startVal: value };
    e.currentTarget.setPointerCapture(e.pointerId);
  };

  const onPointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!dragRef.current) return;
    const dx = e.clientX - dragRef.current.startX;
    // 80px → full L–R sweep; Shift for fine control
    const sensitivity = e.shiftKey ? 320 : 80;
    const next = Math.max(-1, Math.min(1, dragRef.current.startVal + dx / sensitivity));
    if (next !== value) onChange(next);
  };

  const onPointerUp = (e: React.PointerEvent<HTMLDivElement>) => {
    e.currentTarget.releasePointerCapture(e.pointerId);
    dragRef.current = null;
  };

  const onDoubleClick = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (value !== 0) onChange(0);
  };

  // Map -1..1 → angle in degrees (-135° at L, 0° = top, +135° at R)
  // SVG angle: 0° = 3 o'clock, so top = -90°
  const angleDeg = value * 135 - 90;
  const rad = (angleDeg * Math.PI) / 180;
  const cx = 11, cy = 11, r = 7.5;
  const dotX = cx + r * Math.cos(rad);
  const dotY = cy + r * Math.sin(rad);

  // Arc endpoints for the track (from -135° to +135° relative to top)
  const arcStart = ((-135 - 90) * Math.PI) / 180;
  const arcEnd   = ((+135 - 90) * Math.PI) / 180;
  const trackR = 9;
  const arcX1 = cx + trackR * Math.cos(arcStart);
  const arcY1 = cy + trackR * Math.sin(arcStart);
  const arcX2 = cx + trackR * Math.cos(arcEnd);
  const arcY2 = cy + trackR * Math.sin(arcEnd);

  return (
    <div
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onDoubleClick={onDoubleClick}
      className="shrink-0 cursor-ew-resize select-none"
      style={{ width: 22, height: 22, touchAction: "none" }}
      title={`Pan: ${panLabel(value)} — double-click to reset`}
    >
      <svg width={22} height={22}>
        {/* Body */}
        <circle cx={cx} cy={cy} r={10} fill="rgba(0,0,0,0.45)" stroke="rgba(255,255,255,0.1)" strokeWidth={0.8} />

        {/* Track arc */}
        <path
          d={`M ${arcX1} ${arcY1} A ${trackR} ${trackR} 0 1 1 ${arcX2} ${arcY2}`}
          fill="none"
          stroke="rgba(255,255,255,0.12)"
          strokeWidth={1.5}
          strokeLinecap="round"
        />

        {/* Center tick (12 o'clock) */}
        <line x1={cx} y1={cy - trackR + 1} x2={cx} y2={cy - trackR + 3.5}
          stroke="rgba(255,255,255,0.3)" strokeWidth={1} />

        {/* Value pointer */}
        <line
          x1={cx} y1={cy} x2={dotX} y2={dotY}
          stroke={color} strokeWidth={1.5} strokeLinecap="round" opacity={0.9}
        />

        {/* Hub dot */}
        <circle cx={cx} cy={cy} r={1.8} fill={color} opacity={0.7} />
      </svg>
    </div>
  );
}

// ── Drag handle props type (from dnd-kit) ────────────────────────────────────

type DragHandleProps = HTMLAttributes<HTMLDivElement> & {
  role?: string;
  tabIndex?: number;
  "aria-describedby"?: string;
  "aria-pressed"?: boolean | "false" | "true" | "mixed";
  "aria-roledescription"?: string;
  "aria-disabled"?: boolean;
};

// ── TrackHeader ───────────────────────────────────────────────────────────────

export function TrackHeader({
  track,
  index,
  dragHandleProps,
  isDragging,
}: {
  track: DawTrack;
  index: number;
  dragHandleProps?: DragHandleProps;
  isDragging?: boolean;
}) {
  const { setTrackArmed } = useProjectStore();
  const {
    selectedTrackId, setSelectedTrackId,
    selectedTrackIds, setSelectedTrackIds, toggleTrackInSelection,
    setSelectedMixerTrackId, setSelectedClipIds, setFocusedPanel,
  } = useUIStore();
  const isPrimary  = selectedTrackId === track.id;
  const isInGroup  = selectedTrackIds.includes(track.id);
  const selected   = isPrimary || isInGroup;
  const headerBg   = isPrimary ? "#252c35" : isInGroup ? "#1f2730" : "#1c2028";
  const TypeIcon = TYPE_ICONS[track.type] ?? Mic2;

  const trackValue = `${(track.volume * 100).toFixed(1)}%`;

  return (
    <div
      onClick={(e) => {
        if (e.shiftKey && selectedTrackId) {
          toggleTrackInSelection(track.id);
        } else {
          setSelectedTrackId(track.id);
          setSelectedTrackIds([]);
          setSelectedMixerTrackId(track.id);
          setSelectedClipIds([]);
        }
        setFocusedPanel("timeline");
      }}
      onContextMenu={(e) => {
        e.preventDefault();
        setSelectedTrackId(track.id);
        setFocusedPanel("timeline");
        useUIStore.getState().setContextMenu(true, { x: e.clientX, y: e.clientY }, buildTrackContextMenu(track));
      }}
      className="sticky left-0 z-50 flex shrink-0 cursor-default overflow-hidden border-r border-b border-daw-border transition-colors shadow-[6px_0_16px_rgba(0,0,0,0.32)]"
      style={{ width: HEADER_WIDTH, minWidth: HEADER_WIDTH, height: TRACK_HEIGHT, background: headerBg }}
    >
      {/* Right-edge bleed shadow */}
      <div
        className="pointer-events-none absolute bottom-0 right-[-12px] top-0 z-0 w-3"
        style={{ background: `linear-gradient(to right, ${headerBg}, transparent)` }}
      />

      {/* Color strip */}
      <div className="w-[4px] shrink-0" style={{ background: track.color }} />

      <div className="relative z-10 flex min-w-0 flex-1 flex-col gap-1.5 overflow-hidden px-2 py-2">

        {/* ── Row 1: icon / name / buttons ─────────────────────────────── */}
        <div className="flex min-w-0 items-center gap-1.5">
          {/* Drag handle */}
          <div
            {...(dragHandleProps ?? {})}
            onPointerDown={(e) => {
              e.stopPropagation();
              dragHandleProps?.onPointerDown?.(e);
            }}
            onClick={(e) => e.stopPropagation()}
            title="Drag to reorder"
            className={`flex h-7 w-3 shrink-0 items-center justify-center rounded-sm text-daw-faint transition-colors hover:bg-white/[0.06] hover:text-daw-dim ${
              isDragging ? "cursor-grabbing text-daw-text" : "cursor-grab"
            }`}
            style={{ touchAction: "none" }}
          >
            <GripVertical size={12} />
          </div>

          {/* Track type icon */}
          <div
            className="flex h-6 w-6 shrink-0 items-center justify-center rounded border"
            style={{
              background: `${track.color}18`,
              borderColor: `${track.color}55`,
              color: track.color,
            }}
          >
            <TypeIcon size={12} />
          </div>

          {/* Name + type badge */}
          <div className="min-w-0 flex-1">
            <div className="flex min-w-0 items-center gap-1">
              <span className={`truncate text-[11px] font-semibold leading-4 ${isPrimary ? "text-daw-text" : selected ? "text-daw-dim/90" : "text-daw-dim"}`}>
                {track.name}
              </span>
              <span
                className="shrink-0 rounded px-1 py-[1px] text-[8px] font-bold tracking-wide"
                style={{ background: `${track.color}22`, color: track.color }}
              >
                {trackTypeLabel(track.type)}
              </span>
            </div>
            <div className="mt-0.5 flex items-center gap-1 text-[9px] leading-none text-daw-faint">
              <span className="tabular-nums">CH {String(index + 1).padStart(2, "0")}</span>
              <span className="h-[3px] w-[3px] rounded-full" style={{ background: track.color }} />
              <span>{track.clips.length} clips</span>
            </div>
          </div>

          {/* Mute / Solo / Arm / Delete */}
          <div className="flex shrink-0 items-center gap-[3px] rounded border border-white/[0.06] bg-black/15 p-[2px]">
            <TrackBtn icon={VolumeX} active={track.muted} activeColor="#f3c969" label="Mute"
              onClick={() => useHistoryStore.getState().execute(new SetTrackMuteCommand(track.id, !track.muted))} />
            <TrackBtn icon={Star}    active={track.solo}  activeColor="#7bd88f" label="Solo"
              onClick={() => useHistoryStore.getState().execute(new SetTrackSoloCommand(track.id, !track.solo))} />
            <TrackBtn icon={Mic}     active={track.armed} activeColor="#f06a61" label="Arm"
              onClick={() => setTrackArmed(track.id, !track.armed)} />
            <TrackBtn icon={Trash2}  active={false} activeColor="#f06a61" label="Delete Track"
              onClick={() => {
                useHistoryStore.getState().execute(new DeleteTrackCommand(track.id));
                useUIStore.getState().setSelectedTrackId(null);
                useUIStore.getState().setSelectedMixerTrackId(null);
              }} />
          </div>
        </div>

        {/* ── Row 2: volume · pan · VU · dB ──────────────────────────── */}
        <div className="flex min-w-0 items-center gap-1.5 rounded border border-white/[0.055] bg-black/10 px-1.5 py-1">
          {/* Volume icon */}
          <Volume2 size={10} className="shrink-0" style={{ color: track.color, opacity: 0.7 }} />

          {/* Volume fader */}
          <input
            type="range" min={0} max={1} step={0.004} value={track.volume}
            onClick={(e) => e.stopPropagation()}
            onChange={(e) => {
              e.stopPropagation();
              useHistoryStore.getState().execute(
                new SetTrackVolumeCommand(track.id, parseFloat(e.target.value), track.volume)
              );
            }}
            className="daw-track-fader min-w-0 flex-1"
            style={{ "--track-accent": track.color, "--track-value": trackValue } as React.CSSProperties}
          />

          {/* Pan knob */}
          <PanKnob
            value={track.pan ?? 0}
            color={track.color}
            onChange={(next) => {
              const snapped = Math.abs(next) < 0.03 ? 0 : next;
              useHistoryStore.getState().execute(
                new SetTrackPanCommand(track.id, snapped, track.pan ?? 0)
              );
            }}
          />

          {/* Canvas VU meter (stereo, imperative RAF loop) */}
          <CanvasVUMeter trackId={track.id} width={12} height={16} />

          {/* dB readout */}
          <span className="shrink-0 w-[38px] text-right text-[9px] tabular-nums text-daw-faint">
            {volumeToDb(track.volume)}
          </span>
        </div>
      </div>
    </div>
  );
}
