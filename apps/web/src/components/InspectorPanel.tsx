import { useState, useEffect } from "react";
import { Activity, AlertCircle, ArrowUpDown, CornerDownLeft, Cpu, GitFork, GitMerge, Layers, Mic2, Music, PhoneIncoming, PhoneOutgoing, RotateCcw, Scissors, Sliders, Trash2, Volume2, X, Radio } from "lucide-react";
import { useProjectStore } from "../store/projectStore";
import { useUIStore } from "../store/uiStore";
import { useHistoryStore } from "../store/historyStore";
import { SetTrackVolumeCommand, SetTrackPanCommand, SetTrackMuteCommand, SetTrackSoloCommand, SetTrackOutputCommand, DeleteTrackCommand, UpdateClipCommand } from "../commands";
import { mixer } from "../engine/Mixer";
import { INSPECTOR_WIDTH } from "../theme";
import { formatBeatLength } from "../utils/musicalTime";
import type { TrackType, TrackRouting, AudioClipProcess, DawClip, DawTrack } from "../types/daw";
import { clipType } from "../types/daw";
import { getOutputTargets, getSendTargets } from "../utils/routingHelpers";
import { DEFAULT_AUDIO_PROCESS } from "../utils/normalize";
import { audioProcessingService } from "../audio/AudioProcessingService";
import { audioCacheManager } from "../audio/AudioCacheManager";
import { buildDecodedCacheKey } from "../audio/audioCacheKeys";
import { audioEngine } from "../engine/AudioEngine";
import { DawSelect } from "./ui/DawSelect";
import { NumberInput } from "./ui/NumberInput";
import type { DawSelectOption } from "./ui/DawSelect";
import { clipScheduler } from "../engine/ClipScheduler";
import { transport } from "../engine/Transport";
import { useDeviceStore } from "../store/deviceStore";
import { useAudioSettingsStore } from "../store/audioSettingsStore";
import { audioDeviceService } from "../engine/AudioDeviceService";
import { midiDeviceService } from "../engine/MidiDeviceService";
import { platform } from "../platform";
import { useMidiInput } from "../hooks/useMidiInput";
import type { MidiEventLog, MidiEventType } from "../hooks/useMidiInput";

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
            <TrackRoutingSection trackId={track.id} />

            {/* MIDI live monitor (midi tracks only) */}
            {track.type === "midi" && <MidiMonitorSection track={track} />}

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


            {/* Advanced */}
            <SectionLabel label="Advanced" />
            <TrackAdvancedSection trackId={track.id} />

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

// ── Routing encode/decode helpers ─────────────────────────────────────────────

/** Encode track.routing into a single select value string (channel-based). */
function encodeInputValue(routing?: TrackRouting): string {
  if (!routing || routing.inputType === "none") return "none";
  if (routing.inputType === "audio-channel") {
    const ch = routing.inputChannel;
    if (ch === "stereo") return "ch:stereo";
    if (typeof ch === "number") return `ch:${ch}`;
    return "ch:stereo";
  }
  // Migrate legacy flat fields to channel-based encoding
  if (routing.inputType === "system-audio") return "ch:stereo";
  if (routing.inputType === "audio-device") return "ch:stereo";
  if (routing.inputType === "midi-device") return routing.inputId ? `midi:${routing.inputId}` : "midi-all";
  if (routing.inputType === "bus") return routing.inputId ? `bus:${routing.inputId}` : "none";
  if (routing.inputType === "track") return routing.inputId ? `track:${routing.inputId}` : "none";
  return "none";
}

/** Decode a select value string back to a TrackRouting partial. */
function decodeInputValue(value: string): Partial<TrackRouting> {
  if (value === "none") return { inputType: "none", inputId: undefined, inputChannel: undefined };
  if (value === "ch:stereo") return { inputType: "audio-channel", inputId: undefined, inputChannel: "stereo" };
  if (value.startsWith("ch:")) {
    const ch = parseInt(value.slice(3), 10);
    return { inputType: "audio-channel", inputId: undefined, inputChannel: isNaN(ch) ? "stereo" : ch };
  }
  if (value === "midi-all") return { inputType: "midi-device", inputId: undefined };
  if (value.startsWith("midi:")) return { inputType: "midi-device", inputId: value.slice(5) };
  if (value.startsWith("bus:"))  return { inputType: "bus",        inputId: value.slice(4) };
  if (value.startsWith("track:")) return { inputType: "track",     inputId: value.slice(6) };
  return { inputType: "none" };
}

