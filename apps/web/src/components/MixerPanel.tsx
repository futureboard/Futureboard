import { ChevronDown } from "lucide-react";
import { useProjectStore } from "../store/projectStore";
import { useUIStore } from "../store/uiStore";
import { mixer } from "../engine/Mixer";
import { MIXER_HEIGHT } from "../theme";
import { VuMeter } from "./ui/VuMeter";

function MixerBtn({ label, active, activeColor, onClick }: {
  label: string; active: boolean; activeColor: string; onClick?: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className="h-5 w-5 rounded border text-[10px] font-bold transition-colors"
      style={{ background: active ? activeColor : "#111418", borderColor: active ? activeColor : "#303943", color: active ? "#07090b" : "#8d99a7" }}
    >
      {label}
    </button>
  );
}

function ChannelStrip({ label, color = "#3d4854", volume, onVolume, muted, solo, onMute, onSolo, isMaster = false }: {
  label: string; color?: string; volume: number; onVolume: (v: number) => void;
  muted?: boolean; solo?: boolean; onMute?: () => void; onSolo?: () => void; isMaster?: boolean;
}) {
  return (
    <div className={[
      "flex flex-col items-center gap-2 overflow-hidden border-r border-daw-border px-2 py-2",
      isMaster ? "w-20 min-w-20 bg-daw-sunken" : "w-16 min-w-16 bg-daw-surface",
    ].join(" ")}>
      <div className="w-full overflow-hidden rounded px-1 py-1" style={{ background: color }}>
        <span className="block truncate text-center text-[10px] font-semibold"
          style={{ color: isMaster ? "#07090b" : "rgba(0,0,0,0.75)" }}>
          {label}
        </span>
      </div>

      {!isMaster && (
        <div className="flex gap-1">
          <MixerBtn label="M" active={!!muted} activeColor="#e0b24d" onClick={onMute} />
          <MixerBtn label="S" active={!!solo}  activeColor="#63c174" onClick={onSolo} />
        </div>
      )}

      <div className="flex w-full flex-1 items-end justify-center gap-2">
        <input
          type="range" min={0} max={1} step={0.01} value={volume}
          onChange={(e) => onVolume(parseFloat(e.target.value))}
          className="vertical flex-1"
          style={{ accentColor: isMaster ? "#5aa7ff" : color } as React.CSSProperties}
        />
        <VuMeter level={volume * 0.85} height={56} width={5} />
      </div>

      <span className="text-[10px] tabular-nums text-daw-faint">{Math.round(volume * 100)}</span>
    </div>
  );
}

export function MixerPanel() {
  const tracks = useProjectStore((s) => s.project.tracks);
  const { setTrackVolume, setTrackMute, setTrackSolo } = useProjectStore();
  const { masterVolume, setMasterVolume, toggleMixer } = useUIStore();

  return (
    <div
      className="flex shrink-0 flex-col border-t border-daw-border bg-daw-surface-high"
      style={{ height: MIXER_HEIGHT, minHeight: MIXER_HEIGHT }}
    >
      <div className="flex h-7 shrink-0 items-center gap-2 border-b border-daw-border bg-daw-sunken px-2.5">
        <span className="text-[12px] font-semibold text-daw-dim">Mixer</span>
        <div className="flex-1" />
        <button onClick={toggleMixer} className="flex items-center rounded p-1 text-daw-faint transition-colors hover:bg-daw-surface-high hover:text-daw-text">
          <ChevronDown size={11} />
        </button>
      </div>

      <div className="flex flex-1 overflow-x-auto overflow-y-hidden">
        <ChannelStrip
          label="Master" isMaster volume={masterVolume}
          onVolume={(v) => { setMasterVolume(v); mixer.setMasterVolume(v); }}
        />
        {tracks.map((t) => (
          <ChannelStrip
            key={t.id} label={t.name} color={t.color}
            volume={t.volume} muted={t.muted} solo={t.solo}
            onVolume={(v) => { setTrackVolume(t.id, v); mixer.setVolume(t.id, v); }}
            onMute={() => { setTrackMute(t.id, !t.muted); mixer.setMute(t.id, !t.muted); }}
            onSolo={() => { setTrackSolo(t.id, !t.solo); mixer.setSolo(t.id, !t.solo); }}
          />
        ))}
      </div>
    </div>
  );
}
