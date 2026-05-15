import { useEffect, useState } from "react";
import { useSettingsStore, APP_SETTINGS_DEFAULTS as DEFAULTS } from "../../store/settingsStore";
import { useProjectStore } from "../../store/projectStore";
import { useWindowStore } from "../../store/windowStore";
import { DawSelect } from "../ui/DawSelect";
import { KeyboardShortcutsPanel } from "./KeyboardShortcutsPanel";
import type { AppSettings, PreferredBufferSize, PreferredEngine, StartupBehavior } from "../../store/settingsStore";

type SettingsTab = "general" | "audio" | "midi" | "project" | "appearance" | "advanced" | "shortcuts";

type ProjectDraft = {
  name: string;
  bpm: number;
  timeSignatureNumerator: number;
  timeSignatureDenominator: number;
  sampleRate: number;
};

type Props = { windowId: string; initialTab?: SettingsTab };

// ── Shared control classes ────────────────────────────────────────────────────

const inputCls =
  "w-full bg-[#151a21] border border-[rgba(255,255,255,0.08)] rounded px-2 h-[28px] text-[12px] text-daw-text focus:outline-none focus:border-[rgba(114,215,215,0.5)] transition-colors";

// ── Reusable setting row ──────────────────────────────────────────────────────

function SettingsRow({
  label,
  description,
  children,
}: {
  label: string;
  description?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-center gap-4 min-h-[50px] border-b border-[rgba(255,255,255,0.05)] last:border-0 py-2">
      <div className="flex-1 min-w-0">
        <div className="text-[12px] text-daw-text leading-none">{label}</div>
        {description && (
          <div className="text-[11px] text-daw-text-muted mt-1 leading-snug">{description}</div>
        )}
      </div>
      <div className="flex-shrink-0">{children}</div>
    </div>
  );
}

function SettingsToggle({
  checked,
  onChange,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
      className={`relative inline-flex h-[17px] w-[30px] items-center rounded-full transition-colors focus:outline-none ${
        checked
          ? "bg-[rgba(114,215,215,0.7)]"
          : "bg-[#1e2530] border border-[rgba(255,255,255,0.1)]"
      }`}
    >
      <span
        className={`inline-block h-[13px] w-[13px] transform rounded-full shadow transition-transform ${
          checked ? "translate-x-[15px] bg-white" : "translate-x-[2px] bg-[#6b7280]"
        }`}
      />
    </button>
  );
}

function SettingsSelect<T extends string | number>({
  value,
  onChange,
  options,
}: {
  value: T;
  onChange: (v: T) => void;
  options: { value: T; label: string }[];
}) {
  return (
    <DawSelect
      className="w-44"
      value={String(value)}
      onChange={(val) => {
        const opt = options.find((o) => String(o.value) === val);
        if (opt) onChange(opt.value);
      }}
      options={options.map((o) => ({
        value: String(o.value),
        label: o.label,
      }))}
    />
  );
}

function SectionHeader({ children }: { children: React.ReactNode }) {
  return (
    <div className="text-[10px] text-[rgba(255,255,255,0.3)] uppercase tracking-[0.12em] font-medium mb-0 mt-5 first:mt-0 pb-1.5 border-b border-[rgba(255,255,255,0.05)]">
      {children}
    </div>
  );
}

// ── Tab content panels ────────────────────────────────────────────────────────

function GeneralTab({ draft, setDraft }: { draft: AppSettings; setDraft: (p: Partial<AppSettings>) => void }) {
  return (
    <div className="flex flex-col">
      <SectionHeader>Startup</SectionHeader>
      <SettingsRow label="Startup Behavior" description="What to show when the app starts">
        <SettingsSelect<StartupBehavior>
          value={draft.startupBehavior}
          onChange={(v) => setDraft({ startupBehavior: v })}
          options={[
            { value: "lastProject", label: "Open Last Project" },
            { value: "newProject", label: "Create New Project" },
            { value: "wizard", label: "Show Project Wizard" },
          ]}
        />
      </SettingsRow>

      <SectionHeader>File Management</SectionHeader>
      <SettingsRow label="Auto-Save" description="Save project changes automatically in the background">
        <SettingsToggle checked={draft.autoSave} onChange={(v) => setDraft({ autoSave: v })} />
      </SettingsRow>
      {draft.autoSave && (
        <SettingsRow label="Auto-Save Interval" description="Minutes between automatic saves">
          <input
            type="number"
            className={`${inputCls} w-24`}
            value={draft.autoSaveIntervalMin}
            min={1}
            max={60}
            onChange={(e) => setDraft({ autoSaveIntervalMin: Math.max(1, Number(e.target.value)) })}
          />
        </SettingsRow>
      )}
    </div>
  );
}

