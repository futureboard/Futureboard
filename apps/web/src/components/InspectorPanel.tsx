import { Cpu, GitMerge, Mic2, Music, Sliders, Volume2, X } from "lucide-react";
import { useProjectStore } from "../store/projectStore";
import { useUIStore } from "../store/uiStore";
import { mixer } from "../engine/Mixer";
import { INSPECTOR_WIDTH } from "../theme";
import { formatBeatLength } from "../utils/musicalTime";
import type { TrackType } from "../types/daw";

const TYPE_ICONS: Record<TrackType, React.ElementType> = {
  audio: Mic2,
  midi: Music,
  plugin: Cpu,
  bus: GitMerge,
};

const TYPE_LABELS: Record<TrackType, string> = {
  audio: "Audio",
  midi: "MIDI",
  plugin: "Plugin",
  bus: "Bus",
};

export function InspectorPanel() {
  const { selectedTrackId, toggleInspector } = useUIStore();
  const { project, setTrackVolume, setTrackPan } = useProjectStore();
  const trackIndex = project.tracks.findIndex((t) => t.id === selectedTrackId);
  const track = trackIndex >= 0 ? project.tracks[trackIndex] : null;
  const timeSig = project.timeSignature ?? { numerator: 4, denominator: 4 };

  const TypeIcon = track ? TYPE_ICONS[track.type] ?? Mic2 : Mic2;

  return (
    <div
      className="flex shrink-0 flex-col overflow-hidden border-l border-daw-border bg-daw-panel"
      style={{ width: INSPECTOR_WIDTH, minWidth: INSPECTOR_WIDTH }}
    >
      {/* Panel header */}
      <div className="flex h-6 shrink-0 items-center justify-between border-b border-daw-border bg-daw-surface px-3">
        <span className="text-[10px] font-semibold uppercase tracking-widest text-daw-faint">
          Inspector
        </span>
        <button
          onClick={toggleInspector}
          className="flex h-5 w-5 items-center justify-center rounded text-daw-faint transition-colors hover:bg-daw-surface-high hover:text-daw-text"
        >
          <X size={12} />
        </button>
      </div>

      {!track ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-2 px-8 text-center">
          <Sliders size={18} className="text-daw-faint opacity-30" />
          <p className="text-[11px] leading-relaxed text-daw-faint">
            Select a track to view channel settings
          </p>
        </div>
      ) : (
        <div className="flex-1 overflow-y-auto">

          {/* Track identity */}
          <div className="flex items-stretch border-b border-daw-border">
            <div className="w-[3px] shrink-0" style={{ background: track.color }} />
            <div className="flex-1 px-3 py-3">
              <div className="flex items-baseline justify-between gap-2">
                <span className="truncate text-[13px] font-semibold text-daw-text">
                  {track.name}
                </span>
                <span className="shrink-0 text-[9px] tabular-nums text-daw-faint">
                  {String(trackIndex + 1).padStart(2, "0")}
                </span>
              </div>
              <div className="mt-1 flex items-center gap-1.5 text-[10px] text-daw-faint">
                <TypeIcon size={9} />
                <span>{TYPE_LABELS[track.type]} Track</span>
              </div>
            </div>
          </div>

          {/* Channel faders */}
          <div className="flex flex-col gap-0 border-b border-daw-border">
            <FaderRow
              label="VOL"
              value={track.volume}
              min={0}
              max={1}
              color={track.color}
              display={`${Math.round(track.volume * 100)}%`}
              onChange={(v) => { setTrackVolume(track.id, v); mixer.setVolume(track.id, v); }}
            />
            <FaderRow
              label="PAN"
              value={(track.pan + 1) / 2}
              min={0}
              max={1}
              color="#a99cff"
              display={track.pan === 0 ? "C" : track.pan < 0 ? `L${Math.round(-track.pan * 100)}` : `R${Math.round(track.pan * 100)}`}
              onChange={(v) => { const p = (v * 2) - 1; setTrackPan(track.id, p); mixer.setPan(track.id, p); }}
            />
          </div>

          {/* Inserts */}
          <SectionLabel label="Inserts" />
          <div className="grid grid-cols-2 gap-1 px-3 pb-3">
            {Array.from({ length: 4 }, (_, i) => (
              <button
                key={i}
                className="flex h-7 items-center justify-center gap-1.5 rounded border border-dashed text-[9px] transition-colors"
                style={{
                  borderColor: "rgba(255,255,255,0.12)",
                  background: "rgba(255,255,255,0.025)",
                  color: "rgba(180,192,204,0.5)",
                }}
                onMouseEnter={(e) => {
                  (e.currentTarget as HTMLElement).style.borderColor = "rgba(255,255,255,0.22)";
                  (e.currentTarget as HTMLElement).style.color = "rgba(180,192,204,0.85)";
                }}
                onMouseLeave={(e) => {
                  (e.currentTarget as HTMLElement).style.borderColor = "rgba(255,255,255,0.12)";
                  (e.currentTarget as HTMLElement).style.color = "rgba(180,192,204,0.5)";
                }}
              >
                <Sliders size={9} />
                Empty
              </button>
            ))}
          </div>

          {/* Clips */}
          <SectionLabel label="Clips" count={track.clips.length} />
          <div className="px-3 pb-3">
            {track.clips.length === 0 ? (
              <p className="py-1 text-[10px] text-daw-faint">No clips on this track</p>
            ) : (
              <div className="flex flex-col gap-0.5">
                {track.clips.map((c) => (
                  <div
                    key={c.id}
                    className="flex items-center gap-2 rounded-md border border-daw-border bg-daw-bg px-2.5 py-1.5"
                  >
                    <Volume2 size={9} className="shrink-0 text-daw-faint" />
                    <span className="min-w-0 flex-1 truncate text-[10px] text-daw-dim">
                      {c.name}
                    </span>
                    <span className="shrink-0 text-[9px] tabular-nums text-daw-faint">
                      {formatBeatLength(c.duration, project.bpm, timeSig)}
                    </span>
                  </div>
                ))}
              </div>
            )}
          </div>

        </div>
      )}
    </div>
  );
}

function SectionLabel({ label, count }: { label: string; count?: number }) {
  return (
    <div className="flex items-center gap-1.5 px-3 pb-1.5 pt-3">
      <span className="text-[9px] font-semibold uppercase tracking-widest text-daw-faint">
        {label}
      </span>
      {count !== undefined && (
        <span className="text-[9px] text-daw-faint opacity-50">{count}</span>
      )}
    </div>
  );
}

function FaderRow({
  label, value, min, max, color, display, onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  color: string;
  display: string;
  onChange: (v: number) => void;
}) {
  return (
    <div className="flex items-center gap-2.5 border-b border-daw-border px-3 py-2">
      <span className="w-6 shrink-0 text-[9px] font-semibold uppercase tracking-widest text-daw-faint">
        {label}
      </span>
      <input
        type="range"
        min={min}
        max={max}
        step={0.001}
        value={value}
        onChange={(e) => onChange(parseFloat(e.target.value))}
        className="flex-1 cursor-ew-resize appearance-none"
        style={{ accentColor: color, height: "3px" }}
      />
      <span className="w-9 shrink-0 text-right text-[9px] tabular-nums text-daw-dim">
        {display}
      </span>
    </div>
  );
}
