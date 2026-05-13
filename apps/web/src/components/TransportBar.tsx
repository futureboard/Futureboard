import {
  Circle,
  FolderOpen,
  PanelBottom,
  PanelRight,
  Pause,
  Play,
  Redo2,
  Repeat2,
  Save,
  Share2,
  SkipBack,
  Square,
  Undo2,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useProjectStore } from "../store/projectStore";
import { useTransportStore } from "../store/transportStore";
import { useUIStore } from "../store/uiStore";
import { transport } from "../engine/Transport";
import { clipScheduler } from "../engine/ClipScheduler";
import { formatBarBeatTick } from "../utils/musicalTime";

const TIME_SIG_NUMERATORS = [2, 3, 4, 5, 6, 7, 8, 9, 12];
const TIME_SIG_DENOMINATORS = [2, 4, 8, 16];

function Divider() {
  return <div className="mx-0.5 h-7 w-px shrink-0 bg-daw-border" />;
}

function IconBtn({
  icon: Icon,
  label,
  onClick,
  active = false,
  accent = false,
  danger = false,
  disabled = false,
  size = 14,
}: {
  icon: React.ElementType;
  label: string;
  onClick?: () => void;
  active?: boolean;
  accent?: boolean;
  danger?: boolean;
  disabled?: boolean;
  size?: number;
}) {
  const cls = [
    "flex h-7 w-7 shrink-0 items-center justify-center rounded-md transition-colors disabled:opacity-30",
    danger && active
      ? "text-daw-ink hover:bg-daw-red"
      : accent && active
        ? "text-daw-ink hover:bg-daw-accent-h"
        : active
          ? "text-daw-text hover:bg-daw-border"
          : "text-daw-dim hover:border-daw-border-light hover:bg-daw-surface-high hover:text-daw-text",
  ].join(" ");
  return (
    <button type="button" onClick={onClick} disabled={disabled} title={label} className={`app-no-drag ${cls}`}>
      <Icon size={size} />
    </button>
  );
}

function MenuBtn({
  label,
  open,
  onClick,
}: {
  label: string;
  open?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={[
        "app-no-drag rounded px-2 py-1 text-[11px] font-semibold transition-colors",
        open
          ? "bg-daw-surface-high text-daw-text"
          : "text-daw-dim hover:bg-daw-surface-high hover:text-daw-text",
      ].join(" ")}
    >
      {label}
    </button>
  );
}

function MenuItem({
  children,
  onClick,
  disabled,
}: {
  children: React.ReactNode;
  onClick?: () => void;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      className="app-no-drag flex w-full items-center gap-2 rounded px-2.5 py-1.5 text-left text-[11px] text-daw-text transition-colors hover:bg-daw-surface-high disabled:cursor-not-allowed disabled:opacity-35"
    >
      {children}
    </button>
  );
}

