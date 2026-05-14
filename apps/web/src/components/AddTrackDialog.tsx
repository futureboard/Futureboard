import { CircleDot, CornerDownLeft, Cpu, GitMerge, Mic2, Music, Plus, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useProjectStore } from "../store/projectStore";
import { useUIStore } from "../store/uiStore";
import { useHistoryStore } from "../store/historyStore";
import { AddTrackCommand } from "../commands";
import { TRACK_COLORS } from "../theme";
import type { DawTrack, TrackType } from "../types/daw";

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
    ready: false,
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
    ready: false,
  },
  {
    type: "bus" as TrackType,
    label: "Return Track",
    description: "Receive sends from other tracks",
    detail: "FX Returns · Aux",
    icon: CornerDownLeft,
    ready: false,
  },
];

export function AddTrackDialog({ onClose }: { onClose: () => void }) {
  const tracks = useProjectStore((s) => s.project.tracks);
  const setSelectedTrackId = useUIStore((s) => s.setSelectedTrackId);
  const nextNum = tracks.length + 1;
  const inputRef = useRef<HTMLInputElement>(null);

  const [selectedType, setSelectedType] = useState<TrackTypeConfig>(TRACK_TYPES[0]);
  const [trackName, setTrackName] = useState(`Audio Track ${nextNum}`);
  const [colorIndex, setColorIndex] = useState(() => tracks.length % TRACK_COLORS.length);
  const [trackCount, setTrackCount] = useState(1);
  const [channelCount, setChannelCount] = useState(2);
  const [initialVolume, setInitialVolume] = useState(0.8);
  const [initialPan, setInitialPan] = useState(0);
  const [armTrack, setArmTrack] = useState(false);

  useEffect(() => {
    window.setTimeout(() => inputRef.current?.select(), 0);
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const handleTypeSelect = (cfg: TrackTypeConfig) => {
    setSelectedType(cfg);
    setTrackName(`${cfg.label} ${nextNum}`);
  };

  const createTrack = () => {
    const baseName = trackName.trim() || `${selectedType.label} ${nextNum}`;
    let firstTrackId: string | null = null;

    for (let i = 0; i < trackCount; i++) {
      const id = crypto.randomUUID();
      const trackNumber = nextNum + i;
      const name = trackCount === 1 ? baseName : `${baseName.replace(/\s+\d+$/, "")} ${trackNumber}`;
      const track: DawTrack = {
        id,
        name,
        type: selectedType.type,
        color: TRACK_COLORS[(colorIndex + i) % TRACK_COLORS.length],
        channelCount,
        volume: initialVolume,
        pan: initialPan,
        muted: false,
        solo: false,
        armed: selectedType.type === "audio" ? armTrack : false,
        clips: [],
      };
      useHistoryStore.getState().execute(new AddTrackCommand(track));
      firstTrackId ??= id;
    }

    if (firstTrackId) setSelectedTrackId(firstTrackId);
    onClose();
  };

  const selectedColor = TRACK_COLORS[colorIndex % TRACK_COLORS.length];

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-transparent px-4 pt-[14vh]"
      onMouseDown={onClose}
    >
      <section
        role="dialog"
        aria-modal="true"
        aria-labelledby="add-track-title"
        className="w-full max-w-[520px] overflow-hidden rounded-xl border border-white/[0.08] bg-[#1a1e26] shadow-[0_1px_0_rgba(255,255,255,0.05)_inset,0_0_0_1px_rgba(0,0,0,0.52),0_18px_44px_rgba(0,0,0,0.46),0_44px_120px_rgba(0,0,0,0.42)]"
        onMouseDown={(e) => e.stopPropagation()}
      >
        {/* Header */}
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

        {/* Track type grid */}
        <div className="grid grid-cols-2 gap-2 p-3">
          {TRACK_TYPES.map((cfg) => {
            const Icon = cfg.icon;
            const active = selectedType === cfg;
            return (
              <button
                key={cfg.type}
                type="button"
                onClick={() => handleTypeSelect(cfg)}
                className={[
                  "group relative flex flex-col gap-2 rounded-lg border p-3 text-left transition-all",
                  active
                    ? "border-daw-accent/50 bg-daw-accent/[0.07]"
                    : "border-white/[0.06] bg-[#1f242c] hover:border-white/[0.1] hover:bg-[#232830]",
                ].join(" ")}
              >
                {/* ready / soon badge */}
                <div className="absolute right-2.5 top-2.5">
                  {cfg.ready ? (
                    <span className="rounded px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-wide"
                      style={{ background: "rgba(86,199,201,0.12)", color: "#56C7C9" }}>
                      Ready
                    </span>
                  ) : (
                    <span className="rounded bg-white/[0.05] px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-wide text-daw-faint">
                      Soon
                    </span>
                  )}
                </div>

                {/* Icon */}
                <div
                  className="flex h-8 w-8 items-center justify-center rounded-lg border"
                  style={
                    active
                      ? { background: "rgba(86,199,201,0.12)", borderColor: "rgba(86,199,201,0.3)", color: "#56C7C9" }
                      : { background: "#13161c", borderColor: "rgba(255,255,255,0.07)", color: "#566372" }
                  }
                >
                  <Icon size={15} />
                </div>

                {/* Text */}
                <div>
                  <div className={`text-[12px] font-semibold ${active ? "text-daw-text" : "text-daw-dim"}`}>
                    {cfg.label}
                  </div>
                  <div className="mt-0.5 text-[10px] leading-snug text-daw-faint">
                    {cfg.description}
                  </div>
                  <div className="mt-1.5 text-[9px] font-medium tracking-wide text-daw-faint opacity-60">
                    {cfg.detail}
                  </div>
                </div>
              </button>
            );
          })}
        </div>

        {/* Name input */}
        <div className="border-t border-white/[0.05] px-3 py-2.5">
          <label className="flex h-8 items-center gap-2.5 rounded-lg border bg-[#13161c] px-3 transition-colors focus-within:border-daw-accent/50"
            style={{ borderColor: "rgba(255,255,255,0.07)" }}>
            <selectedType.icon size={13} className="shrink-0 text-daw-faint" />
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

        {/* Track options */}
        <div className="grid grid-cols-2 gap-2 border-t border-white/[0.05] px-3 py-2.5">
          <OptionGroup label="Amount">
            <button
              type="button"
              onClick={() => setTrackCount((v) => Math.max(1, v - 1))}
              className="flex h-7 w-7 items-center justify-center rounded-md border border-white/[0.07] bg-[#13161c] text-[12px] font-semibold text-daw-dim transition-colors hover:bg-white/[0.05] hover:text-daw-text"
            >
              -
            </button>
            <input
              type="number"
              min={1}
              max={32}
              value={trackCount}
              onChange={(event) => setTrackCount(Math.max(1, Math.min(32, Number(event.target.value) || 1)))}
              className="h-7 min-w-0 flex-1 rounded-md border border-white/[0.07] bg-[#13161c] text-center text-[12px] font-semibold tabular-nums text-daw-text outline-none focus:border-daw-accent/50"
            />
            <button
              type="button"
              onClick={() => setTrackCount((v) => Math.min(32, v + 1))}
              className="flex h-7 w-7 items-center justify-center rounded-md border border-white/[0.07] bg-[#13161c] text-[12px] font-semibold text-daw-dim transition-colors hover:bg-white/[0.05] hover:text-daw-text"
            >
              +
            </button>
          </OptionGroup>

          <OptionGroup label="Channels">
            {[1, 2].map((count) => (
              <button
                key={count}
                type="button"
                onClick={() => setChannelCount(count)}
                className={[
                  "h-7 flex-1 rounded-md border px-2 text-[11px] font-semibold transition-colors",
                  channelCount === count
                    ? "border-daw-accent/50 bg-daw-accent/[0.14] text-daw-text"
                    : "border-white/[0.07] bg-[#13161c] text-daw-faint hover:bg-white/[0.05] hover:text-daw-text",
                ].join(" ")}
              >
                {count === 1 ? "Mono" : "Stereo"}
              </button>
            ))}
          </OptionGroup>
        </div>

        {/* Config section: Volume, Pan, Arm, Routing */}
        <div className="grid grid-cols-2 gap-2 border-t border-white/[0.05] px-3 py-2.5">
          <OptionGroup label="Volume">
            <div className="flex w-full items-center gap-2">
              <input
                type="range"
                min={0}
                max={1}
                step={0.01}
                value={initialVolume}
                onChange={(e) => setInitialVolume(parseFloat(e.target.value))}
                className="flex-1 cursor-ew-resize appearance-none"
                style={{ accentColor: selectedColor, height: "3px" }}
              />
              <span className="w-8 shrink-0 text-right text-[10px] tabular-nums text-daw-dim">
                {Math.round(initialVolume * 100)}%
              </span>
            </div>
          </OptionGroup>

          <OptionGroup label="Pan">
            <div className="flex w-full items-center gap-2">
              <input
                type="range"
                min={-1}
                max={1}
                step={0.01}
                value={initialPan}
                onChange={(e) => setInitialPan(parseFloat(e.target.value))}
                className="flex-1 cursor-ew-resize appearance-none"
                style={{ accentColor: "#a99cff", height: "3px" }}
              />
              <span className="w-8 shrink-0 text-right text-[10px] tabular-nums text-daw-dim">
                {initialPan === 0 ? "C" : initialPan < 0 ? `L${Math.round(-initialPan * 100)}` : `R${Math.round(initialPan * 100)}`}
              </span>
            </div>
          </OptionGroup>
        </div>

        {/* Arm + Routing (compact row) */}
        <div className="flex items-center gap-4 border-t border-white/[0.05] px-3 py-2">
          {selectedType.type === "audio" && (
            <label className="flex cursor-pointer items-center gap-2 text-[11px] text-daw-dim">
              <input
                type="checkbox"
                checked={armTrack}
                onChange={(e) => setArmTrack(e.target.checked)}
                className="h-3 w-3 cursor-pointer accent-red-400"
              />
              Arm for recording
            </label>
          )}
          <div className="ml-auto flex items-center gap-2 opacity-50" title="Routing (coming soon)">
            <span className="text-[9px] uppercase tracking-widest text-daw-faint">In</span>
            <div
              className="flex h-5 w-24 cursor-not-allowed items-center rounded px-2 text-[9px] text-daw-faint"
              style={{ background: "rgba(255,255,255,0.025)", border: "1px solid rgba(255,255,255,0.06)" }}
            >
              System Input
            </div>
            <span className="text-[9px] uppercase tracking-widest text-daw-faint">Out</span>
            <div
              className="flex h-5 w-16 cursor-not-allowed items-center rounded px-2 text-[9px] text-daw-faint"
              style={{ background: "rgba(255,255,255,0.025)", border: "1px solid rgba(255,255,255,0.06)" }}
            >
              Master
            </div>
          </div>
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between border-t border-white/[0.05] px-3 py-2.5">
          {/* Color picker */}
          <div className="flex items-center gap-1">
            {TRACK_COLORS.map((color, i) => (
              <button
                key={color}
                type="button"
                title={`Color ${i + 1}`}
                onClick={() => setColorIndex(i)}
                className="relative flex h-5 w-5 items-center justify-center rounded-full transition-transform hover:scale-110"
                style={{ background: i === colorIndex ? color : "transparent", border: `2px solid ${color}`, opacity: i === colorIndex ? 1 : 0.45 }}
              >
                {i === colorIndex && (
                  <CircleDot size={12} className="absolute text-black/60" />
                )}
              </button>
            ))}
          </div>

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
              Add {trackCount === 1 ? "Track" : `${trackCount} Tracks`}
            </button>
          </div>
        </div>
      </section>
    </div>
  );
}

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
