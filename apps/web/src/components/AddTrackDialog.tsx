import {
  AlertCircle,
  CircleDot,
  CornerDownLeft,
  Cpu,
  GitFork,
  GitMerge,
  Mic2,
  Music,
  Plus,
  Volume2,
  X,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useProjectStore } from "../store/projectStore";
import { useUIStore } from "../store/uiStore";
import { useHistoryStore } from "../store/historyStore";
import { AddTrackCommand } from "../commands";
import { TRACK_COLORS } from "../theme";
import type { DawTrack, TrackInputType, TrackType } from "../types/daw";
import { DawSelect } from "./ui/DawSelect";
import type { DawSelectOption } from "./ui/DawSelect";
import { useDeviceStore } from "../store/deviceStore";
import { useAudioSettingsStore } from "../store/audioSettingsStore";
import { midiDeviceService } from "../engine/MidiDeviceService";
import { platform } from "../platform";

// ── Track type catalogue ──────────────────────────────────────────────────────

type TrackTypeConfig = {
  type: TrackType;
  label: string;
  description: string;
  detail: string;
  icon: React.ElementType;
  ready: boolean;
};

const TRACK_TYPES: TrackTypeConfig[] = [
  {
    type: "audio",
    label: "Audio Track",
    description: "Record and arrange audio clips",
    detail: "WAV · MP3 · AIFF",
    icon: Mic2,
    ready: true,
  },
  {
    type: "midi",
    label: "MIDI Track",
    description: "Sequence instruments with notes",
    detail: "Piano Roll · CC",
    icon: Music,
    ready: true,
  },
  {
    type: "plugin",
    label: "Plugin Track",
    description: "Host virtual instruments & effects",
    detail: "VST3 · AU · CLAP",
    icon: Cpu,
    ready: false,
  },
  {
    type: "bus",
    label: "Bus Track",
    description: "Route and blend multiple channels",
    detail: "Sends · Groups",
    icon: GitMerge,
    ready: true,
  },
  {
    type: "return",
    label: "Return Track",
    description: "Receive sends from other tracks",
    detail: "FX Returns · Aux",
    icon: CornerDownLeft,
    ready: true,
  },
  {
    type: "group",
    label: "Group Track",
    description: "Group and process multiple tracks",
    detail: "Sub-mix · Stem",
    icon: GitFork,
    ready: true,
  },
  {
    type: "master",
    label: "Master Track",
    description: "Final output and master bus",
    detail: "Main Output",
    icon: Volume2,
    ready: true,
  },
];

// ── Input/output value encoding (mirrors InspectorPanel) ─────────────────────

// "none" | "ch:stereo" | "ch:1" | "ch:2" | "midi-all" | "midi:{id}"
type InputValue = string;

function defaultInputForType(type: TrackType): InputValue {
  if (type === "midi") return "midi-all";
  if (type === "audio") return "ch:stereo";
  return "none";
}

// ── Summary text ──────────────────────────────────────────────────────────────

function buildSummary(
  cfg: TrackTypeConfig,
  count: number,
  channelCount: number,
  inputValue: InputValue,
  outputId: string,
  midiChannel: number | "all",
  monitorMode: "off" | "auto" | "in",
  allTracks: DawTrack[],
  midiInputs: { id: string; name: string }[],
): string {
  const n = count === 1 ? "" : `${count} `;
  const plural = count > 1 ? "s" : "";
  const outLabel =
    outputId === "master" || outputId === "none"
      ? "Master"
      : (allTracks.find((t) => t.id === outputId)?.name ?? "Bus");

  if (cfg.type === "audio") {
    const ch = channelCount === 1 ? "mono" : "stereo";
    const inLabel = inputValue === "ch:stereo" ? "stereo in" : inputValue.startsWith("ch:") ? `mono ch ${inputValue.slice(3)}` : "no input";
    const mon = monitorMode !== "off" ? ` · Mon ${monitorMode}` : "";
    return `Add ${n}${ch} audio track${plural} · ${inLabel} → ${outLabel}${mon}`;
  }
  if (cfg.type === "midi") {
    const inLabel =
      inputValue === "midi-all"
        ? "All Enabled Inputs"
        : inputValue === "none"
        ? "No Input"
        : inputValue.startsWith("midi:")
        ? (midiInputs.find((d) => `midi:${d.id}` === inputValue)?.name ?? "MIDI Device")
        : inputValue;
    const chLabel = midiChannel === "all" ? "all channels" : `Ch ${midiChannel}`;
    return `Add ${n}MIDI track${plural} — ${inLabel}, ${chLabel}`;
  }
  if (cfg.type === "master") return "Add master output track";
  if (cfg.type === "bus")    return `Add ${n}bus track${plural} → ${outLabel}`;
  if (cfg.type === "return") return `Add ${n}return track${plural} → ${outLabel}`;
  if (cfg.type === "group")  return `Add ${n}group track${plural} → ${outLabel}`;
  return `Add ${n}track${plural}`;
}

