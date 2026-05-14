import {
  Check,
  ChevronDown,
  ChevronRight,
  Circle,
  FolderOpen,
  PanelBottom,
  PanelRight,
  Pause,
  Play,
  Repeat2,
  Save,
  Share2,
  SkipBack,
  Square,
  Timer,
} from "lucide-react";
import { MenuDawIcon } from "../icons/dawIcons";
import { useEffect, useRef, useState } from "react";
import { transport } from "../engine/Transport";
import { APP_MENUS, type AppMenuGroup, type AppMenuItem } from "../menu/menuItems";
import { runAction } from "../menu/actionRunner";
import { useProjectStore } from "../store/projectStore";
import { useTransportStore } from "../store/transportStore";
import { useMetronomeStore } from "../store/metronomeStore";
import { useUIStore } from "../store/uiStore";
import { formatBarBeatTick } from "../utils/musicalTime";
import logoApp from "../assets/logo.png"
import { ProjectDropdown } from "./project/ProjectDropdown";


const TIME_SIG_NUMERATORS = [2, 3, 4, 5, 6, 7, 8, 9, 12];
const TIME_SIG_DENOMINATORS = [2, 4, 8, 16];

// MENU_ICONS removed — MenuIcon now routes through DawIcon registry

type CommandMenuItem = Extract<AppMenuItem, { type?: "item" }>;

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
    "app-no-drag flex h-7 w-7 shrink-0 items-center justify-center rounded-md transition-colors disabled:opacity-30",
    danger && active
      ? "text-daw-ink hover:bg-daw-red"
      : accent && active
        ? "text-daw-ink hover:bg-daw-accent-h"
        : active
          ? "text-daw-text hover:bg-daw-border"
          : "text-daw-dim hover:border-daw-border-light hover:bg-daw-surface-high hover:text-daw-text",
  ].join(" ");

  return (
    <button type="button" onClick={onClick} disabled={disabled} title={label} className={cls}>
      <Icon size={size} />
    </button>
  );
}

function TopMenuButton({
  label,
  open,
  onClick,
  onMouseEnter,
}: {
  label: string;
  open: boolean;
  onClick: () => void;
  onMouseEnter: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      onMouseEnter={onMouseEnter}
      className={[
        "app-no-drag rounded px-2 py-1 text-[11px] font-semibold transition-colors",
        open ? "bg-daw-surface-high text-daw-text" : "text-daw-dim hover:bg-daw-surface-high hover:text-daw-text",
      ].join(" ")}
    >
      {label}
    </button>
  );
}

function MenuIcon({ icon }: { icon?: string }) {
  return <MenuDawIcon icon={icon} size={12} />;
}

function MenuPanel({
  items,
  onAction,
  itemState,
  nested = false,
}: {
  items: AppMenuGroup["children"];
  onAction: (item: CommandMenuItem) => void;
  itemState: (item: AppMenuItem) => Partial<Pick<CommandMenuItem, "checked" | "enabled">>;
  nested?: boolean;
}) {
  return (
    <div
      className={[
        "app-no-drag absolute z-[120] min-w-[15rem] rounded-md border border-daw-border bg-daw-surface p-1 shadow-[0_12px_36px_rgba(0,0,0,0.52)]",
        nested ? "left-full top-[-4px] ml-1" : "left-0 top-[calc(100%+2px)]",
      ].join(" ")}
    >
      {items.map((item) => {
        if (item.type === "separator") {
          return <div key={item.id} className="my-1 h-px bg-daw-border" />;
        }

        const state = itemState(item);
        const enabled = state.enabled ?? item.enabled ?? true;

        if (item.type === "submenu") {
          return (
            <div key={item.id} className="group relative">
              <button
                type="button"
                disabled={!enabled}
                className="grid h-7 w-full grid-cols-[1.25rem_minmax(0,1fr)_0.75rem] items-center gap-2 rounded px-2 text-left text-[11px] text-daw-text transition-colors hover:bg-daw-surface-high disabled:cursor-not-allowed disabled:opacity-35"
              >
                <MenuIcon icon={item.icon} />
                <span className="min-w-0 flex-1 truncate">{item.label}</span>
                <ChevronRight size={12} className="text-daw-faint" />
              </button>
              {enabled ? (
                <div className="hidden group-hover:block">
                  <MenuPanel items={item.children} onAction={onAction} itemState={itemState} nested />
                </div>
              ) : null}
            </div>
          );
        }

        const checked = state.checked ?? item.checked ?? false;

        return (
          <button
            key={item.id}
            type="button"
            disabled={!enabled}
            onClick={() => onAction(item)}
            className={[
              "grid h-7 w-full grid-cols-[1.25rem_minmax(0,1fr)_auto] items-center gap-2 rounded px-2 text-left text-[11px] transition-colors hover:bg-daw-surface-high disabled:cursor-not-allowed disabled:opacity-35",
              item.danger ? "text-daw-red" : "text-daw-text",
            ].join(" ")}
          >
            <span className="flex w-5 shrink-0 items-center justify-center text-daw-faint">
              {checked ? <Check size={12} className="text-daw-accent" /> : <MenuIcon icon={item.icon} />}
            </span>
            <span className="min-w-0 flex-1 truncate">{item.label}</span>
            {item.accelerator ? <span className="pl-5 text-right text-[10px] text-daw-faint">{item.accelerator}</span> : <span />}
          </button>
        );
      })}
    </div>
  );
}

