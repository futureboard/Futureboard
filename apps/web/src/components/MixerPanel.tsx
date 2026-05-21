import {
  ChevronDown, Minus, Plus, SlidersHorizontal, X,
  Activity, Waves, Sparkles, AudioLines, Gauge, Boxes, Plug,
  Send, FolderPlus, CornerDownLeft, GitMerge, ExternalLink,
} from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "./ui/menu";
import { forwardRef, useRef, useState, useEffect, type ButtonHTMLAttributes } from "react";
import { useProjectStore } from "../store/projectStore";
import { useUIStore } from "../store/uiStore";
import { useHistoryStore } from "../store/historyStore";
import { SetTrackVolumeCommand, SetTrackPanCommand, SetTrackMuteCommand, SetTrackSoloCommand, SetTrackPreviewModeCommand } from "../commands";
import { activeAudioEngine } from "../engine/activeAudioEngine";
import { Knob } from "./ui/Knob";
import { MixerFader } from "./ui/MixerFader";
import { meterStore } from "../store/meterStore";
import { effectiveTrackMeterMode } from "../utils/meterMode";
import type { DawFile, DawProject, DawTrack, InsertDevice, TrackPreviewMode, TrackSend } from "../types/daw";
import { buildTrackContextMenu } from "../menu/trackContextMenu";
import { getSendTargets } from "../utils/routingHelpers";
import { AddTrackSendCommand, RemoveTrackSendCommand } from "../commands";
import { BUILT_IN_PLUGINS, type BuiltInPlugin } from "../plugins/registry";
import { showToast } from "./ui/Toast";
import { platform } from "../platform";

// ─── helpers ──────────────────────────────────────────────────────────────────

let lastInsertAdd:
  | { trackId: string; pluginId: string; at: number }
  | null = null;

function syncNativeMixer(project: DawProject, masterVolume: number) {
  const bridge = window.dawElectron?.floatingWindow;
  if (!bridge?.updateMixer) return;

  const meters = meterStore.getState();
  void bridge.updateMixer({
    tracks: project.tracks
      .filter((track) => track.type !== "master")
      .map((track) => {
        const meter = meters.tracks[track.id];
        return {
          id: track.id,
          name: track.name,
          color: track.color,
          volume: track.volume,
          pan: track.pan,
          mute: track.muted,
          solo: track.solo,
          armed: track.armed,
          meterL: meter?.peakL ?? 0,
          meterR: meter?.peakR ?? 0,
        };
      }),
    master: {
      volume: masterVolume,
      meterL: meters.master.peakL,
      meterR: meters.master.peakR,
    },
  });
}

function sendToDb(v: number) {
  if (v <= 0.001) return "-inf";
  const db = 20 * Math.log10(v);
  return db >= 0 ? `+${db.toFixed(1)}` : db.toFixed(1);
}

function dbToSend(db: number) {
  if (db <= -60) return 0;
  return Math.pow(10, db / 20);
}

function sendToSliderDb(v: number) {
  if (v <= 0.001) return -60;
  return Math.max(-60, Math.min(6, 20 * Math.log10(v)));
}

// ─── Sub-components ───────────────────────────────────────────────────────────

function SectionHeader({
  label, accent, menu,
}: { label: string; accent: string; menu?: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-1.5 px-2 py-[5px]">
      <div className="flex items-center gap-1.5">
        <div className="h-2.5 w-[2px] shrink-0 rounded-full" style={{ background: accent, opacity: 0.55 }} />
        <span
          className="flex-1 text-[9px] font-semibold uppercase tracking-[0.14em]"
          style={{ color: "rgba(220,232,240,0.4)" }}
        >
          {label}
        </span>
      </div>
      {menu}
    </div>
  );
}

type SectionAddButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  accent: string;
};

const SectionAddButton = forwardRef<HTMLButtonElement, SectionAddButtonProps>(
  function SectionAddButton({ accent, onClick, onPointerDown, ...rest }, ref) {
    return (
      <button
        ref={ref}
        type="button"
        {...rest}
        onPointerDown={(e) => {
          e.stopPropagation();
          onPointerDown?.(e);
        }}
        onClick={(e) => {
          e.stopPropagation();
          onClick?.(e);
        }}
        className="app-no-drag flex h-[18px] w-[18px] items-center justify-center rounded-[4px] text-[10px] transition-colors outline-none hover:bg-white/[0.06] data-[state=open]:bg-white/[0.08]"
        style={{ color: "rgba(255,255,255,0.32)" }}
        onMouseEnter={(e) => ((e.currentTarget as HTMLElement).style.color = accent)}
        onMouseLeave={(e) => ((e.currentTarget as HTMLElement).style.color = "rgba(255,255,255,0.32)")}
      >
        <Plus size={11} />
      </button>
    );
  }
);

