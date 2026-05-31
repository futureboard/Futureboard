import { useEffect, useRef, useState } from "react";
import {
  FileText, Mic2, Music, SlidersHorizontal, Square, FolderOpen, Plus, Loader, Check,
} from "lucide-react";
import { useProjectStore } from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
import { useHistoryStore } from "../../store/historyStore";
import { useWindowStore } from "../../store/windowStore";
import { getTrackColor } from "../../theme";
import type { DawTrack } from "../../types/daw";
import { platform } from "../../platform";
import { NumberInput } from "../ui/NumberInput";
import { rememberSavedProject, requestMainWindowOpenProject } from "../../utils/projectLifecycle";

type Props = { windowId: string; external?: boolean };

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
  /** Electron only: absolute path chosen for project location. */
  location: string;
};

const TEMPLATE_PRESETS: Record<Template, Partial<WizardState>> = {
  empty:        { audioTrackCount: 0, midiTrackCount: 0 },
  recording:    { audioTrackCount: 4, midiTrackCount: 0, bpm: 120 },
  "beat-making":{ audioTrackCount: 0, midiTrackCount: 4, bpm: 140 },
  mixing:       { audioTrackCount: 8, midiTrackCount: 0 },
  scoring:      { audioTrackCount: 0, midiTrackCount: 8, timeSignatureNumerator: 4 },
};

type TemplateInfo = {
  id: Template;
  label: string;
  icon: React.ElementType;
  detail: string;
  accentBg: string;
  iconColor: string;
  borderActive: string;
  barColor: string;
};

const TEMPLATES: TemplateInfo[] = [
  {
    id: "empty",
    label: "Empty",
    icon: Square,
    detail: "Blank canvas",
    accentBg: "rgba(255,255,255,0.04)",
    iconColor: "#8a95a3",
    borderActive: "rgba(255,255,255,0.12)",
    barColor: "#8a95a3",
  },
  {
    id: "recording",
    label: "Recording",
    icon: Mic2,
    detail: "4 audio tracks",
    accentBg: "rgba(239,107,107,0.06)",
    iconColor: "#ef9090",
    borderActive: "rgba(239,107,107,0.32)",
    barColor: "#ef9090",
  },
  {
    id: "beat-making",
    label: "Beat Making",
    icon: Music,
    detail: "4 MIDI tracks",
    accentBg: "rgba(167,107,239,0.06)",
    iconColor: "#c490ef",
    borderActive: "rgba(167,107,239,0.32)",
    barColor: "#c490ef",
  },
  {
    id: "mixing",
    label: "Mixing",
    icon: SlidersHorizontal,
    detail: "8 audio tracks",
    accentBg: "rgba(86,199,201,0.06)",
    iconColor: "#56c7c9",
    borderActive: "rgba(86,199,201,0.4)",
    barColor: "#56c7c9",
  },
  {
    id: "scoring",
    label: "Scoring",
    icon: FileText,
    detail: "8 MIDI tracks",
    accentBg: "rgba(128,209,138,0.06)",
    iconColor: "#80d18a",
    borderActive: "rgba(128,209,138,0.32)",
    barColor: "#80d18a",
  },
];

const SAMPLE_RATES = [44100, 48000, 88200, 96000] as const;
const SR_LABEL: Record<number, string> = {
  44100: "44.1k",
  48000: "48k",
  88200: "88.2k",
  96000: "96k",
};

// ─── Sub-components ──────────────────────────────────────────────────────────

function FieldLabel({ children }: { children: React.ReactNode }) {
  return (
    <div className="mb-1.5 text-[10px] font-semibold uppercase tracking-wider text-daw-faint">
      {children}
    </div>
  );
}

function Stepper({
  value, min, max, onChange,
}: { value: number; min: number; max: number; onChange: (v: number) => void }) {
  return (
    <div className="flex items-center gap-1">
      <button
        type="button"
        onClick={() => onChange(Math.max(min, value - 1))}
        className="flex h-7 w-7 shrink-0 items-center justify-center rounded border border-white/[0.08] bg-white/[0.03] text-[14px] font-light text-daw-dim transition-colors hover:bg-white/[0.07] hover:text-daw-text select-none"
      >
        −
      </button>
      <NumberInput
        min={min}
        max={max}
        value={value}
        className="!h-7 w-14 shrink-0"
        align="center"
        ariaLabel="Numeric value"
        onChange={(next) => onChange(Math.max(min, Math.min(max, next || min)))}
      />
      <button
        type="button"
        onClick={() => onChange(Math.min(max, value + 1))}
        className="flex h-7 w-7 shrink-0 items-center justify-center rounded border border-white/[0.08] bg-white/[0.03] text-[14px] font-light text-daw-dim transition-colors hover:bg-white/[0.07] hover:text-daw-text select-none"
      >
        +
      </button>
    </div>
  );
}

