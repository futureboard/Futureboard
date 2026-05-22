import { useEffect, useRef, useState } from "react";
import { ArrowUpDown, Maximize2, RotateCcw, Scissors, VolumeX } from "lucide-react";
import { useUIStore } from "../store/uiStore";
import { useProjectStore } from "../store/projectStore";
import { useHistoryStore } from "../store/historyStore";
import { UpdateClipCommand } from "../commands";
import { WaveformCanvas } from "./timeline/WaveformCanvas";
import type { DawClip, DawTrack } from "../types/daw";
import { clipType } from "../types/daw";
import { MidiEditorPanel } from "./MidiEditorPanel";
import { pickBestLevel } from "../engine/waveformPeakSelector";

export function EditorPanel() {
  const { selectedClipIds } = useUIStore();
  const { project } = useProjectStore();

  const clip =
    selectedClipIds.length === 1
      ? project.tracks.flatMap((t) => t.clips).find((c) => c.id === selectedClipIds[0])
      : null;

  const track = clip
    ? project.tracks.find((t) => t.clips.some((c) => c.id === clip.id))
    : null;

  if (!clip) {
    return (
      <div className="flex min-h-0 flex-1 items-center justify-center">
        <div className="flex flex-col items-center gap-2 text-center">
          <div className="flex h-9 w-9 items-center justify-center rounded-lg border border-white/[0.06] bg-white/[0.025] text-daw-faint">
            <Scissors size={16} />
          </div>
          <p className="text-[12px] font-semibold text-daw-dim">No clip selected</p>
          <p className="max-w-[28ch] text-[11px] leading-relaxed text-daw-faint">
            Select an audio or MIDI clip in the timeline to edit it here.
          </p>
        </div>
      </div>
    );
  }

  const openPianoRollWindow = () => {
    useProjectStore.getState().saveLocal();
    void window.dawElectron?.windows?.openExternal?.({
      contentType: "pianoRoll",
      title: `Piano Roll – ${clip.name}`,
      payload: { clipId: clip.id },
      width: 1100,
      height: 680,
      minWidth: 700,
      minHeight: 420,
    });
  };

  if (clipType(clip) === "midi") return <MidiEditorPanel clip={clip} track={track} onOpenInWindow={openPianoRollWindow} />;
  return <AudioEditor clip={clip} track={track} />;
}