function InsertsAddMenu({ accent, trackId }: { accent: string; trackId?: string }) {
  const { addInsertDevice } = useProjectStore();
  const add = (plugin: BuiltInPlugin) => {
    if (!trackId) return;
    const now = performance.now();
    if (
      lastInsertAdd &&
      lastInsertAdd.trackId === trackId &&
      lastInsertAdd.pluginId === plugin.id &&
      now - lastInsertAdd.at < 450
    ) {
      return;
    }
    lastInsertAdd = { trackId, pluginId: plugin.id, at: now };
    const device: InsertDevice = {
      id: crypto.randomUUID(),
      type: plugin.type,
      name: plugin.name,
      enabled: true,
      order: 0,
      params: plugin.defaultParams(),
    };
    addInsertDevice(trackId, device);
  };

  const iconForPlugin = (plugin: BuiltInPlugin) => {
    if (plugin.type === "eq") return Activity;
    if (plugin.type === "delay") return AudioLines;
    if (plugin.type === "reverb") return Waves;
    if (plugin.type === "optical-compressor" || plugin.type === "compressor") return Gauge;
    return Sparkles;
  };

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <SectionAddButton accent={accent} title="Add insert" disabled={!trackId} />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" sideOffset={4}>
        <DropdownMenuLabel className="text-xs">Add Device</DropdownMenuLabel>
        {BUILT_IN_PLUGINS.map((plugin) => (
          <DropdownMenuItem className="text-xs" key={plugin.id} icon={iconForPlugin(plugin)} onSelect={() => add(plugin)}>
            {plugin.name}
          </DropdownMenuItem>
        ))}
        <DropdownMenuSeparator />
        <DropdownMenuItem icon={Boxes} disabled>Browse Devices…</DropdownMenuItem>
        <DropdownMenuItem icon={Plug} disabled>Plugin Manager…</DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function SendsAddMenu({ accent, track, project }: { accent: string; track: DawTrack; project: DawProject }) {
  const targets = getSendTargets(project, track.id);
  const existingTargetIds = new Set((track.sends ?? []).map((s) => s.targetTrackId));

  function addSend(targetTrackId: string, targetName: string) {
    if (existingTargetIds.has(targetTrackId)) return;
    const send: TrackSend = {
      id: crypto.randomUUID(),
      name: targetName,
      targetTrackId,
      level: 1,
      enabled: true,
      preFader: false,
    };
    useHistoryStore.getState().execute(new AddTrackSendCommand(track.id, send));
  }

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <SectionAddButton accent={accent} title="Add send" />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" sideOffset={4}>
        <DropdownMenuLabel>Send to</DropdownMenuLabel>
        {targets.length === 0 ? (
          <DropdownMenuItem icon={Send} disabled>No return/bus tracks</DropdownMenuItem>
        ) : (
          targets.map((t) => (
            <DropdownMenuItem
              key={t.id}
              icon={t.type === "return" ? CornerDownLeft : GitMerge}
              disabled={existingTargetIds.has(t.id)}
              onSelect={() => addSend(t.id, t.name)}
            >
              {t.name}
            </DropdownMenuItem>
          ))
        )}
        <DropdownMenuSeparator />
        <DropdownMenuItem icon={FolderPlus} disabled>Create New Return…</DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function InsertRow({
  insert, accent, trackId,
}: { insert: InsertDevice; accent: string; trackId: string }) {
  const enabled = insert.enabled;
  const { toggleInsertDevice, removeInsertDevice } = useProjectStore();
  const openEditor = () => {
    if (!platform.pluginHost.isSupported) return;
    void platform.pluginHost.openEditorWindow({
      windowId: `plugin-editor:${trackId}:${insert.id}`,
      title: insert.name || "Plugin Editor",
      subtitle: `${insert.type} • ${trackId}`,
      width: 560,
      height: 380,
    });
  };
  return (
    <div
      className="group flex items-center gap-1.5 border-l-[2px] px-2 py-[3px] transition-colors hover:bg-white/[0.04]"
      style={{ borderColor: enabled ? accent : "rgba(255,255,255,0.12)" }}
    >
      <button
        title="Open plugin editor"
        onDoubleClick={openEditor}
        onClick={openEditor}
        className="flex-1 truncate text-left text-[10px]"
        style={{ color: enabled ? "rgba(255,255,255,0.72)" : "rgba(255,255,255,0.3)" }}
      >
        {insert.name}
      </button>
      <button
        title={enabled ? "Bypass device" : "Enable device"}
        onClick={() => toggleInsertDevice(trackId, insert.id)}
        className="opacity-0 group-hover:opacity-100 transition-opacity text-white/30 hover:text-white/70"
      >
        <Minus size={8} />
      </button>
      <button
        title="Remove device"
        onClick={() => removeInsertDevice(trackId, insert.id)}
        className="opacity-0 group-hover:opacity-100 transition-opacity text-white/30 hover:text-white/70"
      >
        <X size={8} />
      </button>
    </div>
  );
}

function EmptySlotRow({ accent, hint }: { accent: string; hint: string }) {
  return (
    <div
      className="group mx-1 mb-1 flex items-center justify-center rounded-[3px] border border-dashed border-white/[0.05] px-2 py-1 text-[9px] tracking-wide text-white/[0.22] transition-colors hover:border-white/[0.12] hover:bg-white/[0.018] hover:text-white/[0.42]"
      title={hint}
    >
      <span className="truncate">empty</span>
      <span
        className="ml-1.5 hidden h-1 w-1 rounded-full group-hover:inline-block"
        style={{ background: accent, opacity: 0.6 }}
      />
    </div>
  );
}

const PREVIEW_OPTIONS: Array<{ mode: TrackPreviewMode; label: string }> = [
  { mode: "stereo", label: "Stereo" },
  { mode: "mono", label: "Mono" },
  { mode: "mid", label: "Mid" },
  { mode: "side", label: "Side" },
];

function PreviewModeMenu({
  mode, accent, onChange,
}: {
  mode: TrackPreviewMode;
  accent: string;
  onChange?: (mode: TrackPreviewMode) => void;
}) {
  const active = mode !== "stereo";
  const shortLabel = mode === "stereo" ? "M/S" : mode.toUpperCase();
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          title="Stereo Preview Mode"
          className="h-[20px] min-w-[34px] rounded-[4px] border px-1.5 text-[9px] font-bold tracking-wide transition-colors"
          style={{
            background: active ? `${accent}22` : "rgba(255,255,255,0.03)",
            borderColor: active ? `${accent}88` : "rgba(255,255,255,0.09)",
            color: active ? accent : "rgba(220,232,240,0.52)",
          }}
        >
          {shortLabel}
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="center" sideOffset={4}>
        <DropdownMenuLabel>Preview Mode</DropdownMenuLabel>
        {PREVIEW_OPTIONS.map((option) => (
          <DropdownMenuItem
            key={option.mode}
            onSelect={() => onChange?.(option.mode)}
            className={mode === option.mode ? "text-daw-accent" : undefined}
          >
            {option.label}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function SendRow({ send, trackId, project }: { send: TrackSend; trackId: string; project: DawProject }) {
  const targetTrack = project.tracks.find((t) => t.id === send.targetTrackId);
  const displayName = targetTrack?.name ?? send.name;
  const updateTrackSend = useProjectStore((s) => s.updateTrackSend);
  const enabled = send.enabled !== false;
  const sliderDb = sendToSliderDb(send.level);

  const updateLevel = (db: number) => {
    updateTrackSend(trackId, send.id, { level: dbToSend(db), enabled: true });
  };

  return (
    <div
      className="group flex items-center gap-1.5 border-l-[2px] px-2 py-[3px] transition-colors hover:bg-white/[0.04]"
      style={{ borderColor: enabled ? "rgba(255,255,255,0.16)" : "rgba(255,255,255,0.07)" }}
    >
      <button
        type="button"
        title={enabled ? "Disable send" : "Enable send"}
        onClick={() => updateTrackSend(trackId, send.id, { enabled: !enabled })}
        className="h-2 w-2 shrink-0 rounded-full border border-white/[0.16]"
        style={{ background: enabled ? "rgba(114,215,215,0.85)" : "transparent" }}
      />
      <span className="min-w-0 flex-1 truncate text-[10px] text-white/60">{displayName}</span>
      <input
        aria-label={`Send level to ${displayName}`}
        title={`Send level: ${sendToDb(send.level)} dB`}
        type="range"
        min={-60}
        max={6}
        step={0.5}
        value={sliderDb}
        onChange={(e) => updateLevel(Number(e.currentTarget.value))}
        className="h-4 w-12 shrink-0 accent-[#72d7d7]"
      />
      <span className="w-8 shrink-0 text-right text-[9px] tabular-nums text-white/35">{sendToDb(send.level)}</span>
      <button
        title="Remove send"
        className="opacity-0 group-hover:opacity-100 transition-opacity text-white/30 hover:text-white/70 ml-0.5"
        onClick={() => useHistoryStore.getState().execute(new RemoveTrackSendCommand(trackId, send))}
      >
        <X size={8} />
      </button>
    </div>
  );
}

// ─── Responsive level type ────────────────────────────────────────────────────

type StripLevel = "full" | "medium" | "compact";

// ─── Channel Strip ────────────────────────────────────────────────────────────

type StripProps = {
  track?: DawTrack;           // undefined = Master
  project: DawProject;
  label: string;
  color: string;
  volume: number;
  pan?: number;
  channelIndex?: number;
  onVolume: (v: number) => void;
  onPan?: (v: number) => void;
  muted?: boolean;
  solo?: boolean;
  armed?: boolean;
  monitorMode?: DawTrack["monitorMode"];
  previewMode?: TrackPreviewMode;
  onMute?: () => void;
  onSolo?: () => void;
  onArm?: () => void;
  onMonitor?: () => void;
  onPreviewMode?: (mode: TrackPreviewMode) => void;
  onVolumeEnd?: (v: number) => void;
  onPanEnd?: (v: number) => void;
  fixedWidth?: number;
  level: StripLevel;
  onResizeDragStart?: (e: React.PointerEvent) => void;
  files: DawFile[];
};

function ChannelStrip({
  track, project, label, color, volume, pan = 0, channelIndex,
  onVolume, onPan, onVolumeEnd, onPanEnd,
  muted, solo, armed, monitorMode, previewMode = "stereo",
  onMute, onSolo, onArm, onMonitor, onPreviewMode,
  fixedWidth, level, onResizeDragStart,
  files, selected, onClick,
}: StripProps & { selected?: boolean; onClick?: () => void }) {
  const isMaster = !track;
  const accent = color;
  const meterTrackId = isMaster ? "master" : (track?.id ?? "master");
  const meterMode =
    isMaster ? "stereo" : track ? effectiveTrackMeterMode(track, files) : "stereo";
  const inserts: InsertDevice[] = track?.inserts ?? [];
  const sends: TrackSend[] = track?.sends ?? [];
  const previewActive = previewMode !== "stereo";
  const canMonitor = track?.type === "audio" || track?.type === "midi" || track?.type === "instrument";

  const style: React.CSSProperties = fixedWidth !== undefined
    ? { width: fixedWidth, minWidth: fixedWidth, flexShrink: 0 }
    : { flex: 1, minWidth: 72, maxWidth: 200 };

  const showFull   = level === "full";
  const showMedium = level === "full" || level === "medium";

  return (
    <section
      onClick={onClick}
      onContextMenu={(e) => {
        if (!track) return;
        e.preventDefault();
        useUIStore.getState().setSelectedTrackId(track.id);
        useUIStore.getState().setContextMenu(true, { x: e.clientX, y: e.clientY }, buildTrackContextMenu(track));
        if (onClick) onClick();
      }}
      className={[
        "relative flex h-full flex-col select-none",
        isMaster ? "border-l-2 border-l-white/[0.085]" : "border-r border-r-white/[0.04]",
        selected ? "ring-1 ring-inset ring-white/[0.06]" : "",
      ].join(" ")}
      style={{
        ...style,
        background: selected
          ? "rgba(255,255,255,0.038)"
          : isMaster
            ? "linear-gradient(180deg, rgba(72,209,204,0.045) 0%, rgba(72,209,204,0.018) 100%)"
            : `linear-gradient(180deg, ${accent}0E 0%, rgba(255,255,255,0.012) 22%)`,
      }}
    >
      {/* top accent line — subtle gradient instead of solid bar */}
      <div
        className="h-[1.5px] w-full shrink-0"
        style={{ background: `linear-gradient(90deg, transparent 0%, ${accent} 28%, ${accent} 72%, transparent 100%)`, opacity: 0.75 }}
      />

      <div className="shrink-0 border-b border-white/[0.045] px-2 py-1.5">
        <div className="flex items-center gap-1.5">
          <div className="h-7 w-[3px] shrink-0 rounded-full" style={{ background: accent }} />
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-1">
              <span className="truncate text-[10.5px] font-semibold text-white/80" title={label}>{label}</span>
              {previewActive && (
                <span
                  className="rounded-[3px] border px-1 py-[1px] text-[7.5px] font-bold leading-none"
                  style={{ borderColor: `${accent}66`, color: accent, background: `${accent}18` }}
                >
                  {previewMode.toUpperCase()}
                </span>
              )}
            </div>
            <div className="mt-0.5 flex items-center gap-1 text-[8px] uppercase tracking-[0.08em] text-white/28">
              <span>{isMaster ? "MST" : track?.type ?? "audio"}</span>
              <span>CH {isMaster ? "M" : String(channelIndex ?? 1).padStart(2, "0")}</span>
            </div>
          </div>
        </div>
        {import.meta.env.DEV && track && (
          <div className="mt-1 truncate text-[7.5px] tabular-nums text-white/20">
            {track.id} / meter:{track.id}
          </div>
        )}
      </div>

      {/* ── INSERTS (full only) ── */}
      {showFull && (
        <div className="shrink-0 border-b border-white/[0.045]">
          <SectionHeader label="Inserts" accent={accent} menu={<InsertsAddMenu accent={accent} trackId={track?.id} />} />
          {inserts.length === 0 ? (
            <EmptySlotRow accent={accent} hint="Click + to add a device" />
          ) : (
            track && inserts.map((ins) => (
              <InsertRow key={ins.id} insert={ins} accent={accent} trackId={track.id} />
            ))
          )}
        </div>
      )}

      {/* ── SENDS (full only) ── */}
      {showFull && !isMaster && (
        <div className="shrink-0 border-b border-white/[0.045]">
          <SectionHeader label="Sends" accent={accent} menu={<SendsAddMenu accent={accent} track={track!} project={project} />} />
          {sends.length === 0 ? (
            <EmptySlotRow accent={accent} hint="Click + to route a send" />
          ) : (
            sends.map((s) => <SendRow key={s.id} send={s} trackId={track!.id} project={project} />)
          )}
        </div>
      )}

      {/* ── Pan knob (medium+) ── */}
      {showMedium && !isMaster && (
        <div className="flex shrink-0 flex-col items-center gap-0.5 border-b border-white/[0.045] py-2">
          <Knob
            value={pan}
            min={-1}
            max={1}
            size={40}
            color={accent}
            bipolar
            onChange={onPan ?? (() => {})}
            onChangeEnd={onPanEnd}
          />
          <div className="flex w-full items-center justify-between px-3">
            <span className="text-[7px] font-medium uppercase tracking-wider text-white/[0.22]">L</span>
            <span className="text-[7px] font-medium uppercase tracking-wider text-white/[0.22]">R</span>
          </div>
        </div>
      )}

      {/* ── Fader (dB scale + rail + thumb + VU + M/S) ── */}
      <div className="flex min-h-0 flex-1 overflow-hidden px-1.5 py-2">
        <MixerFader
          value={volume}
          meterTrackId={meterTrackId}
          meterMode={meterMode}
          muted={muted}
          solo={solo}
          isMaster={isMaster}
          color={accent}
          showTrackButtons={false}
          onChange={onVolume}
          onCommit={onVolumeEnd}
          onMute={onMute}
          onSolo={onSolo}
        />
      </div>

      {!isMaster && (
        <div className="shrink-0 border-t border-white/[0.045] px-1.5 py-1">
          <div className="grid grid-cols-4 gap-1">
            <button
              type="button"
              title="Mute"
              onClick={onMute}
              className="h-[20px] rounded-[4px] border text-[9px] font-bold"
              style={{
                background: muted ? "#f0c35b" : "rgba(255,255,255,0.03)",
                borderColor: muted ? "#f0c35b" : "rgba(255,255,255,0.09)",
                color: muted ? "#0d1015" : "rgba(220,232,240,0.52)",
              }}
            >
              M
            </button>
            <button
              type="button"
              title="Solo"
              onClick={onSolo}
              className="h-[20px] rounded-[4px] border text-[9px] font-bold"
              style={{
                background: solo ? "#7ccf86" : "rgba(255,255,255,0.03)",
                borderColor: solo ? "#7ccf86" : "rgba(255,255,255,0.09)",
                color: solo ? "#0d1015" : "rgba(220,232,240,0.52)",
              }}
            >
              S
            </button>
            <button
              type="button"
              title="Record Arm"
              onClick={onArm}
              disabled={!canMonitor}
              className="h-[20px] rounded-[4px] border text-[9px] font-bold disabled:opacity-35"
              style={{
                background: armed ? "#ef6b6b" : "rgba(255,255,255,0.03)",
                borderColor: armed ? "#ef6b6b" : "rgba(255,255,255,0.09)",
                color: armed ? "#0d1015" : "rgba(220,232,240,0.52)",
              }}
            >
              R
            </button>
            <button
              type="button"
              title="Input Monitor"
              onClick={onMonitor}
              disabled={!canMonitor}
              className="h-[20px] rounded-[4px] border text-[9px] font-bold disabled:opacity-35"
              style={{
                background: monitorMode === "in" ? `${accent}22` : "rgba(255,255,255,0.03)",
                borderColor: monitorMode === "in" ? `${accent}88` : "rgba(255,255,255,0.09)",
                color: monitorMode === "in" ? accent : "rgba(220,232,240,0.52)",
              }}
            >
              I
            </button>
          </div>
          <div className="mt-1 flex items-center justify-center">
            <PreviewModeMenu mode={previewMode} accent={accent} onChange={onPreviewMode} />
          </div>
        </div>
      )}

      {/* ── Name footer ── */}
      <div
        className="shrink-0 border-t border-white/[0.055] px-1.5 py-1.5 text-center"
        style={{ background: "rgba(0,0,0,0.22)" }}
      >
        <span
          title={label}
          className="block truncate whitespace-nowrap text-[10px] font-semibold tracking-wide"
          style={{ color: selected ? "rgba(238,242,245,0.92)" : "rgba(238,242,245,0.68)" }}
        >
          {label}
        </span>
      </div>

      {/* right-edge resize handle */}
      {fixedWidth !== undefined && (
        <div
          className="absolute inset-y-0 right-0 z-10 w-1 cursor-ew-resize opacity-0 transition-opacity hover:opacity-100"
          style={{ background: "rgba(255,255,255,0.14)" }}
          onPointerDown={onResizeDragStart}
        />
      )}
    </section>
  );
}

// ─── Mixer Panel ──────────────────────────────────────────────────────────────

export function MixerPanel({
  height,
  embedded = false,
  externalWindow = false,
}: {
  height?: number;
  embedded?: boolean;
  externalWindow?: boolean;
}) {
  const project = useProjectStore((s) => s.project);
  const tracks = project.tracks;
  const files = project.files;
  const { setTrackVolume, setTrackPan, setTrackArmed, setTrackMonitorMode } = useProjectStore();
  const {
    masterVolume, setMasterVolume,
    mixerChannelWidth, setMixerChannelWidth,
    mixerFlexLayout, toggleMixerFlexLayout,
    selectedMixerTrackId, setSelectedMixerTrackId,
    setSelectedTrackId, setFocusedPanel, setSelectedClipIds,
    panels, setPanelLayout, togglePanel
  } = useUIStore();

  const mixerHeight = height ?? panels.mixer?.size ?? 300;

  // horizontal virtualization state for main strip scroll area
  const mainScrollRef = useRef<HTMLDivElement>(null);
  const [stripScrollLeft, setStripScrollLeft] = useState(0);
  const [stripViewWidth, setStripViewWidth] = useState(0);

  useEffect(() => {
    const el = mainScrollRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setStripViewWidth(el.clientWidth));
    ro.observe(el);
    setStripViewWidth(el.clientWidth);
    return () => ro.disconnect();
  }, []);

  useEffect(() => {
    syncNativeMixer(project, masterVolume);
  }, [project, masterVolume]);

  // height resize — useRef so drag state survives re-renders
  const hDragRef = useRef<{ startY: number; startH: number } | null>(null);
  const onHeightDragStart = (e: React.PointerEvent<HTMLDivElement>) => {
    e.currentTarget.setPointerCapture(e.pointerId);
    hDragRef.current = { startY: e.clientY, startH: mixerHeight };
  };
  const onHeightDrag = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!hDragRef.current) return;
    const newH = Math.max(160, Math.min(600, hDragRef.current.startH + hDragRef.current.startY - e.clientY));
    setPanelLayout("mixer", { size: newH });
  };
  const onHeightDragEnd = () => { hDragRef.current = null; };

  // strip width resize — useRef
  const wDragRef = useRef<{ startX: number; startW: number } | null>(null);
  const onStripResizeDragStart = (e: React.PointerEvent) => {
    e.stopPropagation();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    wDragRef.current = { startX: e.clientX, startW: mixerChannelWidth };
  };
  const onStripResizeDrag = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!wDragRef.current) return;
    setMixerChannelWidth(wDragRef.current.startW + e.clientX - wDragRef.current.startX);
  };
  const onStripResizeDragEnd = () => { wDragRef.current = null; };

  // responsive: based on available strip content height
  const contentH = mixerHeight - 33; // minus resize handle + header
  const stripLevel: StripLevel =
    contentH >= 340 ? "full" :
    contentH >= 210 ? "medium" :
    "compact";

  const fixedWidth = mixerFlexLayout ? undefined : mixerChannelWidth;

  return (
    <div
      className={[
        "flex flex-col overflow-hidden bg-[#111418]",
        embedded ? "min-h-0 flex-1" : "shrink-0 border-t border-daw-border",
      ].join(" ")}
      style={
        externalWindow
          ? { height: "100%", minHeight: 0 }
          : embedded
            ? undefined
            : { height: mixerHeight, minHeight: mixerHeight }
      }
      onPointerMove={onStripResizeDrag}
      onPointerUp={onStripResizeDragEnd}
    >
      {/* height resize grip (hidden when embedded — wrapper provides its own) */}
      {!embedded && (
        <div
          className="group flex h-[5px] shrink-0 cursor-ns-resize items-center justify-center"
          onPointerDown={onHeightDragStart}
          onPointerMove={onHeightDrag}
          onPointerUp={onHeightDragEnd}
        >
          <div className="h-[2px] w-8 rounded-full bg-white/[0.06] transition-colors group-hover:bg-white/25" />
        </div>
      )}

      {/* header */}
      <div className="flex h-8 pb-1 shrink-0 items-center gap-2 border-b border-white/[0.06] px-3">
        <SlidersHorizontal size={11} className="text-daw-faint" />
        <span className="text-[10px] font-semibold text-daw-text">Mixer</span>
        <span className="rounded border border-white/[0.07] bg-white/[0.03] px-1.5 py-0.5 text-[9px] text-daw-faint">
          {tracks.length + 1} ch
        </span>

        <div className="flex-1" />

        {!externalWindow && (
          <button
            onClick={() => {
              void (async () => {
                useProjectStore.getState().saveLocal();
                const opened = await window.dawElectron?.windows?.openExternal({
                  id: "mixer",
                  contentType: "mixer",
                  title: "Mixer - Futureboard",
                  width: 1180,
                  height: 420,
                  minWidth: 760,
                  minHeight: 320,
                  alwaysOnTop: false,
                  frame: true,
                  transparent: false,
                  resizable: true,
                });
                if (!opened) {
                  setPanelLayout("mixer", { dock: "float" });
                  showToast("External mixer unavailable; opened internal mixer.", true);
                  return;
                }
                setPanelLayout("mixer", { visible: false });
              })();
            }}
            className="flex h-5 shrink-0 items-center gap-1 rounded border border-daw-accent/30 bg-daw-accent/10 px-1.5 text-[9px] font-semibold text-daw-accent transition-colors hover:border-daw-accent/55 hover:bg-daw-accent/15"
            title="Open External Window"
          >
            <ExternalLink size={9} />
            <span className="hidden min-[1100px]:inline">Open External Window</span>
            <span className="min-[1100px]:hidden">External</span>
          </button>
        )}

        {/* fixed / flex toggle */}
        <button
          onClick={toggleMixerFlexLayout}
          title={mixerFlexLayout ? "Switch to Fixed width" : "Switch to Flex width"}
          className={[
            "flex h-5 items-center gap-1 rounded border px-1.5 text-[9px] font-semibold transition-colors",
            mixerFlexLayout
              ? "border-daw-accent/40 bg-daw-accent/10 text-daw-accent"
              : "border-white/[0.07] bg-white/[0.03] text-daw-faint hover:text-daw-dim",
          ].join(" ")}
        >
          {mixerFlexLayout ? "Flex" : "Fixed"}
        </button>

        {/* width stepper (fixed mode only) */}
        {!mixerFlexLayout && (
          <div className="flex items-center gap-0 rounded border border-white/[0.07] bg-white/[0.03]">
            <button
              onClick={() => setMixerChannelWidth(mixerChannelWidth - 8)}
              className="flex h-5 w-5 items-center justify-center text-daw-faint transition-colors hover:text-daw-text"
              title="Narrow"
            >
              <Minus size={9} />
            </button>
            <span className="min-w-[24px] text-center text-[9px] tabular-nums text-daw-dim">
              {mixerChannelWidth}
            </span>
            <button
              onClick={() => setMixerChannelWidth(mixerChannelWidth + 8)}
              className="flex h-5 w-5 items-center justify-center text-daw-faint transition-colors hover:text-daw-text"
              title="Widen"
            >
              <Plus size={9} />
            </button>
          </div>
        )}

        {!embedded && (
          <button
            onClick={() => togglePanel("mixer")}
            className="flex h-5 w-5 items-center justify-center rounded text-daw-faint transition-colors hover:bg-white/[0.05] hover:text-daw-text"
            title="Collapse mixer [M]"
          >
            <ChevronDown size={11} />
          </button>
        )}
      </div>

      {/* strips */}
      {(() => {
        const mainTracks    = tracks.filter((t) => t.type !== "bus" && t.type !== "return" && t.type !== "group");
        const routingTracks = tracks.filter((t) => t.type === "bus" || t.type === "return" || t.type === "group");

        // Horizontal virtualization for mainTracks (fixed-width mode only)
        const STRIP_OVERSCAN = 3;
        let visibleMain = mainTracks;
        let leadSpacer  = 0;
        let tailSpacer  = 0;

        if (!mixerFlexLayout && fixedWidth !== undefined && stripViewWidth > 0) {
          const firstVisible = Math.max(0, Math.floor(stripScrollLeft / fixedWidth) - STRIP_OVERSCAN);
          const lastVisible  = Math.min(
            mainTracks.length - 1,
            Math.ceil((stripScrollLeft + stripViewWidth) / fixedWidth) + STRIP_OVERSCAN,
          );
          leadSpacer  = firstVisible * fixedWidth;
          tailSpacer  = Math.max(0, (mainTracks.length - 1 - lastVisible) * fixedWidth);
          visibleMain = mainTracks.slice(firstVisible, lastVisible + 1);
        }

        const stripFor = (t: DawTrack) => (
          <ChannelStrip
            key={t.id}
            track={t}
            project={project}
            label={t.name}
            color={t.color}
            channelIndex={tracks.findIndex((track) => track.id === t.id) + 1}
            volume={t.volume}
            pan={t.pan}
            muted={t.muted}
            solo={t.solo}
            armed={t.armed}
            monitorMode={t.monitorMode ?? "off"}
            previewMode={t.monitor?.previewMode ?? "stereo"}
            level={stripLevel}
            fixedWidth={fixedWidth}
            files={files}
            onVolume={(v) => { setTrackVolume(t.id, v); activeAudioEngine.setTrackVolume(t.id, v); }}
            onVolumeEnd={(v) => { useHistoryStore.getState().push(new SetTrackVolumeCommand(t.id, v, t.volume)); }}
            onPan={(v) => { setTrackPan(t.id, v); activeAudioEngine.setTrackPan(t.id, v); }}
            onPanEnd={(v) => { useHistoryStore.getState().push(new SetTrackPanCommand(t.id, v, t.pan)); }}
            onMute={() => { useHistoryStore.getState().execute(new SetTrackMuteCommand(t.id, !t.muted)); }}
            onSolo={() => { useHistoryStore.getState().execute(new SetTrackSoloCommand(t.id, !t.solo)); }}
            onArm={() => { setTrackArmed(t.id, !t.armed); }}
            onMonitor={() => { setTrackMonitorMode(t.id, (t.monitorMode ?? "off") === "in" ? "off" : "in"); }}
            onPreviewMode={(mode) => {
              useHistoryStore.getState().execute(
                new SetTrackPreviewModeCommand(t.id, mode, t.monitor?.previewMode ?? "stereo"),
              );
            }}
            onResizeDragStart={onStripResizeDragStart}
            selected={selectedMixerTrackId === t.id}
            onClick={() => {
              setSelectedMixerTrackId(t.id);
              setSelectedTrackId(t.id);
              setSelectedClipIds([]);
              setFocusedPanel("mixer");
            }}
          />
        );

        return (
          <div className="flex min-h-0 flex-1 overflow-hidden">
            {/* ── main scrollable tracks ── */}
            <div
              ref={mainScrollRef}
              className="flex min-h-0 flex-1 overflow-x-auto overflow-y-hidden"
              onScroll={(e) => setStripScrollLeft(e.currentTarget.scrollLeft)}
            >
              {mainTracks.length === 0 && routingTracks.length === 0 && (
                <div className="flex flex-1 items-center justify-center text-[11px] text-daw-faint">
                  Add tracks to see mixer channels.
                </div>
              )}
              {/* lead spacer keeps scroll position accurate */}
              {leadSpacer > 0 && <div style={{ width: leadSpacer, flexShrink: 0 }} />}
              {visibleMain.map(stripFor)}
              {/* tail spacer */}
              {tailSpacer > 0 && <div style={{ width: tailSpacer, flexShrink: 0 }} />}
              {mainTracks.length > 0 && !mixerFlexLayout && leadSpacer === 0 && tailSpacer === 0 && <div className="flex-1" />}
            </div>

            {/* ── pinned routing + master zone ── */}
            <div className="flex shrink-0 border-l border-white/[0.07]">
              {routingTracks.map(stripFor)}
              <ChannelStrip
                label="Master"
                project={project}
                color="#48d1cc"
                volume={masterVolume}
                level={stripLevel}
                fixedWidth={fixedWidth !== undefined ? Math.max(fixedWidth, 76) : undefined}
                files={files}
                onVolume={(v) => { setMasterVolume(v); activeAudioEngine.setMasterVolume(v); }}
                onResizeDragStart={onStripResizeDragStart}
                selected={selectedMixerTrackId === "master"}
                onClick={() => {
                  setSelectedMixerTrackId("master");
                  setSelectedTrackId(null);
                  setSelectedClipIds([]);
                  setFocusedPanel("mixer");
                }}
              />
            </div>

            {import.meta.env.DEV && (
              <div className="pointer-events-none fixed bottom-8 left-2 z-[9999] rounded bg-black/70 px-2 py-0.5 text-[9px] tabular-nums text-white/50">
                strips: {visibleMain.length}/{mainTracks.length}
              </div>
            )}
          </div>
        );
      })()}
    </div>
  );
}
