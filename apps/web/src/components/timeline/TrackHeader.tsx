import { Mic, Star, Volume2, VolumeX } from "lucide-react";
import type { DawTrack } from "../../types/daw";
import { useProjectStore } from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
import { mixer } from "../../engine/Mixer";
import { HEADER_WIDTH, TRACK_HEIGHT } from "../../theme";

function TrackBtn({ icon: Icon, active, activeColor, label, onClick }: {
  icon: React.ElementType; active: boolean; activeColor: string; label: string; onClick: () => void;
}) {
  return (
    <button
      onClick={(e) => { e.stopPropagation(); onClick(); }}
      title={label}
      className="flex h-5 w-5 shrink-0 items-center justify-center rounded border transition-colors"
      style={{ background: active ? activeColor : "#111418", borderColor: active ? activeColor : "#303943", color: active ? "#07090b" : "#8d99a7" }}
    >
      <Icon size={10} />
    </button>
  );
}

export function TrackHeader({ track, index }: { track: DawTrack; index: number }) {
  const { setTrackVolume, setTrackMute, setTrackSolo, setTrackArmed } = useProjectStore();
  const { selectedTrackId, setSelectedTrackId } = useUIStore();
  const selected = selectedTrackId === track.id;

  return (
    <div
      onClick={() => setSelectedTrackId(track.id)}
      className="flex shrink-0 cursor-default overflow-hidden border-r border-b border-daw-border transition-colors"
      style={{ width: HEADER_WIDTH, minWidth: HEADER_WIDTH, height: TRACK_HEIGHT, background: selected ? "#20262d" : "#171b20" }}
    >
      <div className="w-1 shrink-0" style={{ background: track.color }} />

      <div className="flex min-w-0 flex-1 flex-col justify-between gap-2 px-3 py-2">
        <div className="flex min-w-0 items-center gap-2">
          <div className="flex h-6 w-6 shrink-0 items-center justify-center rounded bg-daw-bg text-daw-faint">
            <Mic size={11} />
          </div>
          <span className={`flex-1 truncate text-[12px] font-semibold ${selected ? "text-daw-text" : "text-daw-dim"}`}>
            {track.name}
          </span>
          <span className="text-[10px] tabular-nums text-daw-faint">
            {String(index + 1).padStart(2, "0")}
          </span>
        </div>

        <div className="flex items-center gap-2">
          <Volume2 size={10} className="shrink-0 text-daw-faint" />
          <input
            type="range" min={0} max={1} step={0.01} value={track.volume}
            onClick={(e) => e.stopPropagation()}
            onChange={(e) => { e.stopPropagation(); const v = parseFloat(e.target.value); setTrackVolume(track.id, v); mixer.setVolume(track.id, v); }}
            className="flex-1"
            style={{ accentColor: track.color }}
          />
          <span className="min-w-6 text-right text-[10px] tabular-nums text-daw-faint">
            {Math.round(track.volume * 100)}
          </span>
        </div>

        <div className="flex items-center gap-1">
          <TrackBtn icon={VolumeX} active={track.muted} activeColor="#e0b24d" label="Mute"
            onClick={() => { setTrackMute(track.id, !track.muted); mixer.setMute(track.id, !track.muted); }} />
          <TrackBtn icon={Star} active={track.solo} activeColor="#63c174" label="Solo"
            onClick={() => { setTrackSolo(track.id, !track.solo); mixer.setSolo(track.id, !track.solo); }} />
          <TrackBtn icon={Mic} active={track.armed} activeColor="#f06a61" label="Arm"
            onClick={() => setTrackArmed(track.id, !track.armed)} />
          <div className="ml-auto h-1.5 w-16 overflow-hidden rounded bg-daw-bg">
            <div className="h-full rounded" style={{ width: `${Math.max(4, track.volume * 100)}%`, background: track.color }} />
          </div>
        </div>
      </div>
    </div>
  );
}
