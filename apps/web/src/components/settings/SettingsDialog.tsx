import { useCallback, useEffect, useRef, useState } from "react";
import { useSettingsStore, APP_SETTINGS_DEFAULTS as DEFAULTS } from "../../store/settingsStore";
import { useProjectStore } from "../../store/projectStore";
import { useWindowStore } from "../../store/windowStore";
import { useDeviceStore } from "../../store/deviceStore";
import { useAudioSettingsStore } from "../../store/audioSettingsStore";
import { midiDeviceService } from "../../engine/MidiDeviceService";
import { platform } from "../../platform";
import { DawSelect } from "../ui/DawSelect";
import { NumberInput } from "../ui/NumberInput";
import { KeyboardShortcutsPanel } from "./KeyboardShortcutsPanel";
import type {
  AppSettings,
  PreferredBufferSize,
  StartupBehavior,
  DauxBackend,
  AudioSampleRate,
  ExtraFolderSetting,
  GraphicRenderingMode,
  VisualFrameRate,
} from "../../store/settingsStore";
import { RefreshCw, AlertCircle, FolderOpen, FolderPlus, Trash2 } from "lucide-react";
import { activeAudioEngine } from "../../engine/activeAudioEngine";
import { useAudioBackendStore } from "../../store/audioBackendStore";
import type {
  DawBridgeSphereDeviceInfo,
  DawBridgeDauxStatus,
} from "../../platform/dawBridge.types";

type SettingsTab = "general" | "audio" | "midi" | "project" | "library" | "appearance" | "advanced" | "shortcuts";

type ProjectDraft = {
  name: string;
  bpm: number;
  timeSignatureNumerator: number;
  timeSignatureDenominator: number;
  sampleRate: number;
};

type Props = { windowId: string; initialTab?: SettingsTab; external?: boolean };

// ── Shared control classes ────────────────────────────────────────────────────

