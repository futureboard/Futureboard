import { Cpu, GitMerge, Mic, Mic2, Music, Star, Trash2, Volume2, VolumeX } from "lucide-react";
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
import { useHistoryStore } from "../../store/historyStore";
import { SetTrackMuteCommand, SetTrackSoloCommand, SetTrackVolumeCommand, DeleteTrackCommand } from "../../commands";
import { HEADER_WIDTH, TRACK_HEIGHT } from "../../theme";

function volumeToDb(v: number) {
  if (v <= 0.001) return "-∞";
  const db = 20 * Math.log10(v);
  return (db >= 0 ? `+${db.toFixed(1)}` : db.toFixed(1)) + " dB";
}

function trackTypeLabel(type: TrackType) {
  if (type === "midi") return "MIDI";
  if (type === "plugin") return "PLUG";
  if (type === "bus") return "BUS";
  return "AUD";
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
  const { setTrackArmed } = useProjectStore();
  const { selectedTrackId, setSelectedTrackId, setSelectedClipIds, setFocusedPanel } = useUIStore();
  const selected = selectedTrackId === track.id;
  const headerBg = selected ? "#252c35" : "#1c2028";
  const TypeIcon = TYPE_ICONS[track.type] ?? Mic2;

  const trackValue = `${(track.volume * 100).toFixed(1)}%`;

  return (
    <div
      onClick={() => {
        setSelectedTrackId(track.id);
        setSelectedClipIds([]);
        setFocusedPanel("timeline");
      }}
      onContextMenu={(e) => {
        e.preventDefault();
        setSelectedTrackId(track.id);
        setFocusedPanel("timeline");
        useUIStore.getState().setContextMenu(true, { x: e.clientX, y: e.clientY }, [
          {
            id: "ctx.delete_track",
            label: "Delete Track",
            danger: true,
            action: "edit:delete-track"
          }
        ]);
      }}
      className="sticky left-0 z-50 flex shrink-0 cursor-default overflow-hidden border-r border-b border-daw-border transition-colors shadow-[6px_0_16px_rgba(0,0,0,0.32)]"
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

      <div className="w-[4px] shrink-0" style={{ background: track.color }} />

      <div className="relative z-10 flex min-w-0 flex-1 flex-col gap-1.5 overflow-hidden px-2.5 py-2">
        <div className="flex min-w-0 items-center gap-2">
          <div className="flex min-w-0 flex-1 items-center gap-2">
            <div
              className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md border"
              style={{
                background: `${track.color}18`,
                borderColor: `${track.color}55`,
                color: track.color,
              }}
            >
              <TypeIcon size={13} />
            </div>

            <div className="min-w-0 flex-1">
              <div className="flex min-w-0 items-center gap-1.5">
                <span className={`truncate text-[11px] font-semibold leading-4 ${selected ? "text-daw-text" : "text-daw-dim"}`}>
                  {track.name}
                </span>
                <span
                  className="shrink-0 rounded px-1 py-[1px] text-[8px] font-bold tracking-wide"
                  style={{ background: `${track.color}22`, color: track.color }}
                >
                  {trackTypeLabel(track.type)}
                </span>
              </div>
              <div className="mt-0.5 flex items-center gap-1.5 text-[9px] leading-none text-daw-faint">
                <span className="tabular-nums">CH {String(index + 1).padStart(2, "0")}</span>
                <span className="h-1 w-1 rounded-full" style={{ background: track.color }} />
                <span>{track.clips.length} clips</span>
              </div>
            </div>
          </div>

          <div className="flex shrink-0 items-center gap-[3px] rounded-md border border-white/[0.06] bg-black/15 p-[2px]">
            <TrackBtn icon={VolumeX} active={track.muted} activeColor="#f3c969" label="Mute"
              onClick={() => useHistoryStore.getState().execute(new SetTrackMuteCommand(track.id, !track.muted))} />
            <TrackBtn icon={Star} active={track.solo} activeColor="#7bd88f" label="Solo"
              onClick={() => useHistoryStore.getState().execute(new SetTrackSoloCommand(track.id, !track.solo))} />
            <TrackBtn icon={Mic} active={track.armed} activeColor="#f06a61" label="Arm"
              onClick={() => setTrackArmed(track.id, !track.armed)} />
            <TrackBtn icon={Trash2} active={false} activeColor="#f06a61" label="Delete Track"
              onClick={() => {
                useHistoryStore.getState().execute(new DeleteTrackCommand(track.id));
                useUIStore.getState().setSelectedTrackId(null);
                useUIStore.getState().setSelectedMixerTrackId(null);
              }} />
          </div>
        </div>

        <div className="flex min-w-0 items-center gap-2 rounded-md border border-white/[0.055] bg-black/10 px-2 py-1.5">
          <Volume2 size={10} className="shrink-0" style={{ color: track.color, opacity: 0.75 }} />
          <input
            type="range" min={0} max={1} step={0.004} value={track.volume}
            onClick={(e) => e.stopPropagation()}
            onChange={(e) => {
              e.stopPropagation();
              const v = parseFloat(e.target.value);
              useHistoryStore.getState().execute(new SetTrackVolumeCommand(track.id, v, track.volume));
            }}
            className="daw-track-fader min-w-0 flex-1"
            style={{
              "--track-accent": track.color,
              "--track-value": trackValue,
            } as React.CSSProperties}
          />
          <div className="flex h-4 w-8 shrink-0 items-end gap-px">
            {Array.from({ length: 8 }, (_, i) => {
              const active = i < Math.round(track.volume * 8);
              return (
                <span
                  key={i}
                  className="w-0.5 rounded-full"
                  style={{
                    height: `${4 + i * 1.35}px`,
                    background: active ? track.color : "rgba(255,255,255,0.08)",
                    opacity: active ? 0.9 : 1,
                  }}
                />
              );
            })}
          </div>
          <span className="shrink-0 min-w-[42px] text-right text-[9px] tabular-nums text-daw-faint">
            {volumeToDb(track.volume)}
          </span>
        </div>
      </div>
    </div>
  );
}