function AudioTab({ draft, setDraft }: { draft: AppSettings; setDraft: (p: Partial<AppSettings>) => void }) {
  return (
    <div className="flex flex-col">
      <SectionHeader>Engine</SectionHeader>
      <SettingsRow label="Audio Engine" description="Select the audio processing backend">
        <SettingsSelect<PreferredEngine>
          value={draft.preferredEngine}
          onChange={(v) => setDraft({ preferredEngine: v })}
          options={[
            { value: "auto", label: "Automatic (Recommended)" },
            { value: "webAudio", label: "Web Audio (Built-in)" },
            { value: "wasm", label: "WASM Engine (High Performance)" },
          ]}
        />
      </SettingsRow>

      <SectionHeader>Performance</SectionHeader>
      <SettingsRow label="Buffer Size" description="Lower values reduce latency but increase CPU load">
        <SettingsSelect<PreferredBufferSize>
          value={draft.preferredBufferSize}
          onChange={(v) => setDraft({ preferredBufferSize: v })}
          options={[
            { value: 64, label: "64 samples" },
            { value: 128, label: "128 samples" },
            { value: 256, label: "256 samples" },
            { value: 512, label: "512 samples" },
            { value: 1024, label: "1024 samples" },
          ]}
        />
      </SettingsRow>

      <SectionHeader>Monitoring</SectionHeader>
      <SettingsRow label="Input Monitoring" description="Hear audio inputs during recording">
        <SettingsToggle checked={draft.enableDevTools} onChange={(v) => setDraft({ enableDevTools: v })} />
      </SettingsRow>
    </div>
  );
}

function ProjectTab({ projectDraft, setProjectDraft }: { projectDraft: ProjectDraft; setProjectDraft: (p: Partial<ProjectDraft>) => void }) {
  return (
    <div className="flex flex-col">
      <SectionHeader>Defaults</SectionHeader>
      <SettingsRow label="Project Name">
        <input
          type="text"
          className={inputCls}
          value={projectDraft.name}
          onChange={(e) => setProjectDraft({ name: e.target.value })}
        />
      </SettingsRow>
      <SettingsRow label="Tempo (BPM)">
        <input
          type="number"
          className={`${inputCls} w-24`}
          value={projectDraft.bpm}
          min={40}
          max={300}
          onChange={(e) => setProjectDraft({ bpm: Math.max(40, Number(e.target.value)) })}
        />
      </SettingsRow>
      <SettingsRow label="Time Signature">
        <div className="flex items-center gap-1.5">
          <input
            type="number"
            className={`${inputCls} w-14 text-center`}
            value={projectDraft.timeSignatureNumerator}
            min={1}
            max={32}
            onChange={(e) =>
              setProjectDraft({ timeSignatureNumerator: Math.max(1, Number(e.target.value)) })
            }
          />
          <span className="text-[rgba(255,255,255,0.3)] text-xs">/</span>
          <DawSelect
            className="w-14"
            value={String(projectDraft.timeSignatureDenominator)}
            onChange={(val) =>
              setProjectDraft({ timeSignatureDenominator: Number(val) })
            }
            options={[2, 4, 8, 16].map((d) => ({
              value: String(d),
              label: String(d),
            }))}
          />
        </div>
      </SettingsRow>

      <SectionHeader>Audio Format</SectionHeader>
      <SettingsRow label="Sample Rate" description="Changes take effect on next project load">
        <SettingsSelect<number>
          value={projectDraft.sampleRate}
          onChange={(v) => setProjectDraft({ sampleRate: v })}
          options={[
            { value: 44100, label: "44100 Hz" },
            { value: 48000, label: "48000 Hz" },
            { value: 88200, label: "88200 Hz" },
            { value: 96000, label: "96000 Hz" },
          ]}
        />
      </SettingsRow>
    </div>
  );
}

