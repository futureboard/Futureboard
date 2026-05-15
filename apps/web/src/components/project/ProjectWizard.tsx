import { useRef, useState } from "react";
import {
  FileText, Mic2, Music, SlidersHorizontal, Square, FolderOpen, Plus,
} from "lucide-react";
import { useProjectStore } from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
import { useHistoryStore } from "../../store/historyStore";
import { useWindowStore } from "../../store/windowStore";
import { useRecentProjectsStore } from "../../store/recentProjectsStore";
import { getTrackColor } from "../../theme";
import type { DawTrack } from "../../types/daw";

type Props = { windowId: string };

type Template = "empty" | "recording" | "beat-making" | "mixing" | "scoring";

type WizardState = {
  name: string;
  bpm: number;
  timeSignatureNumerator: number;
  timeSignatureDenominator: number;
  sampleRate: number;
  template: Template;
  audioTrackCount: number;
  midiTrackCount: number;
};

const TEMPLATE_PRESETS: Record<Template, Partial<WizardState>> = {
  empty:        { audioTrackCount: 0, midiTrackCount: 0 },
  recording:    { audioTrackCount: 4, midiTrackCount: 0, bpm: 120 },
  "beat-making":{ audioTrackCount: 0, midiTrackCount: 4, bpm: 140 },
  mixing:       { audioTrackCount: 8, midiTrackCount: 0 },
  scoring:      { audioTrackCount: 0, midiTrackCount: 8, timeSignatureNumerator: 4 },
};

const TEMPLATES: { id: Template; label: string; icon: React.ElementType; detail: string }[] = [
  { id: "empty",       label: "Empty",      icon: Square,           detail: "Blank canvas"   },
  { id: "recording",   label: "Recording",  icon: Mic2,             detail: "4 audio tracks" },
  { id: "beat-making", label: "Beat Making",icon: Music,            detail: "4 MIDI tracks"  },
  { id: "mixing",      label: "Mixing",     icon: SlidersHorizontal,detail: "8 audio tracks" },
  { id: "scoring",     label: "Scoring",    icon: FileText,         detail: "8 MIDI tracks"  },
];

const SAMPLE_RATES = [44100, 48000, 88200, 96000] as const;
const SR_LABEL: Record<number, string> = { 44100: "44.1k", 48000: "48k", 88200: "88.2k", 96000: "96k" };

// ─── Shared sub-components ────────────────────────────────────────────────────

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

function Stepper({
  value, min, max, onChange,
}: { value: number; min: number; max: number; onChange: (v: number) => void }) {
  return (
    <>
      <button
        type="button"
        onClick={() => onChange(Math.max(min, value - 1))}
        className="flex h-7 w-7 items-center justify-center rounded-md border border-white/[0.07] bg-[#13161c] text-[12px] font-semibold text-daw-dim transition-colors hover:bg-white/[0.05] hover:text-daw-text"
      >
        <span className="select-none leading-none">−</span>
      </button>
      <input
        type="number"
        min={min}
        max={max}
        value={value}
        onChange={(e) => onChange(Math.max(min, Math.min(max, Number(e.target.value) || min)))}
        className="h-7 min-w-0 flex-1 rounded-md border border-white/[0.07] bg-[#13161c] text-center text-[12px] font-semibold tabular-nums text-daw-text outline-none focus:border-daw-accent/50"
      />
      <button
        type="button"
        onClick={() => onChange(Math.min(max, value + 1))}
        className="flex h-7 w-7 items-center justify-center rounded-md border border-white/[0.07] bg-[#13161c] text-[12px] font-semibold text-daw-dim transition-colors hover:bg-white/[0.05] hover:text-daw-text"
      >
        <span className="select-none leading-none">+</span>
      </button>
    </>
  );
}

// ─── Main component ───────────────────────────────────────────────────────────

