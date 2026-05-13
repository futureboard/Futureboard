import { ChevronDown, Minus, Plus, SlidersHorizontal, X } from "lucide-react";
import { useRef } from "react";
import { useProjectStore } from "../store/projectStore";
import { useUIStore } from "../store/uiStore";
import { useHistoryStore } from "../store/historyStore";
import { SetTrackVolumeCommand, SetTrackPanCommand, SetTrackMuteCommand, SetTrackSoloCommand } from "../commands";
import { mixer } from "../engine/Mixer";
import { VuMeter } from "./ui/VuMeter";
import { Knob } from "./ui/Knob";
import { VerticalFader } from "./ui/VerticalFader";
import { useVuStereoLevels } from "../hooks/useVuLevel";
import { effectiveTrackMeterMode } from "../utils/meterMode";
import type { DawFile, DawTrack, TrackInsert, TrackSend } from "../types/daw";

// ─── helpers ──────────────────────────────────────────────────────────────────

function volumeToDb(v: number) {
  if (v <= 0.001) return "-∞";
  const db = 20 * Math.log10(v);
  return (db >= 0 ? `+${db.toFixed(1)}` : db.toFixed(1)) + " dB";
}

function sendToDb(v: number) {
  if (v <= 0.001) return "-inf";
  const db = 20 * Math.log10(v);
  return db >= 0 ? `+${db.toFixed(1)}` : db.toFixed(1);
}

// ─── Sub-components ───────────────────────────────────────────────────────────

function MixBtn({
  label, active, activeColor, onClick, title, wide = false,
}: { label: string; active: boolean; activeColor: string; onClick?: () => void; title?: string; wide?: boolean }) {
  return (
    <button
      type="button"
      title={title ?? label}
      onClick={onClick}
      className={[
        "grid place-items-center  text-[10px] border font-black transition-colors",
        wide ? "h-6 flex-1" : "h-5 w-5",
      ].join(" ")}
      style={{
        background: active ? activeColor : "",
        borderColor: active ? activeColor : "rgba(255,255,255,0.09)",
        color: active ? "#0d1015" : "rgba(220,232,240,0.6)",
      }}
    >
      {label}
    </button>
  );
}

function SectionHeader({
  label, accent, onAdd,
}: { label: string; accent: string; onAdd?: () => void }) {
  return (
    <div className="flex items-center gap-1.5 px-1 py-[5px] justify-between">
      <div className="flex space-x-2">
        <div className="h-3 w-[2px] shrink-0 rounded-full" style={{ background: accent }} />
        <span className="flex-1 text-[9px] font-bold uppercase tracking-widest" style={{ color: accent, opacity: 0.75 }}>
          {label}
        </span>
      </div>
      <button
        onClick={onAdd}
        className="flex h-4 w-4 items-center justify-center rounded text-[10px] transition-colors"
        style={{ color: "rgba(255,255,255,0.3)" }}
        onMouseEnter={(e) => ((e.currentTarget as HTMLElement).style.color = accent)}
        onMouseLeave={(e) => ((e.currentTarget as HTMLElement).style.color = "rgba(255,255,255,0.3)")}
        title={`Add ${label.toLowerCase()}`}
      >
        <Plus size={10} />
      </button>
    </div>
  );
}

function InsertRow({ insert, accent }: { insert: TrackInsert; accent: string }) {
  return (
    <div
      className="group flex items-center gap-1.5 border-l-[2px] px-2 py-[3px] transition-colors hover:bg-white/[0.04]"
      style={{ borderColor: insert.bypassed ? "rgba(255,255,255,0.12)" : accent }}
    >
      <span
        className="flex-1 truncate text-[10px]"
        style={{ color: insert.bypassed ? "rgba(255,255,255,0.3)" : "rgba(255,255,255,0.72)" }}
      >
        {insert.name}
      </span>
      <button className="opacity-0 group-hover:opacity-100 transition-opacity text-white/30 hover:text-white/70">
        <X size={8} />
      </button>
    </div>
  );
}

function SendRow({ send }: { send: TrackSend }) {
  return (
    <div className="flex items-center gap-1.5 border-l-[2px] border-white/[0.1] px-2 py-[3px] transition-colors hover:bg-white/[0.04]">
      <span className="flex-1 truncate text-[10px] text-white/60">{send.name}</span>
      <span className="shrink-0 text-[9px] tabular-nums text-white/35">{sendToDb(send.level)}</span>
    </div>
  );
}

// ─── Responsive level type ────────────────────────────────────────────────────

type StripLevel = "full" | "medium" | "compact";

// ─── Channel Strip ────────────────────────────────────────────────────────────

type StripProps = {
  track?: DawTrack;           // undefined = Master
  label: string;
  color: string;
  volume: number;
  pan?: number;
  onVolume: (v: number) => void;
  onPan?: (v: number) => void;
  muted?: boolean;
  solo?: boolean;
  onMute?: () => void;
  onSolo?: () => void;
  onVolumeEnd?: (v: number) => void;
  onPanEnd?: (v: number) => void;
  fixedWidth?: number;
  level: StripLevel;
  onResizeDragStart?: (e: React.PointerEvent) => void;
  files: DawFile[];
};