/** Build BFS-reachable output set for circular routing detection. */
function buildOutputReachable(project: { tracks: { id: string; output?: string }[] }, fromTrackId: string): Set<string> {
  const visited = new Set<string>();
  const queue = [fromTrackId];
  while (queue.length > 0) {
    const cur = queue.shift()!;
    if (visited.has(cur)) continue;
    visited.add(cur);
    const t = project.tracks.find((x) => x.id === cur);
    const out = t?.output;
    if (out && out !== "master" && out !== "none" && !visited.has(out)) {
      queue.push(out);
    }
  }
  return visited;
}

// ── TrackRoutingSection ───────────────────────────────────────────────────────

function TrackRoutingSection({ trackId }: { trackId: string }) {
  const { project, updateTrackRouting } = useProjectStore();
  const { audioPermission, audioInputs, audioOutputs, midiPermission, midiInputs } = useDeviceStore();
  const { audioInputDeviceId, midiEnabledInputIds } = useAudioSettingsStore();
  const history = useHistoryStore.getState;

  const track = project.tracks.find((t) => t.id === trackId);
  if (!track) return null;

  const routing = track.routing;
  const isMaster = track.type === "master";
  const isMidi = track.type === "midi";
  const isAudio = track.type === "audio" || track.type === "instrument" || track.type === "plugin";

  // Global audio input device (for channel list)
  const globalInput = audioInputDeviceId
    ? audioInputs.find((d) => d.id === audioInputDeviceId)
    : (audioInputs.find((d) => d.isDefault) ?? audioInputs[0] ?? null);

  // ── IN encoding ───────────────────────────────────────────────────────────
  const inValue = encodeInputValue(routing);

  // Build IN option list using channel choices from global device
  const inOptions: DawSelectOption[] = (() => {
    if (isMaster) return [{ value: "system-mix", label: "System Mix" }];

    const opts: DawSelectOption[] = [{ value: "none", label: "None" }];

    if (isMidi) {
      // Show enabled MIDI inputs (empty list = all connected inputs enabled)
      const enabledInputs = midiInputs.filter((d) =>
        midiEnabledInputIds.length === 0 || midiEnabledInputIds.includes(d.id)
      );
      opts.push({ value: "midi-all", label: enabledInputs.length > 0 ? "All Enabled Inputs" : "All MIDI Inputs" });
      for (const d of enabledInputs) {
        opts.push({ value: `midi:${d.id}`, label: d.name });
      }
    } else {
      // Audio/instrument/plugin/bus/group/return: channel choices from global device
      if (globalInput) {
        opts.push({ value: "ch:stereo", label: `${globalInput.name} (Stereo)` });
        opts.push({ value: "ch:1", label: "Input 1 (Mono)" });
        opts.push({ value: "ch:2", label: "Input 2 (Mono)" });
      } else {
        opts.push({ value: "ch:stereo", label: "System Input" });
      }
      // Bus/group tracks as sources
      for (const t of project.tracks) {
        if (t.id === trackId) continue;
        if (t.type === "bus" || t.type === "group") {
          opts.push({ value: `bus:${t.id}`, label: t.name });
        }
      }
    }

    return opts;
  })();

  // ── OUT encoding ──────────────────────────────────────────────────────────
  const outValue = track.output ?? "master";

  const hasMissingOutput = (() => {
    if (!outValue || outValue === "master" || outValue === "none") return false;
    return !project.tracks.some((t) => t.id === outValue);
  })();

  const outOptions: DawSelectOption[] = (() => {
    const opts: DawSelectOption[] = [{ value: "master", label: "Master" }];
    // Reachable set for circular routing prevention
    const reachable = buildOutputReachable(project, trackId);
    for (const t of getOutputTargets(project, trackId)) {
      if (t.id === "master") continue;
      // Disable option if selecting it would create a cycle
      const wouldCycle = reachable.has(t.id) && t.id !== trackId;
      opts.push({ value: t.id, label: t.name, disabled: wouldCycle });
    }
    if (audioOutputs.length > 0) {
      for (const d of audioOutputs) {
        opts.push({ value: `hw:${d.id}`, label: d.name, disabled: true });
      }
    }
    opts.push({ value: "none", label: "None" });
    if (hasMissingOutput && !opts.some((o) => o.value === outValue)) {
      opts.unshift({ value: outValue, label: "Missing target" });
    }
    return opts;
  })();

  // ── Handlers ──────────────────────────────────────────────────────────────
  const handleInChange = (value: string) => {
    updateTrackRouting(trackId, decodeInputValue(value));
  };

  const handleOutChange = (value: string) => {
    if (value.startsWith("hw:")) return;
    // Circular routing guard
    if (value !== "master" && value !== "none") {
      const reachable = buildOutputReachable(project, value);
      if (reachable.has(trackId)) return; // would create cycle
    }
    history().execute(new SetTrackOutputCommand(trackId, value, track.output ?? "master"));
    if (value === "master")    updateTrackRouting(trackId, { outputType: "master", outputId: undefined });
    else if (value === "none") updateTrackRouting(trackId, { outputType: "none",   outputId: undefined });
    else                       updateTrackRouting(trackId, { outputType: "bus",    outputId: value });
  };

  // ── Status helpers ────────────────────────────────────────────────────────
  const isWeb = platform.kind === "web";
  const showMidiUnsupported      = isMidi && midiPermission === "unsupported";
  const showMidiPermissionPrompt = isWeb && isMidi && (midiPermission === "unknown" || midiPermission === "prompting");
  const showMidiDenied           = isWeb && isMidi && midiPermission === "denied";
  // For audio: instead of permission prompt in routing section, we show a "no device" hint
  const showNoAudioDevice = !isMidi && !isMaster && !globalInput && audioPermission !== "denied";
  const showAudioDenied   = isWeb && (isAudio) && audioPermission === "denied";

  return (
    <>
      <SectionLabel label="Routing" />
      <div className="flex flex-col gap-1.5 px-3 pb-3">
        {/* IN row */}
        <div className="flex items-center gap-2">
          <PhoneIncoming size={9} className="shrink-0 text-daw-faint" />
          <span className="w-8 shrink-0 text-[9px] text-daw-faint">IN</span>
          {isMaster ? (
            <div
              className="flex h-6 flex-1 cursor-not-allowed items-center rounded px-2 text-[10px] text-daw-faint"
              style={{ background: "rgba(255,255,255,0.025)", border: "1px solid rgba(255,255,255,0.06)" }}
              title="Master track receives from all other tracks"
            >
              All Tracks
            </div>
          ) : (
            <DawSelect className="min-w-0 flex-1" value={inValue} onChange={handleInChange} options={inOptions} />
          )}
        </div>

        {/* Status messages */}
        {showNoAudioDevice && (
          <button
            type="button"
            onClick={() => audioDeviceService.requestAudioPermission()}
            className="ml-[42px] flex h-6 items-center gap-1.5 rounded px-2 text-[10px] text-daw-faint transition-colors hover:bg-white/[0.06] hover:text-daw-text"
            style={{ border: "1px solid rgba(255,255,255,0.08)" }}
          >
            <AlertCircle size={9} className="text-yellow-400" />
            Select device in Settings
          </button>
        )}
        {showAudioDenied && (
          <p className="ml-[42px] text-[9px] text-red-400/80">
            Microphone access denied — check browser settings
          </p>
        )}
        {showMidiUnsupported && (
          <p className="ml-[42px] text-[9px] text-daw-faint opacity-60">
            MIDI unavailable in this browser
          </p>
        )}
        {showMidiPermissionPrompt && (
          <button
            type="button"
            onClick={() => midiDeviceService.requestMidiAccess()}
            className="ml-[42px] flex h-6 items-center gap-1.5 rounded px-2 text-[10px] text-daw-faint transition-colors hover:bg-white/[0.06] hover:text-daw-text"
            style={{ border: "1px solid rgba(255,255,255,0.08)" }}
          >
            <AlertCircle size={9} className="text-yellow-400" />
            Enable MIDI Devices
          </button>
        )}
        {showMidiDenied && (
          <p className="ml-[42px] text-[9px] text-red-400/80">
            MIDI access denied — check browser settings
          </p>
        )}

        {/* OUT row */}
        <div className="flex items-center gap-2">
          <PhoneOutgoing size={9} className="shrink-0 text-daw-faint" />
          <span className="w-8 shrink-0 text-[9px] text-daw-faint">OUT</span>
          <DawSelect
            className="min-w-0 flex-1"
            value={outOptions.some((o) => o.value === outValue) ? outValue : "master"}
            onChange={handleOutChange}
            options={outOptions}
          />
          {hasMissingOutput && (
            <span title="Output track not found">
              <AlertCircle size={9} className="shrink-0 text-orange-400" />
            </span>
          )}
        </div>
      </div>
    </>
  );
}