export function ProjectWizard({ windowId }: Props) {
  const nameRef = useRef<HTMLInputElement>(null);

  const [state, setState] = useState<WizardState>({
    name: "Untitled Project",
    bpm: 120,
    timeSignatureNumerator: 4,
    timeSignatureDenominator: 4,
    sampleRate: 48000,
    template: "empty",
    audioTrackCount: 0,
    midiTrackCount: 0,
  });

  const set = (patch: Partial<WizardState>) => setState((s) => ({ ...s, ...patch }));

  const applyTemplate = (t: Template) => set({ template: t, ...TEMPLATE_PRESETS[t] });

  const handleCreate = () => {
    const history  = useHistoryStore.getState();
    const uiStore  = useUIStore.getState();
    const ws       = useWindowStore.getState();

    const tracks: DawTrack[] = [];

    for (let i = 0; i < state.audioTrackCount; i++) {
      tracks.push({
        id: crypto.randomUUID(),
        name: `Audio ${i + 1}`,
        type: "audio",
        color: getTrackColor(i),
        channelCount: 2,
        volume: 0.8,
        pan: 0,
        muted: false,
        solo: false,
        armed: false,
        clips: [],
      });
    }

    for (let i = 0; i < state.midiTrackCount; i++) {
      tracks.push({
        id: crypto.randomUUID(),
        name: `MIDI ${i + 1}`,
        type: "midi",
        color: getTrackColor(state.audioTrackCount + i),
        channelCount: 2,
        volume: 0.8,
        pan: 0,
        muted: false,
        solo: false,
        armed: false,
        clips: [],
      });
    }

    useProjectStore.setState({
      project: {
        id: crypto.randomUUID(),
        name: state.name.trim() || "Untitled Project",
        version: 1,
        sampleRate: state.sampleRate,
        bpm: state.bpm,
        timeSignature: {
          numerator: state.timeSignatureNumerator,
          denominator: state.timeSignatureDenominator,
        },
        tracks,
        files: [],
      },
    });

    history.clear();
    uiStore.setSelectedClipIds([]);
    uiStore.setSelectedTrackId(null);
    uiStore.setSaveStatus("saved");

    const { project } = useProjectStore.getState();
    useRecentProjectsStore.getState().addRecentProject({
      id: project.id,
      name: project.name,
      source: "browser",
    });

    ws.closeWindow(windowId);
  };

  const totalTracks = state.audioTrackCount + state.midiTrackCount;
  const templateLabel = TEMPLATES.find((t) => t.id === state.template)?.label ?? state.template;

  return (
    <div className="flex flex-col">

      {/* ── Project name ── */}
      <div className="px-3 py-2.5">
        <label
          className="flex h-8 items-center gap-2.5 rounded-lg border bg-[#13161c] px-3 transition-colors focus-within:border-daw-accent/50"
          style={{ borderColor: "rgba(255,255,255,0.07)" }}
        >
          <FolderOpen size={13} className="shrink-0 text-daw-faint" />
          <input
            ref={nameRef}
            // eslint-disable-next-line jsx-a11y/no-autofocus
            autoFocus
            value={state.name}
            onChange={(e) => set({ name: e.target.value })}
            onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); handleCreate(); } }}
            placeholder="Project name"
            className="min-w-0 flex-1 bg-transparent text-[12px] font-medium text-daw-text outline-none placeholder:text-daw-faint"
          />
        </label>
      </div>

      {/* ── Template cards ── */}
      <div className="border-t border-white/[0.05] px-3 py-2.5">
        <div className="mb-2 text-[9px] font-semibold uppercase tracking-wide text-daw-faint">Template</div>
        <div className="grid grid-cols-5 gap-1.5">
          {TEMPLATES.map(({ id, label, icon: Icon, detail }) => {
            const active = state.template === id;
            return (
              <button
                key={id}
                type="button"
                onClick={() => applyTemplate(id)}
                className={[
                  "flex flex-col items-center gap-1.5 rounded-lg border py-2.5 px-1 text-center transition-all",
                  active
                    ? "border-daw-accent/50 bg-daw-accent/[0.07]"
                    : "border-white/[0.06] bg-[#1f242c] hover:border-white/[0.1] hover:bg-[#232830]",
                ].join(" ")}
              >
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
                <div>
                  <div className={`text-[10px] font-semibold leading-tight ${active ? "text-daw-text" : "text-daw-dim"}`}>
                    {label}
                  </div>
                  <div className="mt-0.5 text-[9px] leading-tight text-daw-faint opacity-70">
                    {detail}
                  </div>
                </div>
              </button>
            );
          })}
        </div>
      </div>

      {/* ── BPM + Time Signature ── */}
      <div className="grid grid-cols-2 gap-2.5 border-t border-white/[0.05] px-3 py-2.5">
        <OptionGroup label="BPM">
          <Stepper value={state.bpm} min={40} max={320} onChange={(v) => set({ bpm: v })} />
        </OptionGroup>

        <OptionGroup label="Time Signature">
          <input
            type="number"
            min={1}
            max={16}
            value={state.timeSignatureNumerator}
            onChange={(e) => set({ timeSignatureNumerator: Math.max(1, Math.min(16, Number(e.target.value))) })}
            className="h-7 w-10 shrink-0 rounded-md border border-white/[0.07] bg-[#13161c] text-center text-[12px] font-semibold tabular-nums text-daw-text outline-none focus:border-daw-accent/50"
          />
          <span className="shrink-0 text-[14px] font-light text-daw-faint select-none">/</span>
          {([2, 4, 8, 16] as const).map((d) => (
            <button
              key={d}
              type="button"
              onClick={() => set({ timeSignatureDenominator: d })}
              className={[
                "h-7 flex-1 rounded-md border px-1 text-[11px] font-semibold transition-colors",
                state.timeSignatureDenominator === d
                  ? "border-daw-accent/50 bg-daw-accent/[0.14] text-daw-text"
                  : "border-white/[0.07] bg-[#13161c] text-daw-faint hover:bg-white/[0.05] hover:text-daw-text",
              ].join(" ")}
            >
              {d}
            </button>
          ))}
        </OptionGroup>
      </div>

      {/* ── Sample Rate ── */}
      <div className="border-t border-white/[0.05] px-3 py-2.5">
        <OptionGroup label="Sample Rate">
          {SAMPLE_RATES.map((sr) => (
            <button
              key={sr}
              type="button"
              onClick={() => set({ sampleRate: sr })}
              className={[
                "h-7 flex-1 rounded-md border px-2 text-[11px] font-semibold tabular-nums transition-colors",
                state.sampleRate === sr
                  ? "border-daw-accent/50 bg-daw-accent/[0.14] text-daw-text"
                  : "border-white/[0.07] bg-[#13161c] text-daw-faint hover:bg-white/[0.05] hover:text-daw-text",
              ].join(" ")}
            >
              {SR_LABEL[sr]}
            </button>
          ))}
        </OptionGroup>
      </div>

      {/* ── Starter tracks ── */}
      <div className="grid grid-cols-2 gap-2.5 border-t border-white/[0.05] px-3 py-2.5">
        <OptionGroup label="Audio Tracks">
          <Stepper value={state.audioTrackCount} min={0} max={32} onChange={(v) => set({ audioTrackCount: v })} />
        </OptionGroup>

        <OptionGroup label="MIDI Tracks">
          <Stepper value={state.midiTrackCount} min={0} max={32} onChange={(v) => set({ midiTrackCount: v })} />
        </OptionGroup>
      </div>

      {/* ── Summary strip ── */}
      <div className="border-t border-white/[0.05] px-3 py-2">
        <span className="text-[10px] tabular-nums text-daw-faint">
          {state.bpm} BPM
          {" · "}
          {state.timeSignatureNumerator}/{state.timeSignatureDenominator}
          {" · "}
          {SR_LABEL[state.sampleRate]} Hz
          {" · "}
          {templateLabel}
          {totalTracks > 0 && ` · ${totalTracks} track${totalTracks !== 1 ? "s" : ""}`}
        </span>
      </div>

      {/* ── Footer ── */}
      <div className="flex items-center justify-end gap-2 border-t border-white/[0.05] px-3 py-2.5">
        <button
          type="button"
          onClick={() => useWindowStore.getState().closeWindow(windowId)}
          className="h-7 rounded-md border border-white/[0.07] bg-transparent px-3 text-[11px] font-medium text-daw-faint transition-colors hover:bg-white/[0.05] hover:text-daw-text"
        >
          Cancel
        </button>
        <button
          type="button"
          onClick={handleCreate}
          className="flex h-7 items-center gap-1.5 rounded-md px-3 text-[11px] font-semibold text-[#0d1117] transition-colors"
          style={{ background: "rgba(86,199,201,0.85)" }}
          onMouseEnter={(e) => ((e.currentTarget as HTMLElement).style.background = "rgba(86,199,201,1)")}
          onMouseLeave={(e) => ((e.currentTarget as HTMLElement).style.background = "rgba(86,199,201,0.85)")}
        >
          <Plus size={12} />
          Create Project
        </button>
      </div>
    </div>
  );
}