function AudioEditor({ clip, track }: { clip: DawClip; track: DawTrack | null | undefined }) {
  const history = useHistoryStore.getState;
  const peakMeta       = useProjectStore((s) => s.peakMeta);
  const waveformStatus = useProjectStore((s) => s.waveformStatus);
  const files          = useProjectStore((s) => s.project.files);
  const pixelsPerSecond = useUIStore((s) => s.pixelsPerSecond);

  const file     = files.find((f) => f.id === clip.fileId);
  const levelMeta = pickBestLevel(peakMeta, clip.fileId, pixelsPerSecond);
  const status   = waveformStatus.get(clip.fileId) ?? "idle";
  const trackColor = track?.color ?? "#56c7c9";

  const waveRef = useRef<HTMLDivElement>(null);
  const [waveDims, setWaveDims] = useState({ w: 0, h: 0 });

  useEffect(() => {
    const el = waveRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      setWaveDims({ w: el.clientWidth, h: el.clientHeight });
    });
    ro.observe(el);
    setWaveDims({ w: el.clientWidth, h: el.clientHeight });
    return () => ro.disconnect();
  }, []);

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      {/* Clip header */}
      <div
        className="flex h-8 shrink-0 items-center gap-2.5 border-b px-3"
        style={{ borderColor: "rgba(255,255,255,0.06)" }}
      >
        <div className="h-2.5 w-2.5 shrink-0 rounded-full" style={{ background: trackColor }} />
        <span className="truncate text-[12px] font-semibold text-daw-text">{clip.name}</span>
        <span className="shrink-0 text-[10px] text-daw-faint">· Audio Clip</span>
        {file && (
          <span className="ml-auto shrink-0 text-[10px] tabular-nums text-daw-faint">
            {(file.sampleRate / 1000).toFixed(1)}kHz · {file.channels === 1 ? "Mono" : "Stereo"} · {file.duration.toFixed(2)}s
          </span>
        )}
      </div>

      <div className="flex min-h-0 flex-1 overflow-hidden">
        {/* Waveform + ruler */}
        <div className="relative flex min-h-0 min-w-0 flex-1 flex-col">
          <div
            ref={waveRef}
            className="relative flex-1 overflow-hidden"
            style={{ background: "#0d1014" }}
          >
            {waveDims.w > 0 && waveDims.h > 0 && (
              <WaveformCanvas
                fileId={clip.fileId}
                levelMeta={levelMeta}
                width={waveDims.w}
                height={waveDims.h}
                clipOffset={clip.offset}
                clipDuration={clip.duration}
                color={trackColor}
                status={status}
                sourceDuration={file?.duration ?? levelMeta?.duration}
                sampleRate={file?.sampleRate ?? levelMeta?.sampleRate}
              />
            )}
          </div>
          <div
            className="relative flex h-5 shrink-0 overflow-hidden border-t"
            style={{ borderColor: "rgba(255,255,255,0.05)", background: "#0f1318" }}
          >
            {[0, 0.25, 0.5, 0.75, 1.0].map((t) => (
              <span
                key={t}
                className="absolute text-[9px] tabular-nums text-daw-faint"
                style={{ left: `${t * 100}%`, top: "50%", transform: "translate(-50%, -50%)" }}
              >
                {(clip.offset + t * clip.duration).toFixed(2)}s
              </span>
            ))}
          </div>
        </div>

        {/* Right controls panel */}
        <div
          className="flex w-[320px] min-w-[300px] max-w-[360px] shrink-0 flex-col overflow-y-auto overflow-x-hidden border-l"
          style={{ borderColor: "rgba(255,255,255,0.06)", background: "#111418" }}
        >
          <CtrlSection label="Gain">
            <SliderRow
              label="Gain"
              value={clip.gain}
              min={0}
              max={2}
              step={0.01}
              display={`${Math.round(clip.gain * 100)}%`}
              onChange={(v) => history().execute(new UpdateClipCommand(clip.id, { gain: v }, "Set Clip Gain"))}
            />
          </CtrlSection>

          <CtrlSection label="Fades">
            <SliderRow
              label="Fade In"
              value={clip.fadeIn ?? 0}
              min={0}
              max={Math.min(clip.duration, 10)}
              step={0.01}
              display={`${(clip.fadeIn ?? 0).toFixed(2)}s`}
              onChange={(v) => history().execute(new UpdateClipCommand(clip.id, { fadeIn: v }, "Set Fade In"))}
            />
            <SliderRow
              label="Fade Out"
              value={clip.fadeOut ?? 0}
              min={0}
              max={Math.min(clip.duration, 10)}
              step={0.01}
              display={`${(clip.fadeOut ?? 0).toFixed(2)}s`}
              onChange={(v) => history().execute(new UpdateClipCommand(clip.id, { fadeOut: v }, "Set Fade Out"))}
            />
          </CtrlSection>

          <CtrlSection label="Timing">
            <InfoRow label="Start" value={`${clip.startTime.toFixed(3)}s`} />
            <InfoRow label="Duration" value={`${clip.duration.toFixed(3)}s`} />
            <InfoRow label="Offset" value={`${clip.offset.toFixed(3)}s`} />
          </CtrlSection>

          <CtrlSection label="Process">
            <div className="grid grid-cols-2 gap-1.5 px-3 pb-2.5">
              <PlaceholderBtn icon={RotateCcw} label="Reverse" />
              <PlaceholderBtn icon={ArrowUpDown} label="Normalize" />
              <PlaceholderBtn icon={VolumeX} label="Silence" />
              <PlaceholderBtn icon={Maximize2} label="Stretch" />
            </div>
          </CtrlSection>

          {file && (
            <CtrlSection label="Source">
              <InfoRow label="File" value={file.name} truncate />
              <InfoRow label="SR" value={`${file.sampleRate}Hz`} />
              <InfoRow label="Ch" value={file.channels === 1 ? "Mono" : "Stereo"} />
            </CtrlSection>
          )}
        </div>
      </div>
    </div>
  );
}

function CtrlSection({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="border-b" style={{ borderColor: "rgba(255,255,255,0.05)" }}>
      <div className="px-3 pb-1 pt-2.5 text-[9px] font-semibold uppercase tracking-widest text-daw-faint">
        {label}
      </div>
      {children}
    </div>
  );
}

function SliderRow({
  label, value, min, max, step, display, onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  display: string;
  onChange: (v: number) => void;
}) {
  return (
    <div className="flex items-center gap-2.5 px-3 pb-2">
      <span className="w-14 shrink-0 text-[10px] text-daw-faint">{label}</span>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => onChange(parseFloat(e.target.value))}
        className="min-w-0 flex-1 cursor-ew-resize appearance-none"
        style={{ accentColor: "#56c7c9", height: "3px" }}
      />
      <span className="w-11 shrink-0 text-right text-[10px] tabular-nums text-daw-dim">
        {display}
      </span>
    </div>
  );
}

function InfoRow({ label, value, truncate }: { label: string; value: string; truncate?: boolean }) {
  return (
    <div className="flex items-center gap-2.5 px-3 pb-1.5">
      <span className="w-14 shrink-0 text-[10px] text-daw-faint">{label}</span>
      <span
        className="text-[10px] tabular-nums text-daw-dim"
        style={truncate ? { minWidth: 0, flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" } : undefined}
        title={truncate ? value : undefined}
      >
        {value}
      </span>
    </div>
  );
}

function PlaceholderBtn({ icon: Icon, label }: { icon: React.ElementType; label: string }) {
  return (
    <button
      type="button"
      disabled
      title={`${label} (coming soon)`}
      className="flex h-7 cursor-not-allowed items-center justify-center gap-1 rounded border border-dashed text-[9px] opacity-40"
      style={{
        borderColor: "rgba(255,255,255,0.12)",
        background: "rgba(255,255,255,0.02)",
        color: "rgba(180,192,204,0.6)",
      }}
    >
      <Icon size={9} />
      {label}
    </button>
  );
}
