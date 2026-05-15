import { useState, useEffect, useRef } from "react";
import { Activity, ArrowUpDown, CornerDownLeft, Cpu, GitFork, GitMerge, Layers, Mic2, Music, PhoneIncoming, PhoneOutgoing, RotateCcw, Scissors, Sliders, Trash2, Volume2, X } from "lucide-react";
import { useProjectStore } from "../store/projectStore";
import { useUIStore } from "../store/uiStore";
import { useHistoryStore } from "../store/historyStore";
import { SetTrackVolumeCommand, SetTrackPanCommand, SetTrackMuteCommand, SetTrackSoloCommand, SetTrackOutputCommand, DeleteTrackCommand, UpdateClipCommand } from "../commands";
import { mixer } from "../engine/Mixer";
import { INSPECTOR_WIDTH } from "../theme";
import { formatBeatLength } from "../utils/musicalTime";
import type { TrackType, AudioClipProcess, DawClip } from "../types/daw";
import { clipType } from "../types/daw";
import { getOutputTargets, getSendTargets } from "../utils/routingHelpers";
import { DEFAULT_AUDIO_PROCESS } from "../utils/normalize";
import { audioProcessingService } from "../audio/AudioProcessingService";
import { audioCacheManager } from "../audio/AudioCacheManager";
import { buildDecodedCacheKey } from "../audio/audioCacheKeys";
import { audioEngine } from "../engine/AudioEngine";
import { DawSelect } from "./ui/DawSelect";
import { clipScheduler } from "../engine/ClipScheduler";
import { transport } from "../engine/Transport";

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

const TYPE_LABELS: Record<TrackType, string> = {
  audio:      "Audio",
  midi:       "MIDI",
  instrument: "Instrument",
  plugin:     "Plugin",
  bus:        "Bus",
  return:     "Return",
  group:      "Group",
  master:     "Master",
};