const inputCls =
  "w-full bg-[#111821] border border-white/[0.08] rounded-[5px] px-2 h-[27px] text-[12px] text-daw-text focus:outline-none focus:border-[rgba(114,215,215,0.48)] focus:bg-[#151c25] transition-colors";

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
    <div className="grid min-h-[46px] grid-cols-[minmax(0,1fr)_minmax(150px,240px)] items-center gap-4 border-b border-white/[0.045] py-[7px] last:border-0">
      <div className="flex-1 min-w-0">
        <div className="text-[11.5px] font-medium text-daw-text leading-none">{label}</div>
        {description && (
          <div className="text-[10.5px] text-daw-text-muted mt-1 leading-snug">{description}</div>
        )}
      </div>
      <div className="flex min-w-0 justify-end">{children}</div>
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
      className={`relative inline-flex h-[17px] w-[30px] items-center rounded-full transition-colors focus:outline-none focus:ring-1 focus:ring-[rgba(114,215,215,0.35)] ${
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
    <div className="mt-4 border-b border-white/[0.055] pb-1.5 text-[9.5px] font-semibold uppercase tracking-[0.14em] text-white/34 first:mt-1">
      {children}
    </div>
  );
}

// ── Tab content panels ────────────────────────────────────────────────────────

function MidiTab() {
  const { midiInputs, midiOutputs, midiPermission } = useDeviceStore();
  const audioSettings = useAudioSettingsStore();
  const isWeb = platform.kind === "web";
  const needsPermission = isWeb && (midiPermission === "unknown" || midiPermission === "prompting");
  const denied = isWeb && midiPermission === "denied";
  const unsupported = midiPermission === "unsupported";

  const isInputEnabled = (id: string) =>
    audioSettings.midiEnabledInputIds.length === 0 || audioSettings.midiEnabledInputIds.includes(id);

  return (
    <div className="flex flex-col">
      <SectionHeader>MIDI Inputs</SectionHeader>

      {unsupported && (
        <div className="mb-3 flex items-center gap-2 rounded border border-daw-border bg-daw-bg px-3 py-2">
          <AlertCircle size={11} className="shrink-0 text-daw-faint" />
          <span className="text-[11px] text-daw-faint">Web MIDI is not supported in this browser.</span>
        </div>
      )}

      {needsPermission && (
        <div className="mb-3 flex items-center gap-2 rounded border border-[rgba(114,215,215,0.2)] bg-[rgba(114,215,215,0.04)] px-3 py-2">
          <AlertCircle size={11} className="shrink-0 text-[rgba(114,215,215,0.7)]" />
          <span className="text-[11px] text-[rgba(114,215,215,0.7)]">
            MIDI access not yet granted.
          </span>
          <button
            onClick={() => midiDeviceService.requestMidiAccess()}
            className="ml-auto shrink-0 rounded border border-[rgba(114,215,215,0.3)] px-2 py-0.5 text-[10px] text-[rgba(114,215,215,0.85)] hover:bg-[rgba(114,215,215,0.08)] transition-colors"
          >
            Enable MIDI
          </button>
        </div>
      )}

      {denied && (
        <div className="mb-3 text-[11px] text-red-400/80">
          MIDI access denied — check browser site permissions.
        </div>
      )}

      <div className="flex items-center justify-between mb-2">
        <span className="text-[10px] text-daw-faint">
          {midiInputs.length === 0 ? "No MIDI inputs detected" : `${midiInputs.length} input${midiInputs.length !== 1 ? "s" : ""} found`}
        </span>
        <div className="flex items-center gap-1.5">
          {audioSettings.midiEnabledInputIds.length > 0 && (
            <button
              onClick={() => audioSettings.enableAllMidiInputs()}
              className="text-[10px] text-daw-faint hover:text-daw-text transition-colors"
            >
              Enable All
            </button>
          )}
          <button
            title="Refresh MIDI devices"
            onClick={() => midiDeviceService.refreshMidiDevices()}
            className="flex h-6 w-6 items-center justify-center rounded border border-[rgba(255,255,255,0.08)] text-daw-faint hover:text-daw-text hover:bg-[rgba(255,255,255,0.06)] transition-colors"
          >
            <RefreshCw size={10} />
          </button>
        </div>
      </div>

      {midiInputs.length > 0 && (
        <div className="flex flex-col gap-1 mb-3">
          {midiInputs.map((d) => (
            <div
              key={d.id}
              className="flex items-center gap-2 rounded border border-[rgba(255,255,255,0.07)] bg-[rgba(255,255,255,0.025)] px-2.5 py-1.5"
            >
              <span className="flex-1 min-w-0 truncate text-[11px] text-daw-text">{d.name}</span>
              <SettingsToggle
                checked={isInputEnabled(d.id)}
                onChange={() => audioSettings.toggleMidiInput(d.id)}
              />
            </div>
          ))}
        </div>
      )}

      <SectionHeader>MIDI Outputs</SectionHeader>
      <div className="flex flex-col gap-1">
        {midiOutputs.length === 0 ? (
          <span className="text-[10px] text-daw-faint py-1">No MIDI outputs detected</span>
        ) : (
          midiOutputs.map((d) => (
            <div
              key={d.id}
              className="flex items-center gap-2 rounded border border-[rgba(255,255,255,0.07)] bg-[rgba(255,255,255,0.025)] px-2.5 py-1.5"
            >
              <span className="flex-1 min-w-0 truncate text-[11px] text-daw-text">{d.name}</span>
              <SettingsToggle
                checked={audioSettings.midiEnabledOutputIds.includes(d.id)}
                onChange={() => audioSettings.toggleMidiOutput(d.id)}
              />
            </div>
          ))
        )}
      </div>
    </div>
  );
}

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

// ── Backend ID helpers ────────────────────────────────────────────────────────

/** Maps our UI DauxBackend enum to the Rust openDaux() backendId string. */
function dauxBackendId(b: DauxBackend): string {
  switch (b) {
    case "wasapi":           return "wasapi-shared";
    case "wasapi-exclusive": return "wasapi-exclusive";
    case "mme":              return "mme";
    case "coreaudio":        return "coreaudio";
    case "alsa":             return "alsa";
  }
}

/** Returns the platform default DAUx backend based on `window.dawElectron.platform`. */
function platformDefaultBackend(): DauxBackend {
  const p = window.dawElectron?.platform ?? "";
  if (p === "darwin") return "coreaudio";
  if (p === "linux")  return "alsa";
  return "wasapi"; // win32 + unknown; maps to WASAPI Shared for Settings stability
}

// ── Restart banner ────────────────────────────────────────────────────────────

function RestartBanner() {
  return (
    <div className="mb-2 flex items-center gap-2 rounded border border-[rgba(226,184,102,0.22)] bg-[rgba(226,184,102,0.06)] px-3 py-[7px]">
      <span className="text-[10px] text-[#e2b866]/80">⚠</span>
      <span className="text-[11px] text-[#e2b866]/80 leading-snug">
        Audio settings changed — click Apply or Done to restart the engine.
      </span>
    </div>
  );
}

// ── DAUx engine status card ───────────────────────────────────────────────────

function DauxEngineCard({
  status,
  pendingBackend,
  onRefresh,
}: {
  status: DawBridgeDauxStatus | null;
  /** Backend the user has selected but not yet applied. Null = no pending change. */
  pendingBackend?: string | null;
  onRefresh: () => void;
}) {
  const [testToneOn, setTestToneOn] = useState(false);
  const running = !!status && status.sampleRate > 0;

  const toggleTestTone = () => {
    const sphere = window.dawElectron?.sphereAudio;
    if (!sphere) return;
    const next = !testToneOn;
    setTestToneOn(next);
    void sphere.setTestTone(next, 440).catch(() => setTestToneOn(false));
  };

  return (
    <div className="mb-3 rounded border border-[rgba(255,255,255,0.07)] bg-[rgba(255,255,255,0.025)] px-3 py-2.5">
      {/* Header */}
      <div className="flex items-center gap-2 mb-1.5">
        <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${
          running
            ? "bg-[rgba(74,222,128,0.9)]"
            : status
            ? "bg-yellow-400/80"
            : "bg-[rgba(255,255,255,0.18)] animate-pulse"
        }`} />
        <span className="text-[11px] font-medium text-daw-text">DAUx</span>
        {status?.backendName && (
          <span className="text-[10px] text-daw-faint">· {status.backendName}</span>
        )}
        {/* Pending backend indicator */}
        {pendingBackend && (
          <span className="text-[10px] text-[#e2b866]/80">→ {pendingBackend}</span>
        )}
        <span className="ml-auto text-[10px] text-daw-faint">
          {running ? "Running" : status ? "Stopped" : "Detecting…"}
        </span>
        {running && (
          <button
            title={testToneOn ? "Stop test tone (440 Hz)" : "Play 440 Hz test tone"}
            onClick={toggleTestTone}
            className={`flex h-5 items-center gap-1 px-1.5 rounded border text-[9px] transition-colors ${
              testToneOn
                ? "border-[rgba(74,222,128,0.4)] bg-[rgba(74,222,128,0.12)] text-[rgba(74,222,128,0.9)]"
                : "border-[rgba(255,255,255,0.08)] text-daw-faint hover:text-daw-text hover:bg-[rgba(255,255,255,0.06)]"
            }`}
          >
            {testToneOn ? "■ Stop" : "♪ Test"}
          </button>
        )}
        <button
          title="Refresh engine status"
          onClick={onRefresh}
          className="flex h-5 w-5 items-center justify-center rounded border border-[rgba(255,255,255,0.08)] text-daw-faint hover:text-daw-text hover:bg-[rgba(255,255,255,0.06)] transition-colors"
        >
          <RefreshCw size={9} />
        </button>
      </div>

      {/* Last error */}
      {status?.lastError && (
        <div className="mb-1.5 flex items-start gap-1.5 rounded border border-red-500/20 bg-red-500/[0.07] px-2 py-1">
          <AlertCircle size={10} className="mt-px shrink-0 text-red-400/70" />
          <span className="text-[10px] leading-snug text-red-300/80">{status.lastError}</span>
        </div>
      )}

      {/* Stats grid */}
      {status ? (
        <div className="grid grid-cols-2 gap-x-4 gap-y-0.5">
          {status.bufferSize > 0 && (
            <>
              <span className="text-[10px] text-daw-faint">Buffer</span>
              <span className="text-[10px] text-daw-text tabular-nums">
                {status.bufferSize} samples
                {status.estimatedLatencyMs > 0 && (
                  <span className="text-daw-faint ml-1.5">
                    ≈ {status.estimatedLatencyMs.toFixed(1)} ms
                  </span>
                )}
              </span>
            </>
          )}
          {status.sampleRate > 0 && (
            <>
              <span className="text-[10px] text-daw-faint">Sample Rate</span>
              <span className="text-[10px] text-daw-text tabular-nums">
                {status.sampleRate % 1000 === 0
                  ? `${status.sampleRate / 1000} kHz`
                  : `${(status.sampleRate / 1000).toFixed(1)} kHz`}
              </span>
            </>
          )}
          {status.outputDevice && (
            <>
              <span className="text-[10px] text-daw-faint">Output</span>
              <span className="text-[10px] text-daw-text truncate" title={status.outputDevice}>
                {status.outputDevice}
              </span>
            </>
          )}
          {status.glitchCount > 0 && (
            <>
              <span className="text-[10px] text-daw-faint">Glitches</span>
              <span className="text-[10px] text-red-400/90 tabular-nums">{status.glitchCount}</span>
            </>
          )}
          {status.mmcssActive && (
            <>
              <span className="text-[10px] text-daw-faint">MMCSS</span>
              <span className="text-[10px] text-[rgba(74,222,128,0.8)]">Active</span>
            </>
          )}
        </div>
      ) : (
        <p className="text-[10px] text-daw-faint">
          DAUx engine not detected — ensure the native addon is loaded.
        </p>
      )}
    </div>
  );
}