function ChannelStrip({
  track, label, color, volume, pan = 0,
  onVolume, onPan, onVolumeEnd, onPanEnd,
  muted, solo, onMute, onSolo,
  fixedWidth, level, onResizeDragStart,
  files, selected, onClick,
}: StripProps & { selected?: boolean; onClick?: () => void }) {
  const isMaster = !track;
  const accent = color;
  const vu = useVuStereoLevels(isMaster ? "master" : (track?.id ?? "master"));
  const meterMode =
    isMaster ? "stereo" : track ? effectiveTrackMeterMode(track, files) : "stereo";
  const inserts: TrackInsert[] = track?.inserts ?? [];
  const sends: TrackSend[] = track?.sends ?? [];

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
        useUIStore.getState().setContextMenu(true, { x: e.clientX, y: e.clientY }, [
          {
            id: "ctx.delete_track",
            label: "Delete Track",
            danger: true,
            action: "edit:delete-track"
          }
        ]);
        if (onClick) onClick();
      }}
      className={`relative flex h-full flex-col border-x border-white/[0.055] select-none ${selected ? "bg-white/[0.05] ring-1 ring-inset ring-white/[0.05]" : ""}`}
      style={{ ...style, background: selected ? undefined : isMaster ? "rgba(72,209,204,0.035)" : "rgba(255,255,255,0.016)" }}
    >
      {/* top colour bar */}
      <div className="h-[2px] w-full shrink-0" style={{ background: accent }} />

      {/* ── INSERTS (full only) ── */}
      {showFull && (
        <div className="shrink-0 border-b border-white/[0.05]">
          <SectionHeader label="Inserts" accent={accent} />
          {inserts.length === 0 ? (
            <div className="px-4 pb-[5px] text-[9px] italic text-white/20">empty</div>
          ) : (
            inserts.map((ins) => <InsertRow key={ins.id} insert={ins} accent={accent} />)
          )}
        </div>
      )}

      {/* ── SENDS (full only) ── */}
      {showFull && (
        <div className="shrink-0 border-b border-white/[0.05]">
          <SectionHeader label="Sends" accent={accent} />
          {sends.length === 0 ? (
            <div className="px-4 pb-[5px] text-[9px] italic text-white/20">empty</div>
          ) : (
            sends.map((s) => <SendRow key={s.id} send={s} />)
          )}
        </div>
      )}

      {/* ── Pan knob (medium+) ── */}
      {showMedium && !isMaster && (
        <div className="flex shrink-0 flex-col items-center gap-0.5 border-b border-white/[0.05] py-2">
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
            <span className="text-[8px] text-white/25">L</span>
            <span className="text-[8px] text-white/25">R</span>
          </div>
        </div>
      )}

      {/* ── M / S (medium+) ── */}
      {showMedium && (
        <div className="flex shrink-0 gap-1 border-b border-white/[0.05] px-2 py-1.5">
          {isMaster ? (
            <span className="flex-1 text-center text-[8px] font-semibold uppercase tracking-widest text-white/25">
              master
            </span>
          ) : (
            <>
              <MixBtn label="M" wide active={!!muted} activeColor="#f3c969" onClick={onMute} title="Mute" />
              <MixBtn label="S" wide active={!!solo}  activeColor="#7bd88f" onClick={onSolo} title="Solo" />
            </>
          )}
        </div>
      )}

      {/* ── Fader + VU (always) ── */}
      <div className="flex min-h-0 flex-1 gap-1.5 overflow-hidden px-2 py-2">
        <div
          className={`flex shrink-0 flex-col items-center gap-0.5 self-stretch min-h-0 ${
            meterMode === "stereo" ? "w-[14px]" : "w-[7px]"
          }`}
        >
          <span className="h-[10px] shrink-0 text-[6px] font-semibold uppercase leading-none text-white/28">
            {isMaster ? "LR" : meterMode === "mono" ? "Mono" : "Stereo"}
          </span>
          <div className="flex min-h-0 flex-1 w-full justify-center">
            <VuMeter
              mode={meterMode}
              levelL={vu.l}
              levelR={vu.r}
              columnWidth={5}
            />
          </div>
        </div>

        {/* Vertical fader */}
        <VerticalFader value={volume} onChange={onVolume} onChangeEnd={onVolumeEnd} accent={accent} />
      </div>

      {/* ── Name + dB readout ── */}
      <div
        className="shrink-0 border-t border-white/[0.07] px-1.5 py-1.5 text-center"
        style={{ background: "rgba(0,0,0,0.18)" }}
      >
        <span
          title={label}
          className="block truncate text-[10px] font-bold tracking-wide text-white/65"
        >
          {label}
        </span>
        <span className="block text-[8px] tabular-nums text-white/25">
          {volumeToDb(volume)}
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