function AppearanceTab({ draft, setDraft }: { draft: AppSettings; setDraft: (p: Partial<AppSettings>) => void }) {
  return (
    <div className="flex flex-col">
      <SectionHeader>Theme</SectionHeader>
      <SettingsRow label="Compact UI" description="Reduce whitespace and padding across the app">
        <SettingsToggle checked={draft.compactUI} onChange={(v) => setDraft({ compactUI: v })} />
      </SettingsRow>

      <SectionHeader>Colors</SectionHeader>
      <SettingsRow label="Theme Style">
        <div className="text-[11px] text-daw-text-muted">Dark Mode (Default)</div>
      </SettingsRow>
    </div>
  );
}

function AdvancedTab({ draft, setDraft, onReset }: { draft: AppSettings; setDraft: (p: Partial<AppSettings>) => void; onReset: () => void }) {
  return (
    <div className="flex flex-col">
      <SectionHeader>Development</SectionHeader>
      <SettingsRow label="Enable DevTools" description="Enable internal debugging tools">
        <SettingsToggle checked={draft.enableDevTools} onChange={(v) => setDraft({ enableDevTools: v })} />
      </SettingsRow>

      <SectionHeader>Maintenance</SectionHeader>
      <SettingsRow label="Reset to Defaults" description="Restore all settings to their original values">
        <button
          className="px-3 h-[28px] text-[11px] bg-red-500/10 hover:bg-red-500/20 text-red-400 border border-red-500/30 rounded transition-colors"
          onClick={onReset}
        >
          Reset All Settings
        </button>
      </SettingsRow>
    </div>
  );
}

// ── Main Dialog ──────────────────────────────────────────────────────────────