function TrackAdvancedSection({ trackId }: { trackId: string }) {
  const { project, updateTrackAdvanced } = useProjectStore();
  const track = project.tracks.find((t) => t.id === trackId);
  if (!track) return null;
  const isMidi = track.type === "midi";
  const adv = track.advanced ?? {
    latencyMs: 0, delayMs: 0, semitone: 0, phaseInvert: false, midSideMode: "off" as const,
  };

  // Shared row wrapper keeps all rows the same height/padding.
  const row = "flex h-8 items-center gap-2 border-b border-daw-border px-3";
  const lbl = "w-14 shrink-0 text-[9px] font-semibold uppercase tracking-widest text-daw-faint";
  // Small "wired" / "planned" pill shown at the right of each row.
  const tagWired   = "text-[8px] text-green-400/70 tabular-nums shrink-0";
  const tagPlanned = "text-[8px] text-daw-faint/50 shrink-0";

  const handlePhaseToggle = () => {
    const next = !adv.phaseInvert;
    updateTrackAdvanced(trackId, { phaseInvert: next });
    // Wire immediately into the audio graph.
    mixer.setPhaseInvert(trackId, next);
  };

  return (
    <div className="flex flex-col gap-0 border-b border-daw-border">

      {/* Latency — stored, not applied to engine yet */}
      <div className={row}>
        <span className={lbl}>Latency</span>
        <NumberInput
          className="min-w-0 flex-1"
          min={-500}
          max={500}
          step={1}
          value={adv.latencyMs}
          ariaLabel="Track latency milliseconds"
          onChange={(value) => updateTrackAdvanced(trackId, { latencyMs: value || 0 })}
        />
        <span className="shrink-0 text-[9px] text-daw-faint">ms</span>
        <span className={tagPlanned} title="Stored but not applied to the audio engine yet">not applied</span>
      </div>

      {/* Delay — wired into ClipScheduler */}
      <div className={row}>
        <span className={lbl}>Delay</span>
        <NumberInput
          className="min-w-0 flex-1"
          min={0}
          max={2000}
          step={1}
          value={adv.delayMs}
          ariaLabel="Track delay milliseconds"
          onChange={(value) => {
            updateTrackAdvanced(trackId, { delayMs: value || 0 });
            transport.rescheduleIfPlaying();
          }}
        />
        <span className="shrink-0 text-[9px] text-daw-faint">ms</span>
        <span className={tagWired} title="Applied — shifts clip start later on next play">applied</span>
      </div>

      {/* Semitone — stored, planned */}
      <div className={row}>
        <span className={lbl}>Semitone</span>
        <NumberInput
          className="min-w-0 flex-1"
          min={-48}
          max={48}
          step={1}
          value={adv.semitone}
          ariaLabel="Track semitone offset"
          onChange={(value) => updateTrackAdvanced(trackId, { semitone: value || 0 })}
        />
        <span className="shrink-0 text-[9px] text-daw-faint">st</span>
        <span className={tagPlanned} title="Stored — per-clip pitch transpose is available in the Clip inspector">planned</span>
      </div>

      {/* Phase — wired via mixer GainNode */}
      <div className={row}>
        <span className={lbl}>Phase</span>
        <div className="flex-1" />
        <span className={adv.phaseInvert ? tagWired : tagPlanned}>
          {adv.phaseInvert ? "inverted" : "normal"}
        </span>
        <button
          type="button"
          title={adv.phaseInvert ? "Phase inverted — click to restore" : "Invert phase (×−1 on output GainNode)"}
          onClick={handlePhaseToggle}
          className="flex h-6 w-8 items-center justify-center rounded border text-[9px] font-bold transition-colors"
          style={adv.phaseInvert
            ? { borderColor: "#a99cff", background: "rgba(169,156,255,0.18)", color: "#a99cff" }
            : { borderColor: "rgba(255,255,255,0.08)", background: "rgba(255,255,255,0.02)", color: "rgba(200,212,224,0.5)" }}
        >
          Ø
        </button>
      </div>

      {/* M/S — stored, planned (graph rewrite needed) */}
      <div className={row}>
        <span className={lbl}>M·S</span>
        <DawSelect
          value={adv.midSideMode}
          onChange={(v) => updateTrackAdvanced(trackId, { midSideMode: v as typeof adv.midSideMode })}
          options={[
            { value: "off",        label: "Off" },
            { value: "mid",        label: "Mid (planned)" },
            { value: "side",       label: "Side (planned)" },
            { value: "sum",        label: "Sum (planned)" },
            { value: "difference", label: "Diff (planned)" },
          ]}
        />
        <span className={tagPlanned} title="Stored — mid/side matrix requires a graph rewrite">planned</span>
      </div>

      {/* Monitor mode */}
      <div className={row} style={{ borderBottom: "none" }}>
        <span className={lbl}>Monitor</span>
        <DawSelect
          value={track.monitorMode ?? "off"}
          onChange={(v) => {
            useProjectStore.setState((s) => ({
              project: {
                ...s.project,
                tracks: s.project.tracks.map((t) =>
                  t.id === trackId ? { ...t, monitorMode: v as "off" | "auto" | "in" } : t
                ),
              },
            }));
          }}
          options={[
            { value: "off",  label: "Off" },
            { value: "auto", label: isMidi ? "Auto" : "Auto (planned)" },
            { value: "in",   label: isMidi ? "Input" : "Input (planned)" },
          ]}
        />
      </div>
    </div>
  );
}