export function MixerPanel() {
  const tracks = useProjectStore((s) => s.project.tracks);
  const files = useProjectStore((s) => s.project.files);
  const { setTrackVolume, setTrackPan, setTrackMute, setTrackSolo } = useProjectStore();
  const {
    masterVolume, setMasterVolume,
    toggleMixer,
    mixerHeight, setMixerHeight,
    mixerChannelWidth, setMixerChannelWidth,
    mixerFlexLayout, toggleMixerFlexLayout,
    selectedMixerTrackId, setSelectedMixerTrackId,
    setSelectedTrackId, setFocusedPanel, setSelectedClipIds,
  } = useUIStore();

  // height resize — useRef so drag state survives re-renders
  const hDragRef = useRef<{ startY: number; startH: number } | null>(null);
  const onHeightDragStart = (e: React.PointerEvent<HTMLDivElement>) => {
    e.currentTarget.setPointerCapture(e.pointerId);
    hDragRef.current = { startY: e.clientY, startH: mixerHeight };
  };
  const onHeightDrag = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!hDragRef.current) return;
    setMixerHeight(hDragRef.current.startH + hDragRef.current.startY - e.clientY);
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
      className="flex shrink-0 flex-col overflow-hidden border-t border-daw-border bg-[#111418]"
      style={{ height: mixerHeight, minHeight: mixerHeight }}
      onPointerMove={onStripResizeDrag}
      onPointerUp={onStripResizeDragEnd}
    >
      {/* height resize grip */}
      <div
        className="group flex h-[5px] shrink-0 cursor-ns-resize items-center justify-center"
        onPointerDown={onHeightDragStart}
        onPointerMove={onHeightDrag}
        onPointerUp={onHeightDragEnd}
      >
        <div className="h-[2px] w-8 rounded-full bg-white/[0.06] transition-colors group-hover:bg-white/25" />
      </div>

      {/* header */}
      <div className="flex h-8 pb-1 shrink-0 items-center gap-2 border-b border-white/[0.06] px-3">
        <SlidersHorizontal size={11} className="text-daw-faint" />
        <span className="text-[10px] font-semibold text-daw-text">Mixer</span>
        <span className="rounded border border-white/[0.07] bg-white/[0.03] px-1.5 py-0.5 text-[9px] text-daw-faint">
          {tracks.length + 1} ch
        </span>

        <div className="flex-1" />

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

        <button
          onClick={toggleMixer}
          className="flex h-5 w-5 items-center justify-center rounded text-daw-faint transition-colors hover:bg-white/[0.05] hover:text-daw-text"
          title="Collapse mixer [M]"
        >
          <ChevronDown size={11} />
        </button>
      </div>

      {/* strips */}
      <div className="flex min-h-0 flex-1 overflow-x-auto overflow-y-hidden">
        {tracks.length === 0 && (
          <div className="flex flex-1 items-center justify-center text-[11px] text-daw-faint">
            Add tracks to see mixer channels.
          </div>
        )}

        {tracks.map((t) => (
          <ChannelStrip
            key={t.id}
            track={t}
            label={t.name}
            color={t.color}
            volume={t.volume}
            pan={t.pan}
            muted={t.muted}
            solo={t.solo}
            level={stripLevel}
            fixedWidth={fixedWidth}
            files={files}
            onVolume={(v) => { setTrackVolume(t.id, v); mixer.setVolume(t.id, v); }}
            onVolumeEnd={(v) => { useHistoryStore.getState().push(new SetTrackVolumeCommand(t.id, v, t.volume)); }}
            onPan={(v) => { setTrackPan(t.id, v); mixer.setPan(t.id, v); }}
            onPanEnd={(v) => { useHistoryStore.getState().push(new SetTrackPanCommand(t.id, v, t.pan)); }}
            onMute={() => { useHistoryStore.getState().execute(new SetTrackMuteCommand(t.id, !t.muted)); }}
            onSolo={() => { useHistoryStore.getState().execute(new SetTrackSoloCommand(t.id, !t.solo)); }}
            onResizeDragStart={onStripResizeDragStart}
            selected={selectedMixerTrackId === t.id}
            onClick={() => {
              setSelectedMixerTrackId(t.id);
              setSelectedTrackId(t.id);
              setSelectedClipIds([]);
              setFocusedPanel("mixer");
            }}
          />
        ))}

        {tracks.length > 0 && !mixerFlexLayout && <div className="flex-1" />}

        <ChannelStrip
          label="Master"
          color="#48d1cc"
          volume={masterVolume}
          level={stripLevel}
          fixedWidth={fixedWidth !== undefined ? Math.max(fixedWidth, 76) : undefined}
          files={files}
          onVolume={(v) => { setMasterVolume(v); mixer.setMasterVolume(v); }}
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
    </div>
  );
}
