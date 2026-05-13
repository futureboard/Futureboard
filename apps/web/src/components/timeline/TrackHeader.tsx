import { Cpu, GitMerge, Mic, Mic2, Music, Star, Volume2, VolumeX } from "lucide-react";
import type { TrackType } from "../../types/daw";

const TYPE_ICONS: Record<TrackType, React.ElementType> = {
  audio: Mic2,
  midi: Music,
  plugin: Cpu,
  bus: GitMerge,
};

import type { DawTrack } from "../../types/daw";
import { useProjectStore } from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
import { mixer } from "../../engine/Mixer";
import { HEADER_WIDTH, TRACK_HEIGHT } from "../../theme";

function volumeToDb(v: number) {
  if (v <= 0.001) return "-∞";
  const db = 20 * Math.log10(v);
  return (db >= 0 ? `+${db.toFixed(1)}` : db.toFixed(1)) + " dB";
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

export function TrackHeader({ track, index }: { track: DawTrack; index: number }) {
  const { setTrackVolume, setTrackMute, setTrackSolo, setTrackArmed } = useProjectStore();
  const { selectedTrackId, setSelectedTrackId } = useUIStore();
  const selected = selectedTrackId === track.id;
  const headerBg = selected ? "#252c35" : "#1c2028";
  const TypeIcon = TYPE_ICONS[track.type] ?? Mic2;

  const trackValue = `${(track.volume * 100).toFixed(1)}%`;

  return (
    <div
      onClick={() => setSelectedTrackId(track.id)}
      className="sticky left-0 z-50 flex shrink-0 cursor-default border-r border-b border-daw-border transition-colors shadow-[6px_0_16px_rgba(0,0,0,0.32)]"
      style={{
        width: HEADER_WIDTH,
        minWidth: HEADER_WIDTH,
        height: TRACK_HEIGHT,
        background: headerBg,
      }}
    >
      {/* bleed shadow to the right */}
      <div
        className="pointer-events-none absolute bottom-0 right-[-12px] top-0 z-0 w-3"
        style={{ background: `linear-gradient(to right, ${headerBg}, transparent)` }}
      />

      {/* track colour bar */}
      <div className="w-[3px] shrink-0" style={{ background: track.color }} />

      <div className="relative z-10 flex min-w-0 flex-1 flex-col justify-between overflow-hidden px-2 py-[7px]">

        {/* ── row 1: icon · name · buttons · number ── */}
        <div className="flex min-w-0 items-center gap-1.5">
          <div
            className="flex h-[22px] w-[22px] shrink-0 items-center justify-center rounded"
            style={{ background: `${track.color}22` }}
          >
            <TypeIcon size={11} style={{ color: track.color, opacity: 0.85 }} />
          </div>

          <span
            className={`flex-1 truncate text-[11px] font-semibold leading-none tracking-[0.01em] ${selected ? "text-daw-text" : "text-daw-dim"}`}
          >
            {track.name}
          </span>

          <div className="flex items-center gap-[3px]">
            <TrackBtn icon={VolumeX} active={track.muted}  activeColor="#f3c969" label="Mute"
              onClick={() => { setTrackMute(track.id, !track.muted); mixer.setMute(track.id, !track.muted); }} />
            <TrackBtn icon={Star}    active={track.solo}   activeColor="#7bd88f" label="Solo"
              onClick={() => { setTrackSolo(track.id, !track.solo); mixer.setSolo(track.id, !track.solo); }} />
            <TrackBtn icon={Mic}     active={track.armed}  activeColor="#f06a61" label="Arm"
              onClick={() => setTrackArmed(track.id, !track.armed)} />
          </div>

          <span
            className="shrink-0 rounded border border-white/[0.07] bg-black/20 px-[5px] py-[2px] text-[9px] tabular-nums text-daw-faint"
          >
            {String(index + 1).padStart(2, "0")}
          </span>
        </div>

        {/* ── row 2: vol icon · styled fader · dB readout ── */}
        <div className="flex items-center gap-1.5">
          <Volume2 size={10} className="shrink-0" style={{ color: track.color, opacity: 0.5 }} />
          <input
            type="range" min={0} max={1} step={0.004} value={track.volume}
            onClick={(e) => e.stopPropagation()}
            onChange={(e) => {
              e.stopPropagation();
              const v = parseFloat(e.target.value);
              setTrackVolume(track.id, v);
              mixer.setVolume(track.id, v);
            }}
            className="daw-track-fader flex-1"
            style={{
              "--track-accent": track.color,
              "--track-value": trackValue,
            } as React.CSSProperties}
          />
          <span className="shrink-0 min-w-[36px] text-right text-[9px] tabular-nums text-daw-faint">
            {volumeToDb(track.volume)}
          </span>
        </div>

      </div>
    </div>
  );
}