export function TransportBar({ onImport, onSave }: { onImport?: () => void; onSave?: () => void }) {
  const { isPlaying, playheadTime, setIsPlaying } = useTransportStore();
  const { project, setBpm, setTimeSignature } = useProjectStore();
  const {
    panels,
    togglePanel,
    loopEnabled,
    toggleLoop,
    snapToGrid,
    toggleSnapToGrid,
    currentTool,
    selectedClipIds,
    selectedTrackId,
  } = useUIStore();
  const { enabled: metronomeEnabled, toggle: toggleMetronome, countInEnabled } = useMetronomeStore();

  const [openMenu, setOpenMenu] = useState<string | null>(null);
  const [projectDropdownOpen, setProjectDropdownOpen] = useState(false);
  const barRef = useRef<HTMLDivElement>(null);
  const projectBtnRef = useRef<HTMLDivElement>(null);
  const timeSig = project.timeSignature ?? { numerator: 4, denominator: 4 };
  const saveStatus = useUIStore((s) => s.saveStatus);

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
    await transport.play();
    setIsPlaying(true);
  };

  const handlePause = () => {
    transport.pause();
    setIsPlaying(false);
  };

  const handleStop = () => {
    transport.stop();
    setIsPlaying(false);
  };

  const hasClipSel = selectedClipIds.length > 0;
  const hasTrackSel = !!selectedTrackId;

  const getMenuItemState = (item: AppMenuItem) => {
    if (item.type === "separator") return {};
    if (item.type === "submenu") return {};

    switch (item.action) {
      // ── Transport checks ──────────────────────────────────────────────
      case "transport:toggle-loop":
        return { checked: loopEnabled };
      case "transport:toggle-metronome":
        return { checked: metronomeEnabled };
      case "transport:toggle-count-in":
        return { checked: countInEnabled };

      // ── Snap checks ───────────────────────────────────────────────────
      case "timeline:toggle-snap":
        return { checked: snapToGrid };
      case "timeline:set-snap-off":
        return { checked: !snapToGrid };
      case "timeline:set-snap-bar":
      case "timeline:set-snap-beat":
      case "timeline:set-snap-eighth":
      case "timeline:set-snap-sixteenth":
      case "timeline:set-snap-thirty-second":
        return { checked: false };

      // ── Panel visibility checks ───────────────────────────────────────
      case "panel:toggle-browser":
      case "window.show_browser":
        return { checked: panels.browser?.visible };
      case "panel:toggle-inspector":
      case "window.show_inspector":
        return { checked: panels.inspector?.visible };
      case "panel:toggle-mixer":
      case "window.show_mixer":
        return { checked: panels.mixer?.visible };
      case "panel:toggle-automation":
        return { checked: false };
      case "panel:toggle-device-panel":
        return { checked: false };

      // ── Panel dock position checks ────────────────────────────────────
      case "panel:browser-dock-left":
        return { checked: panels.browser?.dock === "left" };
      case "panel:browser-dock-right":
        return { checked: panels.browser?.dock === "right" };
      case "panel:browser-dock-bottom":
        return { checked: panels.browser?.dock === "bottom" };
      case "panel:browser-float":
        return { checked: panels.browser?.dock === "float" };

      case "panel:inspector-dock-left":
        return { checked: panels.inspector?.dock === "left" };
      case "panel:inspector-dock-right":
        return { checked: panels.inspector?.dock === "right" };
      case "panel:inspector-dock-bottom":
        return { checked: panels.inspector?.dock === "bottom" };
      case "panel:inspector-float":
        return { checked: panels.inspector?.dock === "float" };

      case "panel:mixer-dock-left":
        return { checked: panels.mixer?.dock === "left" };
      case "panel:mixer-dock-right":
        return { checked: panels.mixer?.dock === "right" };
      case "panel:mixer-dock-bottom":
        return { checked: panels.mixer?.dock === "bottom" };
      case "panel:mixer-float":
        return { checked: panels.mixer?.dock === "float" };

      // ── Arrangement tool checks ───────────────────────────────────────
      case "tools:select-pointer":
        return { checked: currentTool === "pointer" };
      case "tools:select-pen":
        return { checked: currentTool === "pen" };
      case "tools:select-cut":
        return { checked: currentTool === "cut" };
      case "tools:select-glue":
        return { checked: currentTool === "glue" };
      case "tools:select-mute":
        return { checked: currentTool === "mute" };
      case "tools:select-time":
        return { checked: currentTool === "time" };
      case "tools:select-automation":
        return { checked: currentTool === "automation" };

      // ── Context-aware enabled states ──────────────────────────────────
      case "edit:duplicate":
      case "edit:copy":
      case "edit:cut":
        return { enabled: hasClipSel };
      case "edit:delete":
        return { enabled: hasClipSel || hasTrackSel };
      case "clip:split-at-playhead":
        return { enabled: hasClipSel };
      case "edit:select-track-clips":
        return { enabled: hasTrackSel };
      case "track:duplicate":
      case "track:rename":
      case "track:delete":
        return { enabled: hasTrackSel };

      // stubs that should stay disabled
      case "track:freeze":
      case "track:flatten":
        return { enabled: false };

      default:
        return {};
    }
  };

  const handleMenuAction = (item: CommandMenuItem) => {
    setOpenMenu(null);

    switch (item.action) {
      case "file:import-audio":
        onImport?.();
        break;
      case "project:open":
        runAction("project:open");
        break;
      case "project:save":
        if (onSave) onSave();
        else runAction("project:save");
        break;
      case "transport:play-pause":
        if (isPlaying) handlePause();
        else void handlePlay();
        break;
      case "transport:stop":
        handleStop();
        break;
      case "transport:go-to-start":
        transport.seek(0);
        break;
      case "transport:toggle-loop":
        toggleLoop();
        break;
      case "timeline:toggle-snap":
        toggleSnapToGrid();
        break;
      case "panel:toggle-inspector":
      case "window.show_inspector":
        togglePanel("inspector");
        break;
      case "panel:toggle-mixer":
      case "window.show_mixer":
        togglePanel("mixer");
        break;
      case "panel:toggle-browser":
      case "window.show_browser":
        togglePanel("browser");
        break;
      default:
        break;
    }
  };

  return (
    <div
      ref={barRef}
      className="drag-region-app relative z-[100] flex h-9 shrink-0 select-none items-stretch border-b border-daw-border bg-daw-sunken px-2 pr-35 shadow-[0_8px_24px_rgba(0,0,0,0.22)]"
    >
      <div className="flex w-full min-w-0 items-center justify-between gap-4">
        <div className="flex min-w-0 flex-1 items-center gap-2">
          <div className="app-no-drag flex shrink-0 items-center gap-0.5">
            <div className="p-1 flex items-center">
              <div
                aria-hidden="true"
                className="h-4 w-4 shrink-0 select-none bg-center bg-contain bg-no-repeat"
                style={{
                  backgroundImage: `url(${logoApp})`,
                  WebkitUserDrag: "none",
                  userSelect: "none",
                  pointerEvents: "none",
                } as React.CSSProperties}
              />
            </div>

            {APP_MENUS.map((menu) => (
              <div key={menu.id} className="relative">
                <TopMenuButton
                  label={menu.label}
                  open={openMenu === menu.id}
                  onClick={() => {
                    setOpenMenu((current) => (current === menu.id ? null : menu.id));
                    setProjectDropdownOpen(false);
                  }}
                  onMouseEnter={() => {
                    if (openMenu) setOpenMenu(menu.id);
                  }}
                />
                {openMenu === menu.id ? (
                  <MenuPanel items={menu.children} onAction={handleMenuAction} itemState={getMenuItemState} />
                ) : null}
              </div>
            ))}
          </div>

          <div className="h-5 w-px shrink-0 bg-daw-border" />
          <div ref={projectBtnRef} className="relative flex min-w-0 max-w-[220px] items-center px-1">
            <button
              type="button"
              onClick={() => {
                setProjectDropdownOpen((v) => !v);
                setOpenMenu(null);
              }}
              title={project.name}
              className={[
                "app-no-drag flex min-w-0 items-center gap-1 rounded px-1.5 py-0.5 transition-colors",
                projectDropdownOpen
                  ? "bg-daw-surface-high text-daw-text"
                  : "text-daw-text hover:bg-daw-surface-high",
              ].join(" ")}
            >
              <span className="min-w-0 truncate text-left text-[12px] font-semibold leading-tight">
                {project.name}
              </span>
              <ChevronDown
                size={10}
                className={[
                  "shrink-0 text-daw-faint transition-transform",
                  projectDropdownOpen ? "rotate-180" : "",
                ].join(" ")}
              />
            </button>
            <span className="ml-1.5 shrink-0 whitespace-nowrap text-[8px] font-medium uppercase tracking-wide text-daw-faint">
              {saveStatus === "unsaved" ? "Unsaved" :
               saveStatus === "saving" ? "Saving..." :
               saveStatus === "error" ? "Error" :
               "Saved"}
            </span>
            {projectDropdownOpen && (
              <ProjectDropdown onClose={() => setProjectDropdownOpen(false)} />
            )}
          </div>
        </div>

        <div className="app-no-drag flex min-w-0 shrink-0 flex-wrap items-center justify-end gap-0.5 sm:flex-nowrap">
          <IconBtn
            icon={SkipBack}
            label="Return to start [Enter]"
            onClick={() => transport.seek(0)}
          />
          {isPlaying ? (
            <IconBtn icon={Pause} label="Pause [Space]" active onClick={handlePause} />
          ) : (
            <IconBtn icon={Play} label="Play [Space]" onClick={handlePlay} size={15} />
          )}
          <IconBtn icon={Square} label="Stop [Enter]" onClick={handleStop} disabled={!isPlaying && playheadTime === 0} />
          <IconBtn icon={Circle} label="Record" accent danger size={12} />
          <IconBtn icon={Repeat2} label="Loop [L]" active={loopEnabled} onClick={toggleLoop} size={13} />
          <IconBtn icon={Timer} label="Metronome [K]" active={metronomeEnabled} onClick={toggleMetronome} size={13} />

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

          <div className="flex gap-1">
            <IconBtn icon={FolderOpen} label="Toggle Browser [B]" active={panels.browser?.visible} onClick={() => togglePanel("browser")} />
            <IconBtn icon={PanelBottom} label="Toggle Mixer [M]" active={panels.mixer?.visible} onClick={() => togglePanel("mixer")} />
            <IconBtn icon={PanelRight} label="Toggle Inspector [I]" active={panels.inspector?.visible} onClick={() => togglePanel("inspector")} />
          </div>

          <Divider />

          <IconBtn icon={FolderOpen} label="Import Audio" onClick={onImport} />
          <IconBtn icon={Save} label="Save Project" onClick={onSave} />
          <IconBtn icon={Share2} label="Share" disabled />
        </div>
      </div>
    </div>
  );
}