export function InspectorPanel({ width }: { width?: number } = {}) {
  const { selectedTrackId, selectedClipIds, selectedMixerTrackId, togglePanel, masterVolume, setMasterVolume } = useUIStore();
  const toggleInspector = () => togglePanel("inspector");
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
      style={{ width: width ?? INSPECTOR_WIDTH, minWidth: width ?? INSPECTOR_WIDTH }}
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

        {mode === "clip" && clip && (() => {
          const isAudio = clipType(clip) === "audio";
          return (
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
                    {isAudio ? <Scissors size={9} /> : <Music size={9} />}
                    <span>{isAudio ? "Audio Clip" : "MIDI Clip"}</span>
                  </div>
                </div>
              </div>

              <div className="flex flex-col gap-0 border-b border-daw-border">
                {isAudio && (
                  <FaderRow
                    label="GAIN"
                    value={clip.gain}
                    min={0}
                    max={2}
                    color="#f3c969"
                    display={`${Math.round(clip.gain * 100)}%`}
                    onChange={(v) => {
                      history().execute(new UpdateClipCommand(clip.id, { gain: v }, "Set Clip Gain"));
                      clipScheduler.updateClipGain(clip.id, v);
                    }}
                  />
                )}
                <div className="flex items-center justify-between border-b border-daw-border px-3 py-2">
                  <span className="text-[9px] font-semibold uppercase tracking-widest text-daw-faint">Mute</span>
                  <input
                    type="checkbox"
                    checked={clip.muted ?? false}
                    onChange={(e) => {
                      const muted = e.target.checked;
                      history().execute(new UpdateClipCommand(clip.id, { muted }, muted ? "Mute Clip" : "Unmute Clip"));
                      clipScheduler.updateClipMute(clip.id, muted);
                    }}
                  />
                </div>
              </div>

              {isAudio && (
                <>
                  <SectionLabel label="Fades" />
                  <div className="flex flex-col gap-0 border-b border-daw-border">
                    <FaderRow
                      label="IN"
                      value={clip.fadeIn ?? 0}
                      min={0}
                      max={Math.min(clip.duration, 10)}
                      color="#f3c969"
                      display={`${(clip.fadeIn ?? 0).toFixed(2)}s`}
                      onChange={(v) => history().execute(new UpdateClipCommand(clip.id, { fadeIn: v }, "Set Fade In"))}
                    />
                    <FaderRow
                      label="OUT"
                      value={clip.fadeOut ?? 0}
                      min={0}
                      max={Math.min(clip.duration, 10)}
                      color="#f3c969"
                      display={`${(clip.fadeOut ?? 0).toFixed(2)}s`}
                      onChange={(v) => history().execute(new UpdateClipCommand(clip.id, { fadeOut: v }, "Set Fade Out"))}
                    />
                  </div>
                </>
              )}

              {!isAudio && (
                <>
                  <SectionLabel label="MIDI" />
                  <div className="flex flex-col gap-0 border-b border-daw-border opacity-45">
                    <DimRow label="CH" value="1" title="MIDI channel (coming soon)" />
                    <DimRow label="VEL" value="100" title="Default velocity (coming soon)" />
                    <DimRow label="QNTZ" value="1/16" title="Quantize (coming soon)" />
                    <DimRow label="TRNSP" value="0 st" title="Transpose (coming soon)" />
                  </div>
                </>
              )}

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

              {isAudio && (
                <ClipProcessSection clip={clip} />
              )}
            </>
          );
        })()}

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

            {/* Routing */}
            <SectionLabel label="Routing" />
            <div className="flex flex-col gap-1.5 px-3 pb-3">
              <div className="flex items-center gap-2 opacity-50">
                <PhoneIncoming size={9} className="shrink-0 text-daw-faint" />
                <span className="w-8 shrink-0 text-[9px] text-daw-faint">IN</span>
                <div
                  className="flex h-6 flex-1 cursor-not-allowed items-center rounded px-2 text-[10px] text-daw-faint"
                  style={{ background: "rgba(255,255,255,0.025)", border: "1px solid rgba(255,255,255,0.06)" }}
                  title="Input routing (coming soon)"
                >
                  System Input
                </div>
              </div>
              <div className="flex items-center gap-2">
                <PhoneOutgoing size={9} className="shrink-0 text-daw-faint" />
                <span className="w-8 shrink-0 text-[9px] text-daw-faint">OUT</span>
                <DawSelect
                  value={track.output ?? "master"}
                  onChange={(next) => {
                    history().execute(new SetTrackOutputCommand(track.id, next, track.output ?? "master"));
                  }}
                  options={getOutputTargets(project, track.id).map((t) => ({
                    value: t.id,
                    label: t.name,
                  }))}
                />
              </div>
            </div>

            {/* Sends */}
            {(() => {
              const sendTargets = getSendTargets(project, track.id);
              const sends = track.sends ?? [];
              if (sendTargets.length === 0 && sends.length === 0) return null;
              return (
                <>
                  <SectionLabel label="Sends" />
                  <div className="flex flex-col gap-0.5 px-3 pb-3">
                    {sends.map((send) => {
                      const target = project.tracks.find((t) => t.id === send.targetTrackId);
                      return (
                        <div key={send.id} className="flex items-center gap-2 rounded border border-daw-border bg-daw-bg px-2 py-1">
                          <span className="min-w-0 flex-1 truncate text-[10px] text-daw-dim">
                            {target?.name ?? send.name}
                          </span>
                          <span className="shrink-0 text-[9px] tabular-nums text-daw-faint">
                            {send.level >= 0.999 ? "0.0" : (20 * Math.log10(Math.max(0.001, send.level))).toFixed(1)} dB
                          </span>
                        </div>
                      );
                    })}
                    {sends.length === 0 && (
                      <p className="py-0.5 text-[10px] text-daw-faint">No sends — add from Mixer</p>
                    )}
                  </div>
                </>
              );
            })()}

            <SectionLabel label="Inserts" count={(track.inserts ?? []).length} />
            <InspectorInsertsList trackId={track.id} />


            {/* Advanced (dimmed placeholders) */}
            <SectionLabel label="Advanced" />
            <div className="flex flex-col gap-0 border-b border-daw-border opacity-45">
              <DimRow label="LATENCY" value="0 ms" title="Latency compensation (coming soon)" />
              <DimRow label="DELAY" value="0 ms" title="Track delay offset (coming soon)" />
              <DimRow label="SEMI" value="0 st" title="Semitone pitch offset (coming soon)" />
            </div>
            <div className="flex items-center gap-3 border-b border-daw-border px-3 py-2 opacity-45">
              <span className="text-[9px] font-semibold uppercase tracking-widest text-daw-faint">Phase</span>
              <div className="flex-1" />
              <div
                className="flex h-5 w-8 cursor-not-allowed items-center justify-center rounded border text-[9px] font-bold text-daw-faint"
                style={{ borderColor: "rgba(255,255,255,0.08)", background: "rgba(255,255,255,0.02)" }}
                title="Phase invert (coming soon)"
              >
                Ø
              </div>
              <div
                className="flex h-5 w-8 cursor-not-allowed items-center justify-center rounded border text-[9px] font-bold text-daw-faint"
                style={{ borderColor: "rgba(255,255,255,0.08)", background: "rgba(255,255,255,0.02)" }}
                title="Mono/stereo mode (coming soon)"
              >
                M/S
              </div>
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

function InspectorInsertsList({ trackId }: { trackId: string }) {
  const { project, toggleInsertDevice, removeInsertDevice } = useProjectStore();
  const track = project.tracks.find((t) => t.id === trackId);
  const inserts = (track?.inserts ?? []).slice().sort((a, b) => a.order - b.order);

  if (inserts.length === 0) {
    return (
      <div className="px-3 pb-3">
        <p className="py-0.5 text-[10px] text-daw-faint">No inserts — add from Mixer or Effect Editor</p>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-0.5 px-3 pb-3">
      {inserts.map((ins) => (
        <div
          key={ins.id}
          className="group flex items-center gap-1.5 rounded border px-2 py-[3px]"
          style={{
            borderColor: ins.enabled ? "rgba(255,255,255,0.12)" : "rgba(255,255,255,0.06)",
            background: "rgba(255,255,255,0.025)",
          }}
        >
          <Sliders size={9} className="shrink-0 text-daw-faint" />
          <button
            onClick={() => toggleInsertDevice(trackId, ins.id)}
            className="min-w-0 flex-1 truncate text-left text-[10px]"
            style={{ color: ins.enabled ? "rgba(220,232,240,0.78)" : "rgba(180,192,204,0.4)" }}
            title={ins.enabled ? "Bypass device" : "Enable device"}
          >
            {ins.name}
          </button>
          <button
            onClick={() => removeInsertDevice(trackId, ins.id)}
            title="Remove device"
            className="opacity-0 transition-opacity hover:text-red-400 group-hover:opacity-100 text-daw-faint"
          >
            <X size={9} />
          </button>
        </div>
      ))}
    </div>
  );
}

function DimRow({ label, value, title }: { label: string; value: string; title?: string }) {
  return (
    <div
      className="flex cursor-not-allowed items-center gap-2.5 border-b border-daw-border px-3 py-2"
      title={title}
    >
      <span className="w-9 shrink-0 text-[9px] font-semibold uppercase tracking-widest text-daw-faint">
        {label}
      </span>
      <div
        className="flex h-5 flex-1 items-center rounded px-2 text-[9px] text-daw-faint"
        style={{ background: "rgba(255,255,255,0.02)", border: "1px solid rgba(255,255,255,0.05)" }}
      >
        {value}
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

// ── Audio process section ─────────────────────────────────────────────────────

import type { ProcessorKind } from "../audio/AudioProcessingService";

type ProcessStatus = "idle" | "processing" | "cached" | "failed";

const MODE_LABELS: Record<AudioClipProcess["mode"], string> = {
  resample:    "Resample (tape)",
  monophonic:  "Monophonic",
  polyphonic:  "Polyphonic",
  percussive:  "Percussive",
  granular:    "Granular / Texture",
};

function processorLabel(kind: ProcessorKind, mode: AudioClipProcess["mode"]): string {
  switch (kind) {
    case "rust-wasm":   return "✓ Rust WASM";
    case "ts-wsola":    return `✓ WSOLA (${mode})`;
    case "ts-granular": return `✓ Granular (${mode})`;
    case "ts-resample": return "✓ Resample";
    default:            return "✓ Ready";
  }
}

function ClipProcessSection({ clip }: { clip: DawClip }) {
  const history = useHistoryStore.getState;
  const proc = clip.audioProcess ?? DEFAULT_AUDIO_PROCESS;

  // Local draft so slider drags don't spam history until mouseup
  const [draft, setDraft] = useState<AudioClipProcess>(proc);
  const [processStatus, setProcessStatus] = useState<ProcessStatus>("idle");
  const [processorUsed, setProcessorUsed] = useState<ProcessorKind | null>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Keep draft in sync when a different clip is selected
  const procKey = `${clip.id}:${proc.speedRatio}:${proc.pitchSemitones}:${proc.preservePitch}:${proc.mode}:${proc.quality}`;
  const [lastKey, setLastKey] = useState(procKey);
  if (lastKey !== procKey) {
    setLastKey(procKey);
    setDraft(proc);
  }

  const commit = (patch: Partial<AudioClipProcess>) => {
    const next: AudioClipProcess = { ...draft, ...patch };
    setDraft(next);
    history().execute(new UpdateClipCommand(clip.id, { audioProcess: next }, "Set Clip Process"));
  };

  // Trigger processing whenever committed params change (debounced 300ms).
  useEffect(() => {
    const hasEffect = proc.speedRatio !== 1 || proc.pitchSemitones !== 0;
    if (!hasEffect) { setProcessStatus("idle"); setProcessorUsed(null); return; }

    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(async () => {
      const audioBuffer = audioEngine.getBuffer(clip.fileId);
      if (!audioBuffer) return;
      const key = buildDecodedCacheKey(clip.fileId, audioBuffer.audioBuffer.sampleRate);
      const decoded = audioCacheManager.getDecodedAudio(key);
      if (!decoded) return;

      setProcessStatus("processing");
      try {
        const { processorUsed: kind } = await audioProcessingService.processClipAudio(decoded, {
          speedRatio:     proc.speedRatio,
          pitchSemitones: proc.pitchSemitones,
          preservePitch:  proc.preservePitch,
          mode:           proc.mode ?? "polyphonic",
          quality:        proc.quality,
        });
        setProcessStatus("cached");
        setProcessorUsed(kind);
        transport.rescheduleIfPlaying();
      } catch {
        setProcessStatus("failed");
        setProcessorUsed(null);
      }
    }, 300);

    return () => { if (debounceRef.current) clearTimeout(debounceRef.current); };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [clip.fileId, proc.speedRatio, proc.pitchSemitones, proc.preservePitch, proc.mode, proc.quality]);

  const isModified = draft.speedRatio !== 1 || draft.pitchSemitones !== 0;

  const inputCls =
    "flex h-5 flex-1 items-center rounded px-2 text-[10px] text-daw-text bg-daw-bg border border-daw-border focus:outline-none focus:border-blue-500 tabular-nums";

  // When preserve-pitch is off, mode is irrelevant (resample path always used)
  const modeDisabled = !draft.preservePitch;

  return (
    <>
      <SectionLabel label="Speed & Pitch" />
      <div className="flex flex-col gap-0 border-b border-daw-border">
        {/* Speed */}
        <div className="flex items-center gap-2.5 border-b border-daw-border px-3 py-2">
          <span className="w-9 shrink-0 text-[9px] font-semibold uppercase tracking-widest text-daw-faint">
            SPEED
          </span>
          <input
            type="range" min={0.25} max={4} step={0.01}
            value={draft.speedRatio}
            onChange={(e) => setDraft((s) => ({ ...s, speedRatio: parseFloat(e.target.value) }))}
            onMouseUp={() => commit({ speedRatio: draft.speedRatio })}
            className="flex-1 cursor-ew-resize appearance-none"
            style={{ accentColor: "#5fced0", height: "3px" }}
          />
          <input
            type="number" min={0.25} max={4} step={0.05}
            value={draft.speedRatio.toFixed(2)}
            onChange={(e) => {
              const v = Math.max(0.25, Math.min(4, parseFloat(e.target.value) || 1));
              commit({ speedRatio: v });
            }}
            className={`${inputCls} w-14`}
          />
        </div>

        {/* Pitch semitones */}
        <div className="flex items-center gap-2.5 border-b border-daw-border px-3 py-2">
          <span className="w-9 shrink-0 text-[9px] font-semibold uppercase tracking-widest text-daw-faint">
            PITCH
          </span>
          <input
            type="range" min={-24} max={24} step={1}
            value={draft.pitchSemitones}
            onChange={(e) => setDraft((s) => ({ ...s, pitchSemitones: parseInt(e.target.value, 10) }))}
            onMouseUp={() => commit({ pitchSemitones: draft.pitchSemitones })}
            className="flex-1 cursor-ew-resize appearance-none"
            style={{ accentColor: "#a99cff", height: "3px" }}
          />
          <input
            type="number" min={-24} max={24} step={1}
            value={draft.pitchSemitones}
            onChange={(e) => {
              const v = Math.max(-24, Math.min(24, parseInt(e.target.value, 10) || 0));
              commit({ pitchSemitones: v });
            }}
            className={`${inputCls} w-14`}
          />
        </div>

        {/* Preserve pitch toggle */}
        <div className="flex items-center gap-2.5 border-b border-daw-border px-3 py-2">
          <span className="w-9 shrink-0 text-[9px] font-semibold uppercase tracking-widest text-daw-faint">
            PRES
          </span>
          <span className="flex-1 text-[10px] text-daw-dim">Preserve Pitch</span>
          <button
            type="button" role="switch" aria-checked={draft.preservePitch}
            onClick={() => commit({ preservePitch: !draft.preservePitch })}
            className={`relative inline-flex h-4 w-8 items-center rounded-full transition-colors ${
              draft.preservePitch ? "bg-blue-600" : "border border-daw-border bg-daw-surface"
            }`}
          >
            <span className={`inline-block h-3 w-3 transform rounded-full bg-white shadow transition-transform ${
              draft.preservePitch ? "translate-x-4" : "translate-x-0.5"
            }`} />
          </button>
        </div>

        {/* Mode */}
        <div className="flex items-center gap-2.5 border-b border-daw-border px-3 py-2">
          <span className="w-9 shrink-0 text-[9px] font-semibold uppercase tracking-widest text-daw-faint">
            MODE
          </span>
          <DawSelect
            value={modeDisabled ? "resample" : (draft.mode ?? "polyphonic")}
            onChange={(val) => commit({ mode: val as AudioClipProcess["mode"] })}
            disabled={modeDisabled}
            options={[
              { value: "resample",    label: MODE_LABELS.resample },
              { value: "monophonic",  label: MODE_LABELS.monophonic },
              { value: "polyphonic",  label: MODE_LABELS.polyphonic },
              { value: "percussive",  label: MODE_LABELS.percussive },
              { value: "granular",    label: MODE_LABELS.granular },
            ]}
          />
        </div>

        {/* Quality */}
        <div className="flex items-center gap-2.5 px-3 py-2">
          <span className="w-9 shrink-0 text-[9px] font-semibold uppercase tracking-widest text-daw-faint">
            QUAL
          </span>
          <DawSelect
            value={draft.quality}
            onChange={(val) => commit({ quality: val as AudioClipProcess["quality"] })}
            options={[
              { value: "draft",    label: "Draft" },
              { value: "balanced", label: "Balanced" },
              { value: "high",     label: "High", disabled: true },
            ]}
          />
        </div>
      </div>

      {/* Process status */}
      {isModified && processStatus !== "idle" && (
        <div className="flex items-center gap-1.5 px-3 py-1.5 border-b border-daw-border">
          <span
            className="text-[9px] tabular-nums"
            style={{
              color: processStatus === "cached" ? "#7bd88f"
                   : processStatus === "failed"  ? "#f06a61"
                   : "#a99cff",
            }}
          >
            {processStatus === "processing" && "⏳ Processing…"}
            {processStatus === "cached" && processorUsed
              ? processorLabel(processorUsed, draft.mode ?? "polyphonic")
              : processStatus === "cached" ? "✓ Ready" : null}
            {processStatus === "failed" && "✗ Failed — check console"}
          </span>
        </div>
      )}

      {/* Reset row */}
      {isModified && (
        <div className="flex items-center gap-2 px-3 py-2 border-b border-daw-border">
          <span className="flex-1 text-[10px] text-daw-faint">
            {draft.speedRatio !== 1 ? `${draft.speedRatio.toFixed(2)}×` : ""}
            {draft.speedRatio !== 1 && draft.pitchSemitones !== 0 ? " · " : ""}
            {draft.pitchSemitones !== 0
              ? `${draft.pitchSemitones > 0 ? "+" : ""}${draft.pitchSemitones} st`
              : ""}
          </span>
          <button
            type="button"
            onClick={() => commit({ speedRatio: 1, pitchSemitones: 0, preservePitch: true, mode: "polyphonic" })}
            className="flex items-center gap-1 rounded px-2 py-0.5 text-[9px] text-daw-faint hover:text-daw-text hover:bg-white/5 border border-daw-border"
            title="Reset speed and pitch to defaults"
          >
            <RotateCcw size={8} />
            Reset
          </button>
        </div>
      )}

      {/* Dimmed future process actions */}
      <div className="grid grid-cols-2 gap-1 px-3 pb-3 pt-2 opacity-40">
        <button type="button" disabled
          className="flex h-7 cursor-not-allowed items-center justify-center gap-1 rounded border border-dashed text-[9px]"
          style={{ borderColor: "rgba(255,255,255,0.12)", background: "rgba(255,255,255,0.02)", color: "rgba(180,192,204,0.6)" }}
          title="Reverse (coming soon)">
          <RotateCcw size={9} /> Reverse
        </button>
        <button type="button" disabled
          className="flex h-7 cursor-not-allowed items-center justify-center gap-1 rounded border border-dashed text-[9px]"
          style={{ borderColor: "rgba(255,255,255,0.12)", background: "rgba(255,255,255,0.02)", color: "rgba(180,192,204,0.6)" }}
          title="Normalize (coming soon)">
          <ArrowUpDown size={9} /> Normalize
        </button>
      </div>
    </>
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