// ─── Main component ───────────────────────────────────────────────────────────

export function ProjectWizard({ windowId, external = false }: Props) {
  const nameRef = useRef<HTMLInputElement>(null);

  const isElectron = platform.kind === "electron" && platform.folderProject.isSupported;

  const [state, setState] = useState<WizardState>({
    name: "Untitled Project",
    bpm: 120,
    timeSignatureNumerator: 4,
    timeSignatureDenominator: 4,
    sampleRate: 48000,
    template: "empty",
    audioTrackCount: 0,
    midiTrackCount: 0,
    location: "",
  });
  const [isCreating, setIsCreating] = useState(false);
  const [nameTouched, setNameTouched] = useState(false);
  const set = (patch: Partial<WizardState>) => setState((s) => ({ ...s, ...patch }));

  // Pre-fill the project location with the OS default path on Electron
  useEffect(() => {
    if (!isElectron) return;
    platform.folderProject.getDefaultProjectsPath()
      .then((dir) => { if (dir) set({ location: dir }); })
      .catch(() => { /* ignore — dialog will default correctly */ });
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const applyTemplate = (t: Template) => set({ template: t, ...TEMPLATE_PRESETS[t] });

  const handleBrowseLocation = async () => {
    const loc = await platform.folderProject.browseLocation();
    if (loc) set({ location: loc });
  };

  const handleCreate = async () => {
    if (isCreating) return;
    const history = useHistoryStore.getState();
    const uiStore = useUIStore.getState();
    const ws = useWindowStore.getState();
    const projectName = state.name.trim() || "Untitled Project";

    // Electron: location required
    if (isElectron && !state.location) {
      await platform.folderProject.browseLocation().then((loc) => {
        if (loc) set({ location: loc });
      });
      return;
    }

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

    const newProject = {
      id: crypto.randomUUID(),
      name: projectName,
      version: 1,
      sampleRate: state.sampleRate,
      bpm: state.bpm,
      timeSignature: {
        numerator: state.timeSignatureNumerator,
        denominator: state.timeSignatureDenominator,
      },
      tracks,
      files: [],
    };

    if (isElectron) {
      setIsCreating(true);
      try {
        const folderResult = await platform.folderProject.createProject({
          name: projectName,
          location: state.location,
        });
        if (!folderResult) {
          setIsCreating(false);
          return;
        }
        useProjectStore.getState().loadProject(newProject);
        const saveResult = await platform.projectStorage.saveProject(newProject);
        const rememberedResult = saveResult ?? {
          path: folderResult.projectFilePath,
          projectRoot: folderResult.projectRoot,
        };
        rememberSavedProject(newProject, rememberedResult);
        if (external && platform.kind === "electron") {
          requestMainWindowOpenProject(rememberedResult.path ?? folderResult.projectFilePath);
          platform.window.close();
          return;
        }
      } catch (e) {
        console.error("[ProjectWizard] folder create failed:", e);
        setIsCreating(false);
        return;
      }
    } else {
      useProjectStore.getState().loadProject(newProject);
      const saveResult = await platform.projectStorage.saveProject(newProject);
      if (saveResult) rememberSavedProject(newProject, saveResult);
    }

    history.clear();
    uiStore.setSelectedClipIds([]);
    uiStore.setSelectedTrackId(null);
    uiStore.setSaveStatus("saved");
    // Sync to localStorage so any external window opened after this sees the new project.
    useProjectStore.getState().saveLocal();
    console.log("[ProjectWizard] project created and clean, localStorage synced");

    if (external && platform.kind === "electron") {
      platform.window.close();
    } else {
      ws.closeWindow(windowId);
    }
  };

  const nameEmpty = nameTouched && state.name.trim() === "";
  const canCreate = !isCreating && state.name.trim() !== "" && (!isElectron || !!state.location);

  const totalTracks = state.audioTrackCount + state.midiTrackCount;
  const templateLabel = TEMPLATES.find((t) => t.id === state.template)?.label ?? state.template;

  return (
    <div className={`flex h-full flex-col ${external ? "bg-[#0e1319]" : ""}`} style={{ minHeight: 0 }}>

      {/* ── Subtitle ── */}
      <div
        className="shrink-0 px-4 py-2"
        style={{ borderBottom: "1px solid rgba(255,255,255,0.05)" }}
      >
        <p className="text-[11px] text-daw-faint leading-tight">
          Set up your project settings before creating your session.
        </p>
      </div>

      {/* ── Two-column body ── */}
      <div className="flex min-h-0 flex-1 overflow-hidden">

        {/* ── Left: identity + templates ── */}
        <div
          className="flex shrink-0 flex-col overflow-y-auto"
          style={{ width: 268, borderRight: "1px solid rgba(255,255,255,0.05)" }}
        >
          {/* Project Name */}
          <div className="px-4 pt-4 pb-3.5">
            <FieldLabel>Project Name</FieldLabel>
            <input
              ref={nameRef}
              autoFocus
              value={state.name}
              onChange={(e) => { setNameTouched(true); set({ name: e.target.value }); }}
              onBlur={() => setNameTouched(true)}
              onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); void handleCreate(); } }}
              placeholder="Untitled Project"
              className="w-full rounded-md border bg-[#0c1016] px-3 text-[13px] font-medium text-daw-text outline-none transition-colors placeholder:text-daw-faint focus:border-daw-accent/50"
              style={{
                height: 34,
                borderColor: nameEmpty
                  ? "rgba(239,107,107,0.45)"
                  : "rgba(255,255,255,0.08)",
              }}
            />
            {nameEmpty && (
              <p className="mt-1 text-[10px]" style={{ color: "#ef9090" }}>
                Project name is required.
              </p>
            )}
          </div>

          {/* Save Location — Electron only */}
          {isElectron && (
            <div
              className="px-4 pb-3.5"
              style={{ borderTop: "1px solid rgba(255,255,255,0.05)", paddingTop: 12 }}
            >
              <FieldLabel>Save Location</FieldLabel>
              <div className="flex items-center gap-1.5">
                <div
                  className="flex min-w-0 flex-1 items-center gap-2 rounded-md border bg-[#0c1016] px-2.5"
                  style={{
                    height: 30,
                    borderColor: state.location
                      ? "rgba(255,255,255,0.07)"
                      : "rgba(255,255,255,0.05)",
                  }}
                >
                  <FolderOpen size={11} className="shrink-0 text-daw-faint" />
                  <span
                    className="min-w-0 flex-1 truncate text-[11px]"
                    style={{ color: state.location ? "#8a95a3" : "#3e4a57" }}
                    title={state.location || undefined}
                  >
                    {state.location
                      ? state.location.split(/[/\\]/).slice(-2).join("/")
                      : "No location selected"}
                  </span>
                </div>
                <button
                  type="button"
                  onClick={() => { void handleBrowseLocation(); }}
                  className="h-[30px] shrink-0 rounded-md border border-white/[0.08] bg-white/[0.03] px-2.5 text-[11px] font-medium text-daw-dim transition-colors hover:bg-white/[0.07] hover:text-daw-text"
                >
                  Browse…
                </button>
              </div>
              <p className="mt-1.5 text-[10px] text-daw-faint leading-snug">
                {state.location
                  ? `Project folder will be created at the selected path.`
                  : `Choose where the project folder will be saved.`}
              </p>
            </div>
          )}

          {/* Template picker */}
          <div
            className="flex-1 px-4 pb-4"
            style={{ borderTop: "1px solid rgba(255,255,255,0.05)", paddingTop: 12 }}
          >
            <FieldLabel>Template</FieldLabel>
            <div className="flex flex-col gap-1">
              {TEMPLATES.map(({ id, label, icon: Icon, detail, accentBg, iconColor, borderActive}) => {
                const active = state.template === id;
                return (
                  <button
                    key={id}
                    type="button"
                    onClick={() => applyTemplate(id)}
                    className="flex items-center gap-3 rounded-md px-3 text-left transition-all"
                    style={{
                      height: 42,
                      background: active ? accentBg : "transparent",
                      border: `1px solid ${active ? borderActive : "transparent"}`
                    }}
                    onMouseEnter={(e) => {
                      if (!active) {
                        (e.currentTarget as HTMLElement).style.background = "rgba(255,255,255,0.03)";
                        (e.currentTarget as HTMLElement).style.borderColor = "rgba(255,255,255,0.07)";
                      }
                    }}
                    onMouseLeave={(e) => {
                      if (!active) {
                        (e.currentTarget as HTMLElement).style.background = "transparent";
                        (e.currentTarget as HTMLElement).style.borderColor = "transparent";
                      }
                    }}
                  >
                    <div
                      className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md"
                      style={{
                        background: active ? `${iconColor}1a` : "rgba(255,255,255,0.04)",
                        color: active ? iconColor : "#4a5568",
                      }}
                    >
                      <Icon size={13} />
                    </div>
                    <div className="min-w-0 flex-1">
                      <div
                        className="text-[11px] font-semibold leading-tight"
                        style={{ color: active ? "#e7edf5" : "#8a95a3" }}
                      >
                        {label}
                      </div>
                      <div
                        className="mt-0.5 text-[10px] leading-tight"
                        style={{ color: active ? "#566372" : "#334155" }}
                      >
                        {detail}
                      </div>
                    </div>
                    {active && (
                      <Check size={11} className="shrink-0" style={{ color: iconColor }} />
                    )}
                  </button>
                );
              })}
            </div>
          </div>
        </div>

        {/* ── Right: session settings ── */}
        <div className="flex min-w-0 flex-1 flex-col overflow-y-auto px-5 py-4">
          <div
            className="mb-4 text-[10px] font-semibold uppercase tracking-wider text-daw-faint"
          >
            Session Settings
          </div>

          {/* Tempo + Time Signature */}
          <div className="mb-5 grid grid-cols-2 gap-4">
            {/* Tempo */}
            <div>
              <FieldLabel>Tempo</FieldLabel>
              <div className="flex items-center gap-2">
                <Stepper
                  value={state.bpm}
                  min={20}
                  max={300}
                  onChange={(v) => set({ bpm: v })}
                />
                <span className="text-[10px] text-daw-faint tabular-nums">BPM</span>
              </div>
            </div>

            {/* Time Signature */}
            <div>
              <FieldLabel>Time Signature</FieldLabel>
              <div className="flex items-center gap-1.5">
                <NumberInput
                  min={1}
                  max={16}
                  value={state.timeSignatureNumerator}
                  className="!h-7 w-11 shrink-0"
                  align="center"
                  ariaLabel="Numerator"
                  onChange={(value) =>
                    set({ timeSignatureNumerator: Math.max(1, Math.min(16, value)) })
                  }
                />
                <span className="shrink-0 text-[13px] font-light text-daw-faint select-none">
                  /
                </span>
                <div className="flex gap-0.5">
                  {([2, 4, 8, 16] as const).map((d) => {
                    const sel = state.timeSignatureDenominator === d;
                    return (
                      <button
                        key={d}
                        type="button"
                        onClick={() => set({ timeSignatureDenominator: d })}
                        className="h-7 w-8 rounded border text-[11px] font-semibold transition-colors"
                        style={{
                          borderColor: sel
                            ? "rgba(86,199,201,0.5)"
                            : "rgba(255,255,255,0.07)",
                          background: sel
                            ? "rgba(86,199,201,0.13)"
                            : "rgba(255,255,255,0.03)",
                          color: sel ? "#e7edf5" : "#566372",
                        }}
                      >
                        {d}
                      </button>
                    );
                  })}
                </div>
              </div>
            </div>
          </div>

          {/* Sample Rate */}
          <div className="mb-5">
            <FieldLabel>Sample Rate</FieldLabel>
            <div className="flex gap-1">
              {SAMPLE_RATES.map((sr) => {
                const sel = state.sampleRate === sr;
                return (
                  <button
                    key={sr}
                    type="button"
                    onClick={() => set({ sampleRate: sr })}
                    className="h-8 flex-1 rounded border text-[11px] font-semibold tabular-nums transition-colors"
                    style={{
                      borderColor: sel
                        ? "rgba(86,199,201,0.5)"
                        : "rgba(255,255,255,0.07)",
                      background: sel
                        ? "rgba(86,199,201,0.13)"
                        : "rgba(255,255,255,0.03)",
                      color: sel ? "#e7edf5" : "#566372",
                    }}
                  >
                    {SR_LABEL[sr]} Hz
                  </button>
                );
              })}
            </div>
          </div>

          {/* Starter Tracks */}
          <div>
            <FieldLabel>Starter Tracks</FieldLabel>
            <div className="grid grid-cols-2 gap-4">
              <div>
                <div className="mb-1.5 text-[10px] text-daw-faint">Audio</div>
                <Stepper
                  value={state.audioTrackCount}
                  min={0}
                  max={32}
                  onChange={(v) => set({ audioTrackCount: v })}
                />
              </div>
              <div>
                <div className="mb-1.5 text-[10px] text-daw-faint">MIDI</div>
                <Stepper
                  value={state.midiTrackCount}
                  min={0}
                  max={32}
                  onChange={(v) => set({ midiTrackCount: v })}
                />
              </div>
            </div>
          </div>

          {/* Web storage note */}
          {!isElectron && (
            <div
              className="mt-auto pt-4"
            >
              <div
                className="flex items-center gap-2 rounded-md px-3 py-2"
                style={{
                  background: "rgba(255,255,255,0.025)",
                  border: "1px solid rgba(255,255,255,0.05)",
                }}
              >
                <div
                  className="h-1.5 w-1.5 shrink-0 rounded-full"
                  style={{ background: "rgba(86,199,201,0.5)" }}
                />
                <span className="text-[10px] text-daw-faint">
                  Project will be saved in browser storage.
                </span>
              </div>
            </div>
          )}
        </div>
      </div>

      {/* ── Summary strip ── */}
      <div
        className="flex shrink-0 items-center gap-2.5 px-4"
        style={{
          height: 34,
          borderTop: "1px solid rgba(255,255,255,0.06)",
          background: "rgba(255,255,255,0.018)",
        }}
      >
        <div
          className="h-1.5 w-1.5 shrink-0 rounded-full"
          style={{ background: "rgba(86,199,201,0.55)" }}
        />
        <span className="text-[10px] tabular-nums text-daw-faint">
          {state.bpm} BPM
          <span className="mx-1.5 opacity-30">·</span>
          {state.timeSignatureNumerator}/{state.timeSignatureDenominator}
          <span className="mx-1.5 opacity-30">·</span>
          {SR_LABEL[state.sampleRate]} Hz
          <span className="mx-1.5 opacity-30">·</span>
          {templateLabel}
          {totalTracks > 0 && (
            <>
              <span className="mx-1.5 opacity-30">·</span>
              {totalTracks} track{totalTracks !== 1 ? "s" : ""}
            </>
          )}
        </span>
      </div>

      {/* ── Footer ── */}
      <div
        className="flex shrink-0 items-center justify-end gap-2 px-4"
        style={{
          height: 50,
          borderTop: "1px solid rgba(255,255,255,0.08)",
          background: "rgba(0,0,0,0.18)",
        }}
      >
        <button
          type="button"
          disabled={isCreating}
          onClick={() => {
            if (external && platform.kind === "electron") platform.window.close();
            else useWindowStore.getState().closeWindow(windowId);
          }}
          className="h-8 rounded-md border border-white/[0.08] bg-transparent px-4 text-[12px] font-medium text-daw-faint transition-colors hover:bg-white/[0.05] hover:text-daw-text disabled:opacity-40"
        >
          Cancel
        </button>
        <button
          type="button"
          disabled={!canCreate}
          onClick={() => { void handleCreate(); }}
          className="flex h-8 items-center gap-1.5 rounded-md px-4 text-[12px] font-semibold transition-all disabled:opacity-40"
          style={{
            background: canCreate ? "rgba(86,199,201,0.88)" : "rgba(86,199,201,0.5)",
            color: canCreate ? "#0a0e14" : "#1a2530",
          }}
          onMouseEnter={(e) => {
            if (canCreate) (e.currentTarget as HTMLElement).style.background = "rgba(86,199,201,1)";
          }}
          onMouseLeave={(e) => {
            (e.currentTarget as HTMLElement).style.background = canCreate
              ? "rgba(86,199,201,0.88)"
              : "rgba(86,199,201,0.5)";
          }}
        >
          {isCreating ? <Loader size={12} className="animate-spin" /> : <Plus size={13} />}
          {isCreating ? "Creating…" : "Create Project"}
        </button>
      </div>
    </div>
  );
}