// ── MIDI Monitor ──────────────────────────────────────────────────────────────

const MIDI_TYPE_COLOR: Record<MidiEventType, string> = {
  "note-on":  "#7bd88f",
  "note-off": "rgba(180,192,204,0.38)",
  "cc":       "#a99cff",
  "pc":       "#f3c969",
  "pitch":    "#60c0f0",
  "other":    "rgba(180,192,204,0.3)",
};

const MIDI_TYPE_LABEL: Record<MidiEventType, string> = {
  "note-on":  "ON",
  "note-off":  "OFF",
  "cc":       "CC",
  "pc":       "PC",
  "pitch":    "PB",
  "other":    "???",
};

function MidiEventRow({ event }: { event: MidiEventLog }) {
  return (
    <div className="flex items-center gap-2 rounded border border-daw-border bg-daw-bg px-2 py-[3px]">
      <span
        className="w-7 shrink-0 text-center text-[8px] font-bold tabular-nums"
        style={{ color: MIDI_TYPE_COLOR[event.type] }}
      >
        {MIDI_TYPE_LABEL[event.type]}
      </span>
      <span className="w-7 shrink-0 text-[8px] text-daw-faint tabular-nums">
        Ch{event.channel}
      </span>
      <span className="min-w-0 flex-1 truncate text-[9px] text-daw-dim tabular-nums">
        {event.label}
      </span>
    </div>
  );
}