// ── WASM engine status card ───────────────────────────────────────────────────

function WasmEngineCard() {
  const backend = useAudioBackendStore();
  const running = backend.healthy && backend.active !== null;

  return (
    <div className="mb-3 rounded border border-[rgba(255,255,255,0.07)] bg-[rgba(255,255,255,0.025)] px-3 py-2.5">
      <div className="flex items-center gap-2 mb-1.5">
        <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${
          running ? "bg-[rgba(74,222,128,0.9)]" : "bg-yellow-400/80"
        }`} />
        <span className="text-[11px] font-medium text-daw-text">WASM</span>
        <span className="text-[10px] text-daw-faint">· Browser AudioWorklet</span>
        <span className="ml-auto text-[10px] text-daw-faint">
          {running ? "Running" : (backend.error ?? "Not running")}
        </span>
      </div>
      <div className="grid grid-cols-2 gap-x-4 gap-y-0.5">
        <span className="text-[10px] text-daw-faint">Buffer control</span>
        <span className="text-[10px] text-daw-text">Browser managed</span>
        {backend.contextState && (
          <>
            <span className="text-[10px] text-daw-faint">Context</span>
            <span className="text-[10px] text-daw-text">{backend.contextState}</span>
          </>
        )}
        {backend.fallbackReason && (
          <>
            <span className="text-[10px] text-daw-faint">Fallback</span>
            <span className="text-[10px] text-yellow-400/80">{backend.fallbackReason}</span>
          </>
        )}
      </div>
    </div>
  );
}

// ── AudioTab ──────────────────────────────────────────────────────────────────

function AudioTab({
  draft,
  setDraft,
  dauxStatus,
  onRefreshDauxStatus,
}: {
  draft: AppSettings;
  setDraft: (p: Partial<AppSettings>) => void;
  dauxStatus: DawBridgeDauxStatus | null;
  onRefreshDauxStatus: () => void;
}) {
  const audioSettings = useAudioSettingsStore();
  const isElectron = platform.kind === "electron";
  const { audioInputs, audioOutputs } = useDeviceStore();

  // OS platform from Electron bridge (win32 / darwin / linux)
  const osPlatform = (window.dawElectron?.platform ?? "") as string;
  const isWindows = osPlatform === "win32";
  const isMac     = osPlatform === "darwin";
  const isLinux   = osPlatform === "linux";

  // ── Native device lists ───────────────────────────────────────────────────
  const [nativeInputs,  setNativeInputs]  = useState<DawBridgeSphereDeviceInfo[]>([]);
  const [nativeOutputs, setNativeOutputs] = useState<DawBridgeSphereDeviceInfo[]>([]);
  const [devicesRefreshing, setDevicesRefreshing] = useState(false);

  const refreshDevices = useCallback(async () => {
    const sphere = window.dawElectron?.sphereAudio;
    if (!sphere) { setNativeInputs([]); setNativeOutputs([]); return; }
    setDevicesRefreshing(true);
    try {
      const [inputs, outputs] = await Promise.all([
        sphere.listInputDevices(),
        sphere.listOutputDevices(),
      ]);
      setNativeInputs(inputs);
      setNativeOutputs(outputs);
    } catch {
      setNativeInputs([]);
      setNativeOutputs([]);
    } finally {
      setDevicesRefreshing(false);
    }
  }, []);

  useEffect(() => {
    if (isElectron) void refreshDevices();
  }, [isElectron, refreshDevices]);

  // ── OS Backend options (Electron-only, platform-conditional) ─────────────
  const backendOptions: { value: DauxBackend; label: string }[] = isWindows
    ? [
        { value: "wasapi",           label: "WASAPI Shared (Stable)" },
        { value: "wasapi-exclusive", label: "WASAPI Exclusive (Experimental)" },
        { value: "mme",              label: "MME Fallback (High Latency)" },
      ]
    : isMac
    ? [{ value: "coreaudio", label: "CoreAudio" }]
    : isLinux
    ? [{ value: "alsa", label: "ALSA" }]
    : [];

  const defaultBackend: DauxBackend = isMac ? "coreaudio" : isLinux ? "alsa" : "wasapi";
  const effectiveDauxBackend: DauxBackend = draft.dauxBackend ?? defaultBackend;
  const effectiveSampleRate: AudioSampleRate = draft.audioSampleRate ?? "device-default";

  // ── Restart-required detection ────────────────────────────────────────────
  // Compares draft against the last-committed store values.
  const storedBufferSize  = useSettingsStore((s) => s.preferredBufferSize);
  const storedDauxBackend = useSettingsStore((s) => s.dauxBackend ?? defaultBackend);
  const storedSampleRate  = useSettingsStore((s) => s.audioSampleRate ?? "device-default");
  const restartRequired = isElectron && (
    draft.preferredBufferSize !== storedBufferSize ||
    effectiveDauxBackend      !== storedDauxBackend ||
    effectiveSampleRate       !== storedSampleRate
  );

  // ── Device select options ─────────────────────────────────────────────────
  const inputOptions = [
    { value: "__default__", label: "System Default" },
    ...(nativeInputs.length > 0 ? nativeInputs.map((d) => ({
      value: d.id,
      label: d.isDefault ? `${d.name} (Default)` : d.name,
    })) : audioInputs.map((d) => ({
      value: d.id,
      label: d.isDefault ? `${d.name} (Default)` : d.name,
    }))),
  ];
  const inputDeviceValue =
    audioSettings.audioInputDeviceId &&
    inputOptions.some((o) => o.value === audioSettings.audioInputDeviceId)
      ? audioSettings.audioInputDeviceId
      : "__default__";

  const outputOptions = [
    { value: "__default__", label: "System Default" },
    ...(nativeOutputs.length > 0 ? nativeOutputs.map((d) => ({
      value: d.id,
      label: d.isDefault ? `${d.name} (Default)` : d.name,
    })) : audioOutputs.map((d) => ({
      value: d.id,
      label: d.isDefault ? `${d.name} (Default)` : d.name,
    }))),
  ];
  const outputDeviceValue =
    audioSettings.audioOutputDeviceId &&
    outputOptions.some((o) => o.value === audioSettings.audioOutputDeviceId)
      ? audioSettings.audioOutputDeviceId
      : "__default__";

  return (
    <div className="flex flex-col">

      {/* ── 1. Engine (read-only) ──────────────────────────────────────────── */}
      <SectionHeader>Audio Engine</SectionHeader>
      <SettingsRow
        label="Engine"
        description={
          isElectron
            ? "Low-latency native audio via DAUx (Rust)"
            : "Browser AudioWorklet with WASM DSP"
        }
      >
        <span className="inline-flex items-center h-[22px] px-2.5 rounded text-[11px] font-medium bg-[rgba(114,215,215,0.1)] border border-[rgba(114,215,215,0.2)] text-[rgba(114,215,215,0.9)]">
          {isElectron ? "DAUx" : "WASM"}
        </span>
      </SettingsRow>

      {isElectron
        ? (
          <DauxEngineCard
            status={dauxStatus}
            pendingBackend={
              restartRequired && effectiveDauxBackend !== storedDauxBackend
                ? backendOptions.find((b) => b.value === effectiveDauxBackend)?.label ?? null
                : null
            }
            onRefresh={onRefreshDauxStatus}
          />
        )
        : <WasmEngineCard />
      }

      {/* ── 2. OS Backend (Electron only) ─────────────────────────────────── */}
      {isElectron && backendOptions.length > 0 && (
        <>
          <SectionHeader>OS Backend</SectionHeader>
          <SettingsRow
            label="Backend"
            description={
              isWindows
                ? "Shared is stable for most devices. Exclusive is lower latency but can fail on some drivers."
                : isMac
                ? "CoreAudio is the only supported backend on macOS."
                : "ALSA is the system audio API on Linux."
            }
          >
            <DawSelect
              className="w-52"
              value={effectiveDauxBackend}
              onChange={(v) => setDraft({ dauxBackend: v as DauxBackend })}
              options={backendOptions}
            />
          </SettingsRow>
        </>
      )}

      {/* ── 3. Performance ────────────────────────────────────────────────── */}
      <SectionHeader>Performance</SectionHeader>

      {restartRequired && <RestartBanner />}

      <SettingsRow
        label="Buffer Size"
        description={
          isElectron
            ? "Lower = less latency, higher CPU. 256 is the stable default."
            : "Buffer size is controlled by the browser."
        }
      >
        {isElectron ? (
          <SettingsSelect<PreferredBufferSize>
            value={draft.preferredBufferSize}
            onChange={(v) => setDraft({ preferredBufferSize: v })}
            options={[
              { value: 64,   label: "64 samples" },
              { value: 128,  label: "128 samples" },
              { value: 256,  label: "256 samples (Default)" },
              { value: 512,  label: "512 samples" },
              { value: 1024, label: "1024 samples" },
            ]}
          />
        ) : (
          <span className="text-[11px] text-daw-faint">Browser managed</span>
        )}
      </SettingsRow>

      <SettingsRow
        label="Sample Rate"
        description="Hardware sample rate. Device Default lets the driver choose."
      >
        {isElectron ? (
          <DawSelect
            className="w-44"
            value={String(effectiveSampleRate)}
            onChange={(val) => {
              if (val === "device-default") {
                setDraft({ audioSampleRate: "device-default" });
              } else {
                const n = parseInt(val, 10);
                if (n === 44100 || n === 48000 || n === 96000) {
                  setDraft({ audioSampleRate: n as AudioSampleRate });
                }
              }
            }}
            options={[
              { value: "device-default", label: "Device Default" },
              { value: "44100",          label: "44100 Hz" },
              { value: "48000",          label: "48000 Hz" },
              { value: "96000",          label: "96000 Hz" },
            ]}
          />
        ) : (
          <span className="text-[11px] text-daw-faint">Browser managed</span>
        )}
      </SettingsRow>

      {/* ── 4. Device ─────────────────────────────────────────────────────── */}
      <SectionHeader>Device</SectionHeader>

      {isElectron ? (
        <>
          <SettingsRow label="Input Device" description="Hardware input for recording (microphone, interface)">
            <div className="flex items-center gap-1.5">
              <DawSelect
                className="w-44"
                value={inputDeviceValue}
                onChange={(v) =>
                  audioSettings.setAudioInputDevice(v === "__default__" ? null : v)
                }
                options={inputOptions}
              />
              <button
                title="Refresh device list"
                onClick={() => void refreshDevices()}
                className={`flex h-7 w-7 items-center justify-center rounded border border-[rgba(255,255,255,0.08)] text-daw-faint hover:text-daw-text hover:bg-[rgba(255,255,255,0.06)] transition-colors ${
                  devicesRefreshing ? "opacity-50 pointer-events-none" : ""
                }`}
              >
                <RefreshCw
                  size={11}
                  className={devicesRefreshing ? "animate-spin" : ""}
                />
              </button>
            </div>
          </SettingsRow>
          <SettingsRow label="Output Device" description="Hardware output for the master bus">
            <div className="flex items-center gap-1.5">
              <DawSelect
                className="w-44"
                value={outputDeviceValue}
                onChange={(v) =>
                  audioSettings.setAudioOutputDevice(v === "__default__" ? null : v)
                }
                options={outputOptions}
              />
              <button
                title="Refresh device list"
                onClick={() => void refreshDevices()}
                className={`flex h-7 w-7 items-center justify-center rounded border border-[rgba(255,255,255,0.08)] text-daw-faint hover:text-daw-text hover:bg-[rgba(255,255,255,0.06)] transition-colors ${
                  devicesRefreshing ? "opacity-50 pointer-events-none" : ""
                }`}
              >
                <RefreshCw
                  size={11}
                  className={devicesRefreshing ? "animate-spin" : ""}
                />
              </button>
            </div>
          </SettingsRow>
        </>
      ) : (
        <SettingsRow
          label="Output Device"
          description="Output device is selected and managed by the browser."
        >
          <span className="text-[11px] text-daw-faint">Browser managed</span>
        </SettingsRow>
      )}

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
        <NumberInput
          className="w-24 !h-[28px]"
          value={projectDraft.bpm}
          min={40}
          max={300}
          ariaLabel="Tempo BPM"
          onChange={(value) => setProjectDraft({ bpm: Math.max(40, value) })}
        />
      </SettingsRow>
      <SettingsRow label="Time Signature">
        <div className="flex items-center gap-1.5">
          <NumberInput
            className="w-14 !h-[28px]"
            align="center"
            value={projectDraft.timeSignatureNumerator}
            min={1}
            max={32}
            ariaLabel="Time signature numerator"
            onChange={(value) => setProjectDraft({ timeSignatureNumerator: Math.max(1, value) })}
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
  const isElectron = platform.kind === "electron";
  const storedGraphicMode = useSettingsStore((s) => s.graphicRenderingMode);
  const graphicModeDirty = draft.graphicRenderingMode !== storedGraphicMode;
  const [gpuName, setGpuName] = useState<string>("Detecting GPU...");
  const [gpuBackend, setGpuBackend] = useState<string>("");

  useEffect(() => {
    if (!isElectron) return;
    let cancelled = false;
    window.dawElectron?.sys.getGpuInfo()
      .then((info) => {
        if (cancelled) return;
        setGpuName(info.gpuDescription ?? "Unknown GPU");
        const webgl = info.features.webgl ?? "unknown";
        const raster = info.features.rasterization ?? "unknown";
        setGpuBackend(`WebGL ${webgl} / Raster ${raster}`);
      })
      .catch(() => {
        if (!cancelled) setGpuName("Unknown GPU");
      });
    return () => {
      cancelled = true;
    };
  }, [isElectron]);

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

      {isElectron && (
        <>
          <SectionHeader>Graphics</SectionHeader>
          <SettingsRow
            label="Timeline FPS"
            description="Caps visual refresh for timeline, meters, playhead, and diagnostics. Unlimited follows the display refresh rate."
          >
            <SettingsSelect<VisualFrameRate>
              value={draft.visualFrameRate}
              onChange={(v) => setDraft({ visualFrameRate: v })}
              options={[
                { value: 45, label: "45 FPS" },
                { value: 60, label: "60 FPS" },
                { value: 120, label: "120 FPS" },
                { value: "unlimited", label: "Unlimited" },
              ]}
            />
          </SettingsRow>
          <SettingsRow
            label="GPU Device"
            description={gpuBackend || "Renderer device currently reported by Electron/Chromium."}
          >
            <div className="min-w-0 text-right text-[11px] font-medium text-daw-text">
              <span className="block truncate" title={gpuName}>{gpuName}</span>
            </div>
          </SettingsRow>
          <SettingsRow
            label="Graphic Rendering Mode"
            description="Auto uses Electron defaults with ANGLE D3D11 on Windows. Force GPU bypasses conservative GPU blocking. Software Rendering is safest for unstable drivers."
          >
            <div className="flex flex-col items-end gap-1.5">
              <SettingsSelect<GraphicRenderingMode>
                value={draft.graphicRenderingMode}
                onChange={(v) => setDraft({ graphicRenderingMode: v })}
                options={[
                  { value: "auto",     label: "GPU Rendering (Auto)" },
                  { value: "force",    label: "Force GPU (ANGLE D3D11)" },
                  { value: "software", label: "Software Rendering" },
                ]}
              />
              <span className={`flex items-center gap-1 text-[9.5px] transition-colors ${
                graphicModeDirty
                  ? "text-[#e2b866]/90"
                  : "text-white/30"
              }`}>
                <span>⚠</span>
                Restart Required
              </span>
            </div>
          </SettingsRow>
        </>
      )}
    </div>
  );
}

function basenameFromPath(path: string): string {
  return path.replace(/\\/g, "/").split("/").filter(Boolean).pop() ?? path;
}

function ExtraFoldersTab({
  draft,
  setDraft,
}: {
  draft: AppSettings;
  setDraft: (p: Partial<AppSettings>) => void;
}) {
  const [indexingPath, setIndexingPath] = useState<string | null>(null);
  const isElectron = platform.kind === "electron";

  const updateFolders = (folders: ExtraFolderSetting[]) => setDraft({ extraFolders: folders });

  const addFolder = async () => {
    if (!isElectron) return;
    const path = await platform.folderProject.browseLocation();
    if (!path) return;
    const existing = draft.extraFolders.find((folder) => folder.path === path);
    if (existing) {
      updateFolders(
        draft.extraFolders.map((folder) =>
          folder.path === path ? { ...folder, enabled: true } : folder
        ),
      );
      return;
    }
    updateFolders([
      ...draft.extraFolders,
      {
        id: crypto.randomUUID(),
        name: basenameFromPath(path),
        path,
        enabled: true,
        addedAt: Date.now(),
      },
    ]);
  };

  const toggleFolder = (id: string, enabled: boolean) => {
    updateFolders(draft.extraFolders.map((folder) => folder.id === id ? { ...folder, enabled } : folder));
  };

  const removeFolder = (id: string) => {
    updateFolders(draft.extraFolders.filter((folder) => folder.id !== id));
  };

  const indexFolder = async (path: string) => {
    if (!isElectron) return;
    setIndexingPath(path);
    try {
      await platform.fileSystem.browserIndexStart(path);
    } finally {
      setIndexingPath(null);
    }
  };

  return (
    <div className="flex flex-col">
      <SectionHeader>Browser Library</SectionHeader>
      <SettingsRow
        label="Extra Folders"
        description="Pinned folders for the Browser panel and background audio indexing."
      >
        <button
          type="button"
          disabled={!isElectron}
          onClick={() => { void addFolder(); }}
          className="inline-flex h-[27px] items-center gap-1.5 rounded-[5px] border border-[rgba(114,215,215,0.28)] bg-[rgba(114,215,215,0.08)] px-2.5 text-[11px] font-medium text-[rgba(114,215,215,0.88)] transition-colors hover:bg-[rgba(114,215,215,0.13)] disabled:cursor-not-allowed disabled:border-white/[0.08] disabled:bg-white/[0.025] disabled:text-white/30"
        >
          <FolderPlus size={12} />
          Add Folder
        </button>
      </SettingsRow>

      <div className="mt-2 overflow-hidden rounded-[7px] border border-white/[0.06] bg-black/[0.12]">
        {draft.extraFolders.length === 0 ? (
          <div className="flex h-[70px] items-center justify-center px-3 text-[10.5px] text-white/30">
            {isElectron ? "No extra folders added." : "Extra folders are available in the Electron app."}
          </div>
        ) : (
          draft.extraFolders.map((folder) => (
            <div
              key={folder.id}
              className="grid min-h-[42px] grid-cols-[minmax(0,1fr)_auto] items-center gap-3 border-b border-white/[0.045] px-2.5 py-1.5 last:border-0"
            >
              <div className="flex min-w-0 items-center gap-2">
                <FolderOpen size={13} className="shrink-0 text-[rgba(114,215,215,0.56)]" />
                <div className="min-w-0">
                  <div className="truncate text-[11px] font-medium text-white/76">{folder.name}</div>
                  <div className="truncate text-[9.5px] text-white/30" title={folder.path}>{folder.path}</div>
                </div>
              </div>
              <div className="flex items-center gap-1.5">
                <SettingsToggle
                  checked={folder.enabled}
                  onChange={(enabled) => toggleFolder(folder.id, enabled)}
                />
                <button
                  type="button"
                  title="Index folder"
                  disabled={!folder.enabled || indexingPath === folder.path}
                  onClick={() => { void indexFolder(folder.path); }}
                  className="flex h-[24px] w-[24px] items-center justify-center rounded-[5px] border border-white/[0.08] text-white/40 transition-colors hover:bg-white/[0.055] hover:text-white/70 disabled:opacity-35"
                >
                  <RefreshCw size={11} className={indexingPath === folder.path ? "animate-spin" : ""} />
                </button>
                <button
                  type="button"
                  title="Remove folder"
                  onClick={() => removeFolder(folder.id)}
                  className="flex h-[24px] w-[24px] items-center justify-center rounded-[5px] border border-white/[0.08] text-white/32 transition-colors hover:border-red-400/25 hover:bg-red-500/[0.08] hover:text-red-300/80"
                >
                  <Trash2 size={11} />
                </button>
              </div>
            </div>
          ))
        )}
      </div>

      <SectionHeader>Indexing</SectionHeader>
      <SettingsRow
        label="Index On Open"
        description="Enabled folders are shown in Browser and can be indexed from their row or tree."
      >
        <span className="text-[10.5px] text-white/34">Manual per folder</span>
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

export function SettingsDialog({ windowId, initialTab = "general", external = false }: Props) {
  const store = useSettingsStore();
  const { project } = useProjectStore();
  const { closeWindow, updateWindowPayload } = useWindowStore();
  const audioSettings = useAudioSettingsStore();

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
    startupBehavior:      store.startupBehavior,
    autoSave:             store.autoSave,
    autoSaveIntervalMin:  store.autoSaveIntervalMin,
    preferredEngine:      store.preferredEngine,
    preferredBufferSize:  store.preferredBufferSize,
    dauxBackend:          store.dauxBackend,
    audioSampleRate:      store.audioSampleRate,
    extraFolders:         store.extraFolders,
    compactUI:            store.compactUI,
    enableDevTools:       store.enableDevTools,
    graphicRenderingMode: store.graphicRenderingMode,
    visualFrameRate:      store.visualFrameRate,
  });

  const patchProject = (p: Partial<ProjectDraft>) => setProjectDraft((s) => ({ ...s, ...p }));
  const patchApp = (p: Partial<AppSettings>) => setAppDraft((s) => ({ ...s, ...p }));

  // ── DAUx runtime status (refreshed on mount and after Apply) ─────────────
  const [dauxStatus, setDauxStatus] = useState<DawBridgeDauxStatus | null>(null);
  const [applyError, setApplyError] = useState<string | null>(null);
  const [applying, setApplying] = useState(false);

  const refreshDauxStatus = useCallback(() => {
    const sphere = window.dawElectron?.sphereAudio;
    if (!sphere) { setDauxStatus(null); return; }
    void sphere.getDauxStatus()
      .then(setDauxStatus)
      .catch(() => setDauxStatus(null));
  }, []);

  // Probe once on mount (Electron only)
  useEffect(() => {
    if (platform.kind === "electron") refreshDauxStatus();
  }, [refreshDauxStatus]);

  // Track whether a native engine reopen is in flight
  const nativeApplyingRef = useRef(false);

  const handleResetDefaults = () => {
    if (confirm("Reset all settings to defaults? This cannot be undone.")) {
      store.resetToDefaults();
      setAppDraft(DEFAULTS);
    }
  };

  const handleApply = async (): Promise<boolean> => {
    const isElectron = platform.kind === "electron";
    if (applying || nativeApplyingRef.current) return false;
    setApplyError(null);
    setApplying(true);

    // In Electron, engine is always DAUx (native-sphere-direct).
    const effectiveAppDraft: AppSettings = isElectron
      ? { ...appDraft, preferredEngine: "native-sphere-direct" }
      : appDraft;

    store.applySettings(effectiveAppDraft);
    if (effectiveAppDraft.preferredEngine !== appDraft.preferredEngine) {
      setAppDraft(effectiveAppDraft);
    }

    // Persist graphicRenderingMode to settings.json so Electron reads it on next startup.
    if (isElectron) {
      void window.dawElectron?.sys.writeElectronSettings({
        graphicRenderingMode: effectiveAppDraft.graphicRenderingMode,
      });
    }

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

    // ── Electron: restart DAUx with new settings ───────────────────────────
    const sphere = window.dawElectron?.sphereAudio;
    if (isElectron && sphere && !nativeApplyingRef.current) {
      nativeApplyingRef.current = true;

      const outputDeviceId = audioSettings.audioOutputDeviceId ?? undefined;
      const backend   = effectiveAppDraft.dauxBackend ?? platformDefaultBackend();
      const backendId = dauxBackendId(backend);
      const sr = effectiveAppDraft.audioSampleRate;
      const sampleRate = (sr === "device-default" || sr == null) ? undefined : (sr as number);

      try {
        // Use openDauxSafe so Rust handles the fallback internally —
        // if exclusive fails, the engine restores the previous backend and
        // the error propagates here with a description of what happened.
        const openFn = backendId === "wasapi-exclusive"
          ? sphere.openDauxSafe.bind(sphere)
          : sphere.openDaux.bind(sphere);

        await openFn({
          backendId,
          outputDeviceId: outputDeviceId || undefined,
          bufferSize: effectiveAppDraft.preferredBufferSize,
          sampleRate,
          mmcssPriority: true,
          safeMode: false,
        });
        await sphere.start();
        await activeAudioEngine.reconfigure("native-sphere-direct");
        refreshDauxStatus();
        return true;
      } catch (e: unknown) {
        console.warn("[Settings] DAUx restart failed:", e);
        const message = e instanceof Error ? e.message : String(e);
        // If openDauxSafe threw, the previous backend was already restored in
        // Rust — just show the error and refresh status to reflect that.
        setApplyError(message);
        try {
          await sphere.start();
          await activeAudioEngine.reconfigure("native-sphere-direct");
        } catch {
          // Best-effort: if start also fails, the stream stays closed.
        }
        refreshDauxStatus();
        return false;
      } finally {
        nativeApplyingRef.current = false;
        setApplying(false);
      }
    } else if (!isElectron) {
      // Web: reconfigure WASM engine
      try {
        await activeAudioEngine.reconfigure(effectiveAppDraft.preferredEngine);
        return true;
      } catch (e: unknown) {
        console.warn("[Settings] Web audio reconfigure failed:", e);
        setApplyError(e instanceof Error ? e.message : String(e));
        return false;
      } finally {
        setApplying(false);
      }
    }

    setApplying(false);
    return true;
  };

  const closeSelf = () => {
    if (external && platform.kind === "electron") platform.window.close();
    else closeWindow(windowId);
  };
  const handleCancel = () => closeSelf();
  const handleDone = async () => {
    const ok = await handleApply();
    if (ok) closeSelf();
  };

  const tabs: { id: SettingsTab; label: string }[] = [
    { id: "general",   label: "General"   },
    { id: "audio",     label: "Audio"     },
    { id: "midi",      label: "MIDI"      },
    { id: "project",   label: "Project"   },
    { id: "library",   label: "Library"   },
    { id: "shortcuts", label: "Shortcuts" },
    { id: "appearance",label: "Appearance"},
    { id: "advanced",  label: "Advanced"  },
  ];

  return (
    <div className={`flex h-full w-full overflow-hidden bg-[#0e1319] select-none ${external ? "" : "border border-white/[0.07] shadow-2xl"}`}>
      {/* Sidebar */}
      <div className="flex w-[166px] flex-shrink-0 flex-col border-r border-white/[0.06] bg-[#0a0e13] px-1.5 py-2">
        {tabs.map((tab) => (
          <button
            key={tab.id}
            onClick={() => setActiveTab(tab.id)}
            className={`relative mb-[2px] flex h-[29px] items-center rounded-[5px] px-3 text-left text-[11.5px] font-medium transition-colors focus:outline-none focus:ring-1 focus:ring-[rgba(114,215,215,0.28)] ${
              activeTab === tab.id
                ? "text-[rgba(190,245,245,0.92)] bg-[rgba(114,215,215,0.10)]"
                : "text-white/42 hover:text-white/70 hover:bg-white/[0.045]"
            }`}
          >
            {activeTab === tab.id && (
              <span className="absolute left-0 top-[6px] bottom-[6px] w-[2px] rounded-r bg-[rgba(114,215,215,0.85)]" />
            )}
            {tab.label}
          </button>
        ))}
      </div>

      {/* Content area */}
      <div className="flex-1 flex flex-col min-w-0 overflow-hidden">
        {/* Header */}
        <div className="h-[34px] flex items-center px-5 flex-shrink-0 border-b border-white/[0.055] bg-white/[0.012]">
          <span className="text-[10px] font-semibold text-white/42 uppercase tracking-[0.14em]">
            {activeTab === "shortcuts" ? "Keyboard Shortcuts" : activeTab}
          </span>
        </div>

        {/* Tab body */}
        <div className="flex-1 overflow-y-auto px-5 pb-4 pt-1.5">
          {applyError && (
            <div className="mt-3 rounded border border-red-500/25 bg-red-500/10 px-3 py-2 text-[11px] leading-snug text-red-300">
              {applyError}
            </div>
          )}
          {activeTab === "general" && (
            <GeneralTab draft={appDraft} setDraft={patchApp} />
          )}
          {activeTab === "audio" && (
            <AudioTab
              draft={appDraft}
              setDraft={patchApp}
              dauxStatus={dauxStatus}
              onRefreshDauxStatus={refreshDauxStatus}
            />
          )}
          {activeTab === "midi" && <MidiTab />}
          {activeTab === "project" && (
            <ProjectTab projectDraft={projectDraft} setProjectDraft={patchProject} />
          )}
          {activeTab === "library" && (
            <ExtraFoldersTab draft={appDraft} setDraft={patchApp} />
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
        <div className="h-[42px] flex items-center gap-2 px-4 border-t border-white/[0.06] bg-[#0b1016] flex-shrink-0">
          <div className="flex-1" />
          <button
            className="px-3 h-[26px] rounded-[5px] text-[11px] text-white/50 transition-colors hover:bg-white/[0.055] hover:text-white/78"
            onClick={handleCancel}
          >
            Cancel
          </button>
          <button
            className={`px-3 h-[26px] text-[11px] text-white/55 bg-white/[0.04] border border-white/[0.08] rounded-[5px] transition-colors ${
              applying ? "opacity-50 cursor-wait" : "hover:text-[rgba(255,255,255,0.8)] hover:bg-[rgba(255,255,255,0.08)]"
            }`}
            disabled={applying}
            onClick={() => { void handleApply(); }}
          >
            {applying ? "Applying…" : "Apply"}
          </button>
          <button
            className={`px-3 h-[26px] text-[11px] bg-[rgba(114,215,215,0.14)] text-[rgba(114,215,215,0.9)] border border-[rgba(114,215,215,0.28)] rounded-[5px] font-medium transition-colors ${
              applying ? "opacity-50 cursor-wait" : "hover:bg-[rgba(114,215,215,0.22)]"
            }`}
            disabled={applying}
            onClick={() => { void handleDone(); }}
          >
            Done
          </button>
        </div>
      </div>
    </div>
  );
}
