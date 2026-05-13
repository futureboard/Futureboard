import { Activity, Cpu, GitMerge, Layers, Mic2, Music, Scissors, Sliders, Trash2, Volume2, X } from "lucide-react";
import { useProjectStore } from "../store/projectStore";
import { useUIStore } from "../store/uiStore";
import { useHistoryStore } from "../store/historyStore";
import { SetTrackVolumeCommand, SetTrackPanCommand, SetTrackMuteCommand, SetTrackSoloCommand, DeleteTrackCommand, UpdateClipCommand } from "../commands";
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
  const { selectedTrackId, selectedClipIds, selectedMixerTrackId, toggleInspector, masterVolume, setMasterVolume } = useUIStore();
  const { project } = useProjectStore();
  const history = useHistoryStore.getState;
  
  const trackIndex = project.tracks.findIndex((t) => t.id === selectedTrackId);
  const track = trackIndex >= 0 ? project.tracks[trackIndex] : null;

  const timeSig = project.timeSignature ?? { numerator: 4, denominator: 4 };

  const clip = selectedClipIds.length === 1 
    ? project.tracks.flatMap(t => t.clips).find(c => c.id === selectedClipIds[0])
    : null;

  let mode: "empty" | "master" | "track" | "clip" | "multi-clip" = "empty";
  
  if (selectedMixerTrackId === "master") mode = "master";
  else if (selectedClipIds.length > 1) mode = "multi-clip";
  else if (clip) mode = "clip";
  else if (track) mode = "track";

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

      <div className="flex-1 overflow-y-auto">
        {mode === "empty" && (
          <div className="flex h-full flex-col items-center justify-center gap-2 px-8 text-center">
            <Sliders size={18} className="text-daw-faint opacity-30" />
            <p className="text-[11px] leading-relaxed text-daw-faint">
              Select a track or clip to view settings
            </p>
          </div>
        )}

        {mode === "master" && (
          <>
            <div className="flex items-stretch border-b border-daw-border">
              <div className="w-[3px] shrink-0" style={{ background: "#48d1cc" }} />
              <div className="flex-1 px-3 py-3">
                <span className="truncate text-[13px] font-semibold text-daw-text">
                  Master
                </span>
                <div className="mt-1 flex items-center gap-1.5 text-[10px] text-daw-faint">
                  <Activity size={9} />
                  <span>Main Output</span>
                </div>
              </div>
            </div>
            <div className="flex flex-col gap-0 border-b border-daw-border">
              <FaderRow
                label="VOL"
                value={masterVolume}
                min={0}
                max={1}
                color="#48d1cc"
                display={`${Math.round(masterVolume * 100)}%`}
                onChange={(v) => { setMasterVolume(v); mixer.setMasterVolume(v); }}
              />
            </div>
            <SectionLabel label="Output Device" />
            <div className="px-3 pb-3 text-[10px] text-daw-faint">
              Default System Device (48000Hz)
            </div>
          </>
        )}

        {mode === "multi-clip" && (
          <div className="flex h-full flex-col items-center justify-center gap-2 px-8 text-center">
            <Layers size={18} className="text-daw-faint opacity-30" />
            <p className="text-[11px] leading-relaxed text-daw-faint">
              {selectedClipIds.length} clips selected
            </p>
          </div>
        )}

        {mode === "clip" && clip && (
          <>
            <div className="flex items-stretch border-b border-daw-border">
              <div className="w-[3px] shrink-0" style={{ background: "#f3c969" }} />
              <div className="flex-1 px-3 py-3">
                <input
                  defaultValue={clip.name}
                  onBlur={(e) => {
                    const newName = e.target.value;
                    if (newName !== clip.name) history().execute(new UpdateClipCommand(clip.id, { name: newName }, "Rename Clip"));
                  }}
                  className="w-full bg-transparent text-[13px] font-semibold text-daw-text outline-none placeholder:text-white/20"
                  placeholder="Clip Name"
                />
                <div className="mt-1 flex items-center gap-1.5 text-[10px] text-daw-faint">
                  <Scissors size={9} />
                  <span>Audio Clip</span>
                </div>
              </div>
            </div>

            <div className="flex flex-col gap-0 border-b border-daw-border">
              <FaderRow
                label="GAIN"
                value={clip.gain}
                min={0}
                max={2}
                color="#f3c969"
                display={`${Math.round(clip.gain * 100)}%`}
                onChange={(v) => history().execute(new UpdateClipCommand(clip.id, { gain: v }, "Set Clip Gain"))}
              />
              <div className="flex items-center justify-between border-b border-daw-border px-3 py-2">
                <span className="text-[9px] font-semibold uppercase tracking-widest text-daw-faint">Mute</span>
                <input type="checkbox" checked={clip.muted ?? false} onChange={(e) => history().execute(new UpdateClipCommand(clip.id, { muted: e.target.checked }, e.target.checked ? "Mute Clip" : "Unmute Clip"))} />
              </div>
            </div>

            <SectionLabel label="Timing" />
            <div className="flex flex-col gap-2 px-3 pb-3">
              <div className="flex justify-between text-[10px] text-daw-dim">
                <span>Start Time</span>
                <span className="tabular-nums">{clip.startTime.toFixed(3)}s</span>
              </div>
              <div className="flex justify-between text-[10px] text-daw-dim">
                <span>Duration</span>
                <span className="tabular-nums">{clip.duration.toFixed(3)}s</span>
              </div>
              <div className="flex justify-between text-[10px] text-daw-dim">
                <span>Offset</span>
                <span className="tabular-nums">{clip.offset.toFixed(3)}s</span>
              </div>
            </div>
          </>
        )}

        {mode === "track" && track && (
          <>
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
                onChange={(v) => history().execute(new SetTrackVolumeCommand(track.id, v, track.volume))}
              />
              <FaderRow
                label="PAN"
                value={(track.pan + 1) / 2}
                min={0}
                max={1}
                color="#a99cff"
                display={track.pan === 0 ? "C" : track.pan < 0 ? `L${Math.round(-track.pan * 100)}` : `R${Math.round(track.pan * 100)}`}
                onChange={(v) => { const p = (v * 2) - 1; history().execute(new SetTrackPanCommand(track.id, p, track.pan)); }}
              />
            </div>
            {/* Mute / Solo / Arm / Delete row */}
            <div className="flex items-center gap-1.5 border-b border-daw-border px-3 py-2">
              <InspectorTrackBtn
                label="M"
                title="Mute"
                active={track.muted}
                activeColor="#f3c969"
                onClick={() => history().execute(new SetTrackMuteCommand(track.id, !track.muted))}
              />
              <InspectorTrackBtn
                label="S"
                title="Solo"
                active={track.solo}
                activeColor="#7bd88f"
                onClick={() => history().execute(new SetTrackSoloCommand(track.id, !track.solo))}
              />
              <InspectorTrackBtn
                label="A"
                title="Arm"
                active={track.armed ?? false}
                activeColor="#f06a61"
                onClick={() => useProjectStore.getState().setTrackArmed(track.id, !track.armed)}
              />
              <div className="flex-1" />
              <button
                title="Delete Track"
                onClick={() => {
                  history().execute(new DeleteTrackCommand(track.id));
                  useUIStore.getState().setSelectedTrackId(null);
                  useUIStore.getState().setSelectedMixerTrackId(null);
                }}
                className="flex h-6 w-6 shrink-0 items-center justify-center rounded transition-colors hover:bg-red-500/15 text-daw-faint hover:text-red-400"
              >
                <Trash2 size={10} />
              </button>
            </div>


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
          </>
        )}
      </div>
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

function InspectorTrackBtn({
  label,
  title,
  active,
  activeColor,
  onClick,
}: {
  label: string;
  title: string;
  active: boolean;
  activeColor: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      title={title}
      aria-pressed={active}
      onClick={onClick}
      className="flex h-6 w-6 shrink-0 items-center justify-center rounded border text-[10px] font-bold transition-colors"
      style={{
        background: active ? activeColor : "rgba(255,255,255,0.035)",
        borderColor: active ? activeColor : "rgba(255,255,255,0.08)",
        color: active ? "#101216" : "rgba(200,212,224,0.62)",
      }}
    >
      {label}
    </button>
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