export function SettingsDialog({ windowId, initialTab = "general" }: Props) {
  const store = useSettingsStore();
  const { project } = useProjectStore();
  const { closeWindow, updateWindowPayload } = useWindowStore();

  const [activeTab, setActiveTab] = useState<SettingsTab>(initialTab);

  // React to tab-switch requests pushed via updateWindowPayload from actionRunner
  const payloadTab = useWindowStore(
    (s) => s.windows.find((w) => w.id === windowId)?.payload?.initialTab as SettingsTab | undefined,
  );
  useEffect(() => {
    if (payloadTab && payloadTab !== activeTab) setActiveTab(payloadTab);
  }, [payloadTab]); // intentionally excludes activeTab to avoid loop

  // Clear the pending tab request once we've acted on it so repeated menu presses work
  useEffect(() => {
    if (payloadTab) updateWindowPayload(windowId, { initialTab: undefined });
  }, [payloadTab, windowId, updateWindowPayload]);

  const [projectDraft, setProjectDraft] = useState<ProjectDraft>({
    name: project.name,
    bpm: project.bpm,
    timeSignatureNumerator: project.timeSignature?.numerator ?? 4,
    timeSignatureDenominator: project.timeSignature?.denominator ?? 4,
    sampleRate: project.sampleRate,
  });

  const [appDraft, setAppDraft] = useState<AppSettings>({
    startupBehavior: store.startupBehavior,
    autoSave: store.autoSave,
    autoSaveIntervalMin: store.autoSaveIntervalMin,
    preferredEngine: store.preferredEngine,
    preferredBufferSize: store.preferredBufferSize,
    compactUI: store.compactUI,
    enableDevTools: store.enableDevTools,
  });

  const patchProject = (p: Partial<ProjectDraft>) => setProjectDraft((s) => ({ ...s, ...p }));
  const patchApp = (p: Partial<AppSettings>) => setAppDraft((s) => ({ ...s, ...p }));

  const handleResetDefaults = () => {
    if (confirm("Reset all settings to defaults? This cannot be undone.")) {
      store.resetToDefaults();
      setAppDraft(DEFAULTS);
    }
  };

  const handleApply = () => {
    store.applySettings(appDraft);
    useProjectStore.setState((s) => ({
      project: {
        ...s.project,
        name: projectDraft.name,
        bpm: projectDraft.bpm,
        timeSignature: {
          numerator: projectDraft.timeSignatureNumerator,
          denominator: projectDraft.timeSignatureDenominator,
        },
        sampleRate: projectDraft.sampleRate,
      },
    }));
  };

  const handleCancel = () => closeWindow(windowId);
  const handleDone = () => {
    handleApply();
    closeWindow(windowId);
  };

  const tabs: { id: SettingsTab; label: string }[] = [
    { id: "general",   label: "General"   },
    { id: "audio",     label: "Audio"     },
    { id: "midi",      label: "MIDI"      },
    { id: "project",   label: "Project"   },
    { id: "shortcuts", label: "Shortcuts" },
    { id: "appearance",label: "Appearance"},
    { id: "advanced",  label: "Advanced"  },
  ];

  return (
    <div className="flex h-full w-full bg-[#0f1319] overflow-hidden shadow-2xl border border-[rgba(255,255,255,0.07)] select-none">
      {/* Sidebar */}
      <div className="w-[160px] flex-shrink-0 bg-[#0c0f14] border-r border-[rgba(255,255,255,0.06)] flex flex-col pt-2 pb-2">
        {tabs.map((tab) => (
          <button
            key={tab.id}
            onClick={() => setActiveTab(tab.id)}
            className={`relative flex items-center h-[32px] px-4 text-left text-[12px] font-medium transition-colors ${
              activeTab === tab.id
                ? "text-[rgba(114,215,215,0.95)] bg-[rgba(114,215,215,0.07)]"
                : "text-[rgba(255,255,255,0.4)] hover:text-[rgba(255,255,255,0.7)] hover:bg-[rgba(255,255,255,0.04)]"
            }`}
          >
            {activeTab === tab.id && (
              <span className="absolute left-0 top-[4px] bottom-[4px] w-[2px] bg-[rgba(114,215,215,0.85)] rounded-r" />
            )}
            {tab.label}
          </button>
        ))}
      </div>

      {/* Content area */}
      <div className="flex-1 flex flex-col min-w-0 overflow-hidden">
        {/* Header */}
        <div className="h-[32px] flex items-center px-5 flex-shrink-0 border-b border-[rgba(255,255,255,0.05)]">
          <span className="text-[11px] font-medium text-[rgba(255,255,255,0.45)] uppercase tracking-[0.1em]">
            {activeTab === "shortcuts" ? "Keyboard Shortcuts" : activeTab}
          </span>
        </div>

        {/* Tab body */}
        <div className="flex-1 overflow-y-auto px-5 pb-4 pt-1">
          {activeTab === "general" && (
            <GeneralTab draft={appDraft} setDraft={patchApp} />
          )}
          {activeTab === "audio" && (
            <AudioTab draft={appDraft} setDraft={patchApp} />
          )}
          {activeTab === "midi" && (
            <div className="py-12 flex flex-col items-center justify-center opacity-30">
              <p className="text-[11px] text-daw-text-muted">MIDI device management coming soon</p>
            </div>
          )}
          {activeTab === "project" && (
            <ProjectTab projectDraft={projectDraft} setProjectDraft={patchProject} />
          )}
          {activeTab === "shortcuts" && <KeyboardShortcutsPanel />}
          {activeTab === "appearance" && (
            <AppearanceTab draft={appDraft} setDraft={patchApp} />
          )}
          {activeTab === "advanced" && (
            <AdvancedTab
              draft={appDraft}
              setDraft={patchApp}
              onReset={handleResetDefaults}
            />
          )}
        </div>

        {/* Footer */}
        <div className="h-[42px] flex items-center gap-2 px-4 border-t border-[rgba(255,255,255,0.06)] flex-shrink-0">
          <div className="flex-1" />
          <button
            className="px-3 h-[26px] text-[11px] text-[rgba(255,255,255,0.5)] hover:text-[rgba(255,255,255,0.8)] hover:bg-[rgba(255,255,255,0.06)] rounded transition-colors"
            onClick={handleCancel}
          >
            Cancel
          </button>
          <button
            className="px-3 h-[26px] text-[11px] text-[rgba(255,255,255,0.5)] hover:text-[rgba(255,255,255,0.8)] bg-[rgba(255,255,255,0.04)] hover:bg-[rgba(255,255,255,0.08)] border border-[rgba(255,255,255,0.08)] rounded transition-colors"
            onClick={handleApply}
          >
            Apply
          </button>
          <button
            className="px-3 h-[26px] text-[11px] bg-[rgba(114,215,215,0.15)] hover:bg-[rgba(114,215,215,0.22)] text-[rgba(114,215,215,0.9)] border border-[rgba(114,215,215,0.25)] rounded font-medium transition-colors"
            onClick={handleDone}
          >
            Done
          </button>
        </div>
      </div>
    </div>
  );
}