export function TransportBar({ onImport, onSave }: { onImport?: () => void; onSave?: () => void }) {
  const { isPlaying, playheadTime, setIsPlaying } = useTransportStore();
  const { project, setBpm, setTimeSignature } = useProjectStore();
  const { inspectorOpen, toggleInspector, mixerOpen, toggleMixer, loopEnabled, toggleLoop } = useUIStore();

  const [openMenu, setOpenMenu] = useState<"file" | "edit" | "view" | null>(null);
  const barRef = useRef<HTMLDivElement>(null);

  const timeSig = project.timeSignature ?? { numerator: 4, denominator: 4 };

  useEffect(() => {
    if (!openMenu) return;
    const close = (e: MouseEvent) => {
      if (barRef.current && !barRef.current.contains(e.target as Node)) setOpenMenu(null);
    };
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [openMenu]);

  useEffect(() => {
    if (!openMenu) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpenMenu(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [openMenu]);

  const handlePlay = async () => {
    await transport.play(() => {
      clipScheduler.schedule(project.tracks);
      setIsPlaying(true);
    });
  };
  const handlePause = () => {
    transport.pause();
    clipScheduler.cancelAll();
    setIsPlaying(false);
  };
  const handleStop = () => {
    transport.stop(() => {
      clipScheduler.cancelAll();
      setIsPlaying(false);
    });
  };

  return (
    <div
      ref={barRef}
      className="drag-region-app relative z-20 flex h-9 shrink-0 select-none items-stretch border-b border-daw-border bg-daw-sunken px-2 pr-35 shadow-[0_8px_24px_rgba(0,0,0,0.22)]"
    >
      <div className="flex w-full min-w-0 items-center justify-between gap-4">
        {/* ── Left: menu bar ───────────────────────────────────────────── */}
        <div className="flex min-w-0 flex-1 items-center gap-2">
          <div className="app-no-drag flex shrink-0 items-center gap-0.5">
            <div className="relative">
              <MenuBtn label="File" open={openMenu === "file"} onClick={() => setOpenMenu((m) => (m === "file" ? null : "file"))} />
              {openMenu === "file" && (
                <div className="absolute left-0 top-full z-50 mt-0.5 min-w-[11rem] rounded-md border border-daw-border bg-daw-surface py-1 shadow-xl">
                  <MenuItem
                    onClick={() => {
                      setOpenMenu(null);
                      onImport?.();
                    }}
                  >
                    <FolderOpen size={12} className="text-daw-dim" />
                    Import Audio...
                  </MenuItem>
                  <MenuItem
                    onClick={() => {
                      setOpenMenu(null);
                      onSave?.();
                    }}
                  >
                    <Save size={12} className="text-daw-dim" />
                    Save Project
                  </MenuItem>
                </div>
              )}
            </div>

            <div className="relative">
              <MenuBtn label="Edit" open={openMenu === "edit"} onClick={() => setOpenMenu((m) => (m === "edit" ? null : "edit"))} />
              {openMenu === "edit" && (
                <div className="absolute left-0 top-full z-50 mt-0.5 min-w-[10rem] rounded-md border border-daw-border bg-daw-surface py-1 shadow-xl">
                  <MenuItem disabled>
                    <Undo2 size={12} className="text-daw-dim" />
                    Undo
                  </MenuItem>
                  <MenuItem disabled>
                    <Redo2 size={12} className="text-daw-dim" />
                    Redo
                  </MenuItem>
                </div>
              )}
            </div>

            <div className="relative">
              <MenuBtn label="View" open={openMenu === "view"} onClick={() => setOpenMenu((m) => (m === "view" ? null : "view"))} />
              {openMenu === "view" && (
                <div className="absolute left-0 top-full z-50 mt-0.5 min-w-[11rem] rounded-md border border-daw-border bg-daw-surface py-1 shadow-xl">
                  <MenuItem
                    onClick={() => {
                      setOpenMenu(null);
                      toggleMixer();
                    }}
                  >
                    <PanelBottom size={12} className={mixerOpen ? "text-daw-accent" : "text-daw-dim"} />
                    <span className="flex-1">Mixer</span>
                    {mixerOpen && <span className="text-[9px] text-daw-accent">On</span>}
                  </MenuItem>
                  <MenuItem
                    onClick={() => {
                      setOpenMenu(null);
                      toggleInspector();
                    }}
                  >
                    <PanelRight size={12} className={inspectorOpen ? "text-daw-accent" : "text-daw-dim"} />
                    <span className="flex-1">Inspector</span>
                    {inspectorOpen && <span className="text-[9px] text-daw-accent">On</span>}
                  </MenuItem>
                </div>
              )}
            </div>
          </div>
          <div className="h-5 w-px shrink-0 bg-daw-border" />
          {/* ── Center: project name ───────────────────────────────────── */}
          <div className="flex min-w-0 max-w-full items-center gap-2 px-1">
            <div
              className="min-w-0 flex-1 truncate text-left text-[12px] font-semibold leading-tight text-daw-text"
              title={project.name}
            >
              {project.name}
            </div>

            <div className="shrink-0 whitespace-nowrap text-right text-[8px] font-medium uppercase tracking-wide text-daw-faint">
              Saved locally
            </div>
          </div>
        </div>

        {/* ── Right: transport + time + tempo + quick actions ───────── */}
        <div className="app-no-drag flex min-w-0 shrink-0 flex-wrap items-center justify-end gap-0.5 sm:flex-nowrap">
          <IconBtn
            icon={SkipBack}
            label="Return to start [Enter]"
            onClick={() => {
              transport.seek(0);
              clipScheduler.cancelAll();
            }}
          />
          {isPlaying ? (
            <IconBtn icon={Pause} label="Pause [Space]" active onClick={handlePause} />
          ) : (
            <IconBtn icon={Play} label="Play [Space]" onClick={handlePlay} size={15} />
          )}
          <IconBtn
            icon={Square}
            label="Stop [Enter]"
            onClick={handleStop}
            disabled={!isPlaying && playheadTime === 0}
          />
          <IconBtn icon={Circle} label="Record" accent danger size={12} />
          <IconBtn icon={Repeat2} label="Loop [L]" active={loopEnabled} onClick={toggleLoop} size={13} />

          <Divider />

          <div className="flex h-7 min-w-[6.5rem] items-center justify-center px-1 text-[13px] font-semibold tabular-nums text-daw-text sm:min-w-[7.75rem]">
            {formatBarBeatTick(playheadTime, project.bpm, timeSig)}
          </div>

          <Divider />

          <div className="flex h-7 items-center gap-1 px-1">
            <span className="text-[8px] font-medium text-daw-faint">BPM</span>
            <input
              type="number"
              min={20}
              max={300}
              value={project.bpm}
              onChange={(e) => setBpm(parseInt(e.target.value) || 120)}
              className="w-10 border-none bg-transparent text-center text-[11px] font-semibold text-daw-text outline-none"
            />
          </div>

          <div className="flex h-7 items-center gap-0.5 px-1">
            <select
              value={timeSig.numerator}
              onChange={(e) => setTimeSignature({ ...timeSig, numerator: parseInt(e.target.value) })}
              className="w-5 cursor-pointer appearance-none border-none bg-transparent text-center text-[11px] font-semibold text-daw-text outline-none"
              title="Beats per bar"
            >
              {TIME_SIG_NUMERATORS.map((n) => (
                <option key={n} value={n} className="bg-daw-surface text-daw-text">
                  {n}
                </option>
              ))}
            </select>
            <span className="text-[10px] opacity-25">/</span>
            <select
              value={timeSig.denominator}
              onChange={(e) => setTimeSignature({ ...timeSig, denominator: parseInt(e.target.value) })}
              className="w-5 cursor-pointer appearance-none border-none bg-transparent text-center text-[11px] font-semibold text-daw-text outline-none"
              title="Note value per beat"
            >
              {TIME_SIG_DENOMINATORS.map((n) => (
                <option key={n} value={n} className="bg-daw-surface text-daw-text">
                  {n}
                </option>
              ))}
            </select>
          </div>

          <Divider />

          <IconBtn icon={PanelBottom} label="Toggle Mixer [M]" active={mixerOpen} onClick={toggleMixer} />
          <IconBtn icon={PanelRight} label="Toggle Inspector [I]" active={inspectorOpen} onClick={toggleInspector} />

          <Divider />

          <IconBtn icon={FolderOpen} label="Import Audio" onClick={onImport} />
          <IconBtn icon={Save} label="Save Project" onClick={onSave} />
          <IconBtn icon={Share2} label="Share" disabled />
        </div>
      </div>
    </div>
  );
}