// ── Dialog ────────────────────────────────────────────────────────────────────

export function AddTrackDialog({ onClose }: { onClose: () => void }) {
  const tracks        = useProjectStore((s) => s.project.tracks);
  const setSelectedTrackId = useUIStore((s) => s.setSelectedTrackId);
  const { audioInputs, midiInputs, midiPermission } = useDeviceStore();
  const { audioInputDeviceId, midiEnabledInputIds } = useAudioSettingsStore();
  const nextNum       = tracks.length + 1;
  const inputRef      = useRef<HTMLInputElement>(null);
  const hasMaster     = tracks.some((t) => t.type === "master");

  // ── Local state ─────────────────────────────────────────────────────────────
  const [selectedType, setSelectedType] = useState<TrackTypeConfig>(TRACK_TYPES[0]!);
  const [trackName,    setTrackName]    = useState(`Audio Track ${nextNum}`);
  const [colorIndex,   setColorIndex]   = useState(() => tracks.length % TRACK_COLORS.length);
  const [trackCount,   setTrackCount]   = useState(1);
  const [channelCount, setChannelCount] = useState(2); // 1=mono, 2=stereo
  const [volume,       setVolume]       = useState(0.8);
  const [pan,          setPan]          = useState(0);
  const [armTrack,     setArmTrack]     = useState(false);

  // Routing
  const [inputValue,   setInputValue]   = useState<InputValue>("system-audio");
  const [outputId,     setOutputId]     = useState("master");
  const [monitorMode,  setMonitorMode]  = useState<"off" | "auto" | "in">("off");
  const [midiChannel,  setMidiChannel]  = useState<number | "all">("all");

  // ── Init & keyboard ──────────────────────────────────────────────────────────
  useEffect(() => { window.setTimeout(() => inputRef.current?.select(), 0); }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  // ── Type selection ───────────────────────────────────────────────────────────
  const handleTypeSelect = (cfg: TrackTypeConfig) => {
    if (cfg.type === "master" && hasMaster) return; // only one master allowed
    setSelectedType(cfg);
    setTrackName(`${cfg.label} ${nextNum}`);
    setInputValue(defaultInputForType(cfg.type));
    setOutputId(cfg.type === "midi" ? "none" : "master");
    setMonitorMode("off");
    setMidiChannel("all");
    setArmTrack(false);
  };

  // ── Input select options: channel-based (mirrors InspectorPanel) ─────────────
  const globalInput = audioInputDeviceId
    ? audioInputs.find((d) => d.id === audioInputDeviceId)
    : (audioInputs.find((d) => d.isDefault) ?? audioInputs[0] ?? null);

  const audioInputOptions: DawSelectOption[] = [
    ...(globalInput
      ? [
          { value: "ch:stereo", label: `${globalInput.name} (Stereo)` },
          { value: "ch:1", label: "Input 1 (Mono)" },
          { value: "ch:2", label: "Input 2 (Mono)" },
        ]
      : [{ value: "ch:stereo", label: "System Input" }]),
    { value: "none", label: "None" },
  ];

  const enabledMidiInputs = midiInputs.filter((d) =>
    midiEnabledInputIds.length === 0 || midiEnabledInputIds.includes(d.id)
  );
  const midiInputOptions: DawSelectOption[] = [
    { value: "midi-all", label: enabledMidiInputs.length > 0 ? "All Enabled Inputs" : "All MIDI Inputs" },
    ...enabledMidiInputs.map((d) => ({ value: `midi:${d.id}`, label: d.name })),
    { value: "none", label: "None" },
  ];

  const midiChannelOptions: DawSelectOption[] = [
    { value: "all", label: "All Channels" },
    ...Array.from({ length: 16 }, (_, i) => ({
      value: String(i + 1),
      label: `Channel ${i + 1}`,
    })),
  ];

  const outputOptions: DawSelectOption[] = [
    { value: "master", label: "Master" },
    ...tracks
      .filter((t) => t.type === "bus" || t.type === "group")
      .map((t) => ({ value: t.id, label: t.name })),
  ];

  // ── Create tracks ────────────────────────────────────────────────────────────
  const createTrack = () => {
    const baseName = trackName.trim() || `${selectedType.label} ${nextNum}`;
    let firstId: string | null = null;

    // Decode inputValue → routing fields (channel-based)
    let finalInputType: TrackInputType = "none";
    let finalInputId: string | undefined;
    let finalInputChannel: number | "stereo" | undefined;

    if (inputValue === "none") {
      finalInputType = "none";
    } else if (inputValue === "ch:stereo") {
      finalInputType = "audio-channel"; finalInputChannel = "stereo";
    } else if (inputValue.startsWith("ch:")) {
      finalInputType = "audio-channel";
      const n = parseInt(inputValue.slice(3), 10);
      finalInputChannel = isNaN(n) ? "stereo" : n;
    } else if (inputValue === "midi-all") {
      finalInputType = "midi-device";
    } else if (inputValue.startsWith("midi:")) {
      finalInputType = "midi-device"; finalInputId = inputValue.slice(5);
    } else {
      finalInputType = "none";
    }

    const midiChannelValue: number | "stereo" | undefined =
      selectedType.type === "midi" && midiChannel !== "all" ? midiChannel : undefined;

    for (let i = 0; i < trackCount; i++) {
      const id = crypto.randomUUID();
      const n  = trackCount === 1
        ? baseName
        : `${baseName.replace(/\s+\d+$/, "")} ${nextNum + i}`;

      const track: DawTrack = {
        id,
        name: n,
        type: selectedType.type,
        color: TRACK_COLORS[(colorIndex + i) % TRACK_COLORS.length]!,
        channelCount,
        channelMode: channelCount === 1 ? "mono" : "stereo",
        volume,
        pan,
        muted: false,
        solo:  false,
        armed: selectedType.type === "audio" ? armTrack : false,
        clips: [],
        sends: [],
        inserts: [],
        output: outputId !== "none" ? outputId : undefined,
        routing: {
          inputType:    finalInputType,
          inputId:      finalInputId,
          inputChannel: finalInputChannel ?? midiChannelValue,
          outputType:   outputId === "master" ? "master" : outputId === "none" ? "none" : "bus",
          outputId:     outputId !== "master" && outputId !== "none" ? outputId : undefined,
        },
        advanced: {
          latencyMs: 0, delayMs: 0, semitone: 0, phaseInvert: false, midSideMode: "off",
        },
        monitorMode,
      };

      useHistoryStore.getState().execute(new AddTrackCommand(track));
      firstId ??= id;
    }

    if (firstId) setSelectedTrackId(firstId);
    onClose();
  };

  // ── Derived UI values ────────────────────────────────────────────────────────
  const selectedColor = TRACK_COLORS[colorIndex % TRACK_COLORS.length]!;
  const isAudio  = selectedType.type === "audio";
  const isMidi   = selectedType.type === "midi";
  const isMaster = selectedType.type === "master";
  const showChannels = !isMidi && !isMaster;
  const showOutput   = !isMidi && !isMaster;

  // Permission prompts only shown in web — Electron auto-initialises devices.
  const isWeb = platform.kind === "web";
  const needsMidiPerm   = isWeb && isMidi && midiPermission !== "granted" && midiPermission !== "unsupported";
  const midiUnsupported = isMidi && midiPermission === "unsupported";

  const summary = buildSummary(
    selectedType, trackCount, channelCount, inputValue, outputId,
    midiChannel, monitorMode, tracks, midiInputs,
  );

  // ── Render ───────────────────────────────────────────────────────────────────
  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-transparent px-4 pt-[14vh]"
      onMouseDown={onClose}
    >
      <section
        role="dialog"
        aria-modal="true"
        aria-labelledby="add-track-title"
        className="w-full max-w-[540px] overflow-hidden rounded-xl border border-white/[0.08] bg-[#1a1e26] shadow-[0_1px_0_rgba(255,255,255,0.05)_inset,0_0_0_1px_rgba(0,0,0,0.52),0_18px_44px_rgba(0,0,0,0.46),0_44px_120px_rgba(0,0,0,0.42)]"
        onMouseDown={(e) => e.stopPropagation()}
      >
        {/* ── Header ── */}
        <div className="flex h-10 items-center justify-between border-b border-white/[0.06] px-4">
          <div className="flex items-center gap-2">
            <Plus size={13} className="text-daw-accent" />
            <h2 id="add-track-title" className="text-[12px] font-semibold text-daw-text">
              New Track
            </h2>
          </div>
          <button
            onClick={onClose}
            className="flex h-6 w-6 items-center justify-center rounded-md text-daw-faint transition-colors hover:bg-white/[0.06] hover:text-daw-text"
          >
            <X size={13} />
          </button>
        </div>

        {/* ── Track type grid ── */}
        <div className="grid grid-cols-4 gap-1.5 p-3">
          {TRACK_TYPES.map((cfg) => {
            const Icon    = cfg.icon;
            const active  = selectedType === cfg;
            const blocked = cfg.type === "master" && hasMaster;
            return (
              <button
                key={cfg.type}
                type="button"
                disabled={blocked}
                onClick={() => handleTypeSelect(cfg)}
                className={[
                  "group relative flex flex-col gap-1.5 rounded-lg border p-2.5 text-left transition-all",
                  blocked ? "cursor-not-allowed opacity-40" :
                  active
                    ? "border-daw-accent/50 bg-daw-accent/[0.07]"
                    : "border-white/[0.06] bg-[#1f242c] hover:border-white/[0.1] hover:bg-[#232830]",
                ].join(" ")}
              >
                {/* badge */}
                <div className="absolute right-2 top-2">
                  {blocked ? (
                    <span className="rounded bg-white/[0.05] px-1 py-0.5 text-[8px] font-semibold uppercase tracking-wide text-daw-faint">
                      Exists
                    </span>
                  ) : cfg.ready ? (
                    <span
                      className="rounded px-1 py-0.5 text-[8px] font-semibold uppercase tracking-wide"
                      style={{ background: "rgba(86,199,201,0.12)", color: "#56C7C9" }}
                    >
                      ✓
                    </span>
                  ) : (
                    <span className="rounded bg-white/[0.05] px-1 py-0.5 text-[8px] font-semibold uppercase tracking-wide text-daw-faint">
                      Soon
                    </span>
                  )}
                </div>

                {/* icon */}
                <div
                  className="flex h-7 w-7 items-center justify-center rounded-lg border"
                  style={
                    active
                      ? { background: "rgba(86,199,201,0.12)", borderColor: "rgba(86,199,201,0.3)", color: "#56C7C9" }
                      : { background: "#13161c", borderColor: "rgba(255,255,255,0.07)", color: "#566372" }
                  }
                >
                  <Icon size={13} />
                </div>

                {/* text */}
                <div>
                  <div className={`text-[11px] font-semibold leading-tight ${active ? "text-daw-text" : "text-daw-dim"}`}>
                    {cfg.label}
                  </div>
                  <div className="mt-0.5 text-[9px] leading-snug text-daw-faint opacity-70">
                    {cfg.detail}
                  </div>
                </div>
              </button>
            );
          })}
        </div>

        {/* ── Name input ── */}
        <div className="border-t border-white/[0.05] px-3 py-2">
          <label
            className="flex h-8 items-center gap-2.5 rounded-lg border bg-[#13161c] px-3 transition-colors focus-within:border-daw-accent/50"
            style={{ borderColor: "rgba(255,255,255,0.07)" }}
          >
            <selectedType.icon size={12} className="shrink-0 text-daw-faint" />
            <input
              ref={inputRef}
              value={trackName}
              onChange={(e) => setTrackName(e.target.value)}
              onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); createTrack(); } }}
              placeholder="Track name"
              className="min-w-0 flex-1 bg-transparent text-[12px] font-medium text-daw-text outline-none placeholder:text-daw-faint"
            />
          </label>
        </div>

        {/* ── Amount + Channels + Vol/Pan ── */}
        <div className="grid grid-cols-2 gap-2 border-t border-white/[0.05] px-3 py-2">
          <OptionGroup label="Amount">
            <SpinnerInput
              value={trackCount}
              min={1}
              max={32}
              onChange={setTrackCount}
            />
          </OptionGroup>

          {showChannels ? (
            <OptionGroup label="Channels">
              {[1, 2].map((c) => (
                <PillButton
                  key={c}
                  active={channelCount === c}
                  onClick={() => setChannelCount(c)}
                >
                  {c === 1 ? "Mono" : "Stereo"}
                </PillButton>
              ))}
            </OptionGroup>
          ) : (
            <div /> /* spacer keeps grid balanced */
          )}
        </div>

        <div className="grid grid-cols-2 gap-2 border-t border-white/[0.05] px-3 py-2">
          <OptionGroup label="Volume">
            <SliderField
              value={volume}
              min={0}
              max={1}
              step={0.01}
              color={selectedColor}
              display={`${Math.round(volume * 100)}%`}
              onChange={setVolume}
            />
          </OptionGroup>
          <OptionGroup label="Pan">
            <SliderField
              value={pan}
              min={-1}
              max={1}
              step={0.01}
              color="#a99cff"
              display={pan === 0 ? "C" : pan < 0 ? `L${Math.round(-pan * 100)}` : `R${Math.round(pan * 100)}`}
              onChange={setPan}
            />
          </OptionGroup>
        </div>

        {/* ── Type-aware routing ── */}
        {!isMaster && (
          <div className="flex flex-col gap-1.5 border-t border-white/[0.05] px-3 py-2.5">
            {/* Audio routing */}
            {isAudio && (
              <>
                <RoutingRow label="Monitor">
                  <div className="flex gap-1">
                    {(["off", "auto", "in"] as const).map((m) => (
                      <PillButton key={m} active={monitorMode === m} onClick={() => setMonitorMode(m)}>
                        {m === "off" ? "Off" : m === "auto" ? "Auto" : "In"}
                      </PillButton>
                    ))}
                  </div>
                </RoutingRow>
                <RoutingRow label="Input">
                  <DawSelect
                    value={inputValue}
                    onChange={setInputValue}
                    options={audioInputOptions}
                  />
                </RoutingRow>
              </>
            )}

            {/* MIDI routing */}
            {isMidi && (
              <>
                {midiUnsupported ? (
                  <p className="text-[9px] text-daw-faint opacity-60">
                    MIDI is unavailable in this browser
                  </p>
                ) : (
                  <>
                    <RoutingRow label="Input">
                      <div className="flex flex-1 items-center gap-2">
                        <div className="flex-1">
                          <DawSelect
                            value={inputValue}
                            onChange={setInputValue}
                            options={midiInputOptions}
                          />
                        </div>
                        {needsMidiPerm && (
                          <button
                            type="button"
                            onClick={() => midiDeviceService.requestMidiAccess()}
                            className="flex shrink-0 items-center gap-1 rounded px-1.5 py-1 text-[9px] text-yellow-400/80 transition-colors hover:text-yellow-300"
                            style={{ border: "1px solid rgba(234,179,8,0.2)" }}
                            title="Grant MIDI access to see connected devices"
                          >
                            <AlertCircle size={8} />
                            Enable
                          </button>
                        )}
                      </div>
                    </RoutingRow>
                    <RoutingRow label="Channel">
                      <DawSelect
                        value={midiChannel === "all" ? "all" : String(midiChannel)}
                        onChange={(v) => setMidiChannel(v === "all" ? "all" : Number(v))}
                        options={midiChannelOptions}
                      />
                    </RoutingRow>
                  </>
                )}
              </>
            )}

            {/* Output row — audio, bus, return, group */}
            {showOutput && (
              <RoutingRow label="Output">
                <DawSelect
                  value={outputId}
                  onChange={setOutputId}
                  options={outputOptions}
                />
              </RoutingRow>
            )}

            {/* Arm — audio tracks only */}
            {isAudio && (
              <label className="mt-0.5 flex cursor-pointer items-center gap-2 text-[11px] text-daw-dim">
                <input
                  type="checkbox"
                  checked={armTrack}
                  onChange={(e) => setArmTrack(e.target.checked)}
                  className="h-3 w-3 cursor-pointer accent-red-400"
                />
                Arm for recording
              </label>
            )}

            {/* Bus-like: just output already shown above, nothing else needed */}
          </div>
        )}

        {/* ── Footer ── */}
        <div className="flex flex-col gap-2 border-t border-white/[0.05] px-3 py-2.5">
          {/* Summary */}
          <p className="text-[10px] text-daw-faint opacity-70">{summary}</p>

          <div className="flex items-center justify-between gap-3">
            {/* Color picker */}
            <div className="flex items-center gap-1">
              {TRACK_COLORS.map((color, i) => (
                <button
                  key={color}
                  type="button"
                  title={`Color ${i + 1}`}
                  onClick={() => setColorIndex(i)}
                  className="relative flex h-5 w-5 items-center justify-center rounded-full transition-transform hover:scale-110"
                  style={{
                    background: i === colorIndex ? color : "transparent",
                    border: `2px solid ${color}`,
                    opacity: i === colorIndex ? 1 : 0.45,
                  }}
                >
                  {i === colorIndex && <CircleDot size={12} className="absolute text-black/60" />}
                </button>
              ))}
            </div>

            {/* Actions */}
            <div className="flex items-center gap-2">
              <button
                type="button"
                onClick={onClose}
                className="h-7 rounded-md border border-white/[0.07] bg-transparent px-3 text-[11px] font-medium text-daw-faint transition-colors hover:bg-white/[0.05] hover:text-daw-text"
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={createTrack}
                className="flex h-7 items-center gap-1.5 rounded-md px-3 text-[11px] font-semibold text-daw-ink transition-colors"
                style={{ background: selectedColor }}
              >
                <Plus size={12} />
                {trackCount === 1 ? "Add Track" : `Add ${trackCount} Tracks`}
              </button>
            </div>
          </div>
        </div>
      </section>
    </div>
  );
}