function MidiMonitorSection({ track }: { track: DawTrack }) {
  const { events, isListening, clearEvents } = useMidiInput(track);
  const isArmed   = track.armed ?? false;
  const monitor   = track.monitorMode ?? "off";
  const isMonOn   = monitor === "auto" || monitor === "in";

  return (
    <>
      <SectionLabel label="MIDI Monitor" />
      <div className="px-3 pb-3">
        {/* Status banner */}
        {isListening ? (
          <div className="mb-2 flex items-center justify-between gap-2">
            <div className="flex items-center gap-1.5">
              <Radio size={8} className="text-green-400 animate-pulse" />
              <span className="text-[9px] text-green-400/80">Listening</span>
            </div>
            {events.length > 0 && (
              <button
                type="button"
                onClick={clearEvents}
                className="text-[9px] text-daw-faint transition-colors hover:text-daw-text"
                title="Clear event log"
              >
                Clear
              </button>
            )}
          </div>
        ) : !isArmed ? (
          <p className="text-[10px] text-daw-faint">
            Arm the track to receive MIDI
          </p>
        ) : !isMonOn ? (
          <p className="text-[10px] text-daw-faint">
            Set Monitor to Auto or Input in Advanced
          </p>
        ) : null}

        {/* Event stream */}
        {isListening && events.length === 0 && (
          <p className="text-[9px] text-daw-faint opacity-60">
            Waiting for events…
          </p>
        )}
        {isListening && events.length > 0 && (
          <div className="flex flex-col gap-0.5">
            {events.slice(0, 16).map((ev) => (
              <MidiEventRow key={ev.id} event={ev} />
            ))}
          </div>
        )}
      </div>
    </>
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

type ProcessStatus = "idle" | "realtime" | "cached" | "failed";

const MODE_LABELS: Record<AudioClipProcess["mode"], string> = {
  resample:    "Resample (tape)",
  monophonic:  "Monophonic",
  polyphonic:  "Polyphonic",
  percussive:  "Percussive",
  granular:    "Granular / Texture",
};

function processorLabel(kind: ProcessorKind, mode: AudioClipProcess["mode"]): string {
  switch (kind) {
    case "ts-phase-vocoder": return `Spectral Pitch (${mode})`;
    case "rust-wasm":   return "✓ Rust WASM";
    case "ts-wsola":    return `✓ WSOLA (${mode})`;
    case "ts-granular": return `✓ Granular (${mode})`;
    case "ts-resample": return "✓ Resample";
    default:            return "✓ Ready";
  }
}

function processorKindForParams(params: AudioClipProcess): ProcessorKind {
  if (!params.preservePitch || params.mode === "resample") return "ts-resample";
  if (params.mode === "granular" || params.mode === "percussive") return "ts-granular";
  return params.pitchSemitones !== 0 ? "ts-phase-vocoder" : "ts-wsola";
}

function ClipProcessSection({ clip }: { clip: DawClip }) {
  const history = useHistoryStore.getState;
  const proc = clip.audioProcess ?? DEFAULT_AUDIO_PROCESS;

  // Local draft so slider drags don't spam history until mouseup
  const [draft, setDraft] = useState<AudioClipProcess>(proc);
  const [processStatus, setProcessStatus] = useState<ProcessStatus>("idle");
  const [processorUsed, setProcessorUsed] = useState<ProcessorKind | null>(null);

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

  // Realtime-first: changing pitch/speed never renders the whole file on the UI thread.
  useEffect(() => {
    const hasEffect = proc.speedRatio !== 1 || proc.pitchSemitones !== 0;
    if (!hasEffect) { setProcessStatus("idle"); setProcessorUsed(null); return; }

    const params = {
      speedRatio:     proc.speedRatio,
      pitchSemitones: proc.pitchSemitones,
      preservePitch:  proc.preservePitch,
      mode:           proc.mode ?? "polyphonic",
      quality:        proc.quality,
    };
    const audioBuffer = audioEngine.getBuffer(clip.fileId);
    if (audioBuffer) {
      const key = buildDecodedCacheKey(clip.fileId, audioBuffer.audioBuffer.sampleRate);
      const decoded = audioCacheManager.getDecodedAudio(key);
      const cached = decoded ? audioProcessingService.getCachedProcessed(decoded, params) : null;
      if (cached) {
        setProcessStatus("cached");
        setProcessorUsed(processorKindForParams(params));
      } else {
        setProcessStatus("realtime");
        setProcessorUsed(null);
      }
    } else {
      setProcessStatus("realtime");
      setProcessorUsed(null);
    }
    transport.rescheduleIfPlaying();
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [clip.fileId, proc.speedRatio, proc.pitchSemitones, proc.preservePitch, proc.mode, proc.quality]);

  const isModified = draft.speedRatio !== 1 || draft.pitchSemitones !== 0;

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
          <NumberInput
            className="w-16 !h-5"
            min={0.25}
            max={4}
            step={0.05}
            value={draft.speedRatio}
            ariaLabel="Clip speed ratio"
            onChange={(value) => {
              const v = Math.max(0.25, Math.min(4, value || 1));
              commit({ speedRatio: v });
            }}
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
          <NumberInput
            className="w-16 !h-5"
            min={-24}
            max={24}
            step={1}
            value={draft.pitchSemitones}
            ariaLabel="Clip pitch semitones"
            onChange={(value) => {
              const v = Math.max(-24, Math.min(24, Math.round(value) || 0));
              commit({ pitchSemitones: v });
            }}
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
            {processStatus === "realtime" && "Realtime Preview"}
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
