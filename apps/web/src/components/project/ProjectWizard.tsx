import { useState } from "react";
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
  empty: { audioTrackCount: 0, midiTrackCount: 0 },
  recording: { audioTrackCount: 4, midiTrackCount: 0, bpm: 120 },
  "beat-making": { audioTrackCount: 0, midiTrackCount: 4, bpm: 140 },
  mixing: { audioTrackCount: 8, midiTrackCount: 0 },
  scoring: { audioTrackCount: 0, midiTrackCount: 8, timeSignatureNumerator: 4 },
};

const SAMPLE_RATES = [44100, 48000, 88200, 96000];

export function ProjectWizard({ windowId }: Props) {
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

  const applyTemplate = (t: Template) => {
    set({ template: t, ...TEMPLATE_PRESETS[t] });
  };

  const handleCreate = () => {
    const history = useHistoryStore.getState();
    const uiStore = useUIStore.getState();
    const ws = useWindowStore.getState();

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

  const labelClass = "block text-[10px] text-daw-text-muted uppercase tracking-wide mb-1";
  const inputClass = "w-full bg-daw-bg border border-daw-border rounded px-2 py-1 text-[12px] text-daw-text focus:outline-none focus:border-blue-500";
  const selectClass = inputClass;

  return (
    <div className="flex flex-col gap-4 text-[12px]">
      {/* Project Name */}
      <div>
        <label className={labelClass}>Project Name</label>
        <input
          className={inputClass}
          value={state.name}
          onChange={(e) => set({ name: e.target.value })}
          // eslint-disable-next-line jsx-a11y/no-autofocus
          autoFocus
          onKeyDown={(e) => e.key === "Enter" && handleCreate()}
        />
      </div>

      {/* Template */}
      <div>
        <label className={labelClass}>Template</label>
        <div className="grid grid-cols-5 gap-1">
          {(Object.keys(TEMPLATE_PRESETS) as Template[]).map((t) => (
            <button
              key={t}
              onClick={() => applyTemplate(t)}
              className={`py-1 px-1.5 text-[10px] rounded border capitalize ${
                state.template === t
                  ? "border-blue-500 bg-blue-600/20 text-blue-300"
                  : "border-daw-border text-daw-text-muted hover:border-daw-text hover:text-daw-text"
              }`}
            >
              {t.replace("-", " ")}
            </button>
          ))}
        </div>
      </div>

      {/* BPM + Time Signature */}
      <div className="grid grid-cols-3 gap-3">
        <div>
          <label className={labelClass}>BPM</label>
          <input
            type="number"
            className={inputClass}
            value={state.bpm}
            min={40}
            max={320}
            onChange={(e) => set({ bpm: Math.max(40, Math.min(320, Number(e.target.value))) })}
          />
        </div>
        <div>
          <label className={labelClass}>Time Sig. (Num)</label>
          <input
            type="number"
            className={inputClass}
            value={state.timeSignatureNumerator}
            min={1}
            max={16}
            onChange={(e) => set({ timeSignatureNumerator: Number(e.target.value) })}
          />
        </div>
        <div>
          <label className={labelClass}>Time Sig. (Den)</label>
          <select
            className={selectClass}
            value={state.timeSignatureDenominator}
            onChange={(e) => set({ timeSignatureDenominator: Number(e.target.value) })}
          >
            {[2, 4, 8, 16].map((d) => (
              <option key={d} value={d}>{d}</option>
            ))}
          </select>
        </div>
      </div>

      {/* Sample Rate */}
      <div>
        <label className={labelClass}>Sample Rate</label>
        <select
          className={selectClass}
          value={state.sampleRate}
          onChange={(e) => set({ sampleRate: Number(e.target.value) })}
        >
          {SAMPLE_RATES.map((sr) => (
            <option key={sr} value={sr}>{sr.toLocaleString()} Hz</option>
          ))}
        </select>
      </div>

      {/* Starter Tracks */}
      <div className="grid grid-cols-2 gap-3">
        <div>
          <label className={labelClass}>Audio Tracks</label>
          <input
            type="number"
            className={inputClass}
            value={state.audioTrackCount}
            min={0}
            max={32}
            onChange={(e) => set({ audioTrackCount: Number(e.target.value) })}
          />
        </div>
        <div>
          <label className={labelClass}>MIDI Tracks</label>
          <input
            type="number"
            className={inputClass}
            value={state.midiTrackCount}
            min={0}
            max={32}
            onChange={(e) => set({ midiTrackCount: Number(e.target.value) })}
          />
        </div>
      </div>

      {/* Summary */}
      <div className="text-[10px] text-daw-text-muted border-t border-daw-border pt-3">
        {state.bpm} BPM · {state.timeSignatureNumerator}/{state.timeSignatureDenominator} · {state.sampleRate.toLocaleString()} Hz
        {(state.audioTrackCount + state.midiTrackCount) > 0
          ? ` · ${state.audioTrackCount + state.midiTrackCount} tracks`
          : " · Empty"}
      </div>

      {/* Actions */}
      <div className="flex gap-2 justify-end border-t border-daw-border pt-3 -mb-1">
        <button
          className="px-3 py-1.5 text-[11px] bg-daw-surface hover:bg-white/10 text-daw-text border border-daw-border rounded"
          onClick={() => useWindowStore.getState().closeWindow(windowId)}
        >
          Cancel
        </button>
        <button
          className="px-3 py-1.5 text-[11px] bg-blue-600 hover:bg-blue-500 text-white rounded font-medium"
          onClick={handleCreate}
        >
          Create Project
        </button>
      </div>
    </div>
  );
}