// ── Sub-components ────────────────────────────────────────────────────────────

function OptionGroup({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <div className="mb-1.5 text-[9px] font-semibold uppercase tracking-wide text-daw-faint">
        {label}
      </div>
      <div className="flex items-center gap-1.5">{children}</div>
    </div>
  );
}

function RoutingRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex items-center gap-3">
      <span className="w-14 shrink-0 text-[9px] font-semibold uppercase tracking-wide text-daw-faint">
        {label}
      </span>
      <div className="flex flex-1 items-center gap-2">{children}</div>
    </div>
  );
}

function PillButton({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={[
        "h-7 flex-1 rounded-md border px-2 text-[11px] font-semibold transition-colors",
        active
          ? "border-daw-accent/50 bg-daw-accent/[0.14] text-daw-text"
          : "border-white/[0.07] bg-[#13161c] text-daw-faint hover:bg-white/[0.05] hover:text-daw-text",
      ].join(" ")}
    >
      {children}
    </button>
  );
}

function SpinnerInput({
  value,
  min,
  max,
  onChange,
}: {
  value: number;
  min: number;
  max: number;
  onChange: (v: number) => void;
}) {
  return (
    <>
      <button
        type="button"
        onClick={() => onChange(Math.max(min, value - 1))}
        className="flex h-7 w-7 items-center justify-center rounded-md border border-white/[0.07] bg-[#13161c] text-[12px] font-semibold text-daw-dim transition-colors hover:bg-white/[0.05] hover:text-daw-text"
      >
        −
      </button>
      <input
        type="number"
        min={min}
        max={max}
        value={value}
        onChange={(e) => onChange(Math.max(min, Math.min(max, Number(e.target.value) || 1)))}
        className="h-7 min-w-0 flex-1 rounded-md border border-white/[0.07] bg-[#13161c] text-center text-[12px] font-semibold tabular-nums text-daw-text outline-none focus:border-daw-accent/50"
      />
      <button
        type="button"
        onClick={() => onChange(Math.min(max, value + 1))}
        className="flex h-7 w-7 items-center justify-center rounded-md border border-white/[0.07] bg-[#13161c] text-[12px] font-semibold text-daw-dim transition-colors hover:bg-white/[0.05] hover:text-daw-text"
      >
        +
      </button>
    </>
  );
}

function SliderField({
  value,
  min,
  max,
  step,
  color,
  display,
  onChange,
}: {
  value: number;
  min: number;
  max: number;
  step: number;
  color: string;
  display: string;
  onChange: (v: number) => void;
}) {
  return (
    <div className="flex w-full items-center gap-2">
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => onChange(parseFloat(e.target.value))}
        className="flex-1 cursor-ew-resize appearance-none"
        style={{ accentColor: color, height: "3px" }}
      />
      <span className="w-8 shrink-0 text-right text-[10px] tabular-nums text-daw-dim">
        {display}
      </span>
    </div>
  );
}
