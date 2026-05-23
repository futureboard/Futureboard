import {
  Bug,
  Check,
  ChevronDown,
  ChevronRight,
  Circle,
  FolderOpen,
  MoreHorizontal,
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
import {
  Fragment,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";
import { activeAudioEngine } from "../engine/activeAudioEngine";
import { APP_MENUS, type AppMenuGroup, type AppMenuItem } from "../menu/menuItems";
import { runAction } from "../menu/actionRunner";
import {
  buildSelectionState,
  canDeleteSelection,
  canDuplicateSelection,
  canCopySelection,
} from "../store/selectionSelectors";
import { useProjectStore } from "../store/projectStore";
import { useTransportStore } from "../store/transportStore";
import { useMetronomeStore } from "../store/metronomeStore";
import { useAudioSettingsStore } from "../store/audioSettingsStore";
import { useUIStore } from "../store/uiStore";
import { DawSelect } from "./ui/DawSelect";
import { NumberInput } from "./ui/NumberInput";
import { formatBarBeatTick } from "../utils/musicalTime";
import { ProjectDropdown } from "./project/ProjectDropdown";
import { platform } from "../platform";
import { commitRecordingResults } from "../engine/RecordingManager";

const _isElectron = platform.kind === "electron";
const _isMac = _isElectron && typeof window !== "undefined" && window.dawElectron?.platform === "darwin";
const _isLinux = _isElectron && typeof window !== "undefined" && window.dawElectron?.platform === "linux";
// macOS: traffic lights span ~72px from the left edge (no WCO env vars available on mac).
// Windows: WCO buttons on the right, ~140px wide — pr-35 stays unchanged.
// Linux: WCO button size/side varies by DE; use CSS env(titlebar-area-*) via wcoStyle instead.
const WCO_CLASS = _isElectron && !_isLinux
  ? _isMac ? "pl-[72px] pr-0" : "pr-35"
  : "";
// Linux-only: env(titlebar-area-x/width) is set by Electron when titleBarOverlay is active
// and correctly reflects the WCO geometry regardless of which side the DE places buttons on.
const wcoStyle = _isLinux
  ? {
      paddingLeft: "max(8px, env(titlebar-area-x, 0px))",
      paddingRight: "max(8px, calc(100vw - env(titlebar-area-x, 0px) - env(titlebar-area-width, 100vw)))",
    }
  : undefined;

// ─── Constants ─────────────────────────────────────────────────────────────
const TIME_SIG_NUMERATORS = [2, 3, 4, 5, 6, 7, 8, 9, 12];
const TIME_SIG_DENOMINATORS = [2, 4, 8, 16];

const FULL_MENU_MIN_WIDTH = 1600; // ≥1600px: all menu buttons visible, no ⋯
const OVERFLOW_ONLY_WIDTH = 1400; // <1400px: hamburger-only (all menus in ⋯)

const MENU_WIDTH_EST = 260;  // px — used for right-edge clamping
// const MENU_HEIGHT_EST = 500; // px — used for bottom-edge clamping

// ─── Types ──────────────────────────────────────────────────────────────────
type MenuLayoutMode = "full" | "partial" | "overflow";

type CommandMenuItem = Extract<AppMenuItem, { type?: "item" }>;

/**
 * One entry per open menu layer.
 * layers[0]   = the menu-bar button (or ⋯ button) that was clicked
 * layers[N>0] = the submenu-trigger row that was hovered at depth N
 */
type OpenMenuLayer = {
  id: string;
  depth: number;
  anchorRect: DOMRect;
};

// ─── Positioning helpers ────────────────────────────────────────────────────
/** Position the root panel (appears below the triggering button). */
function calcRootStyle(rect: DOMRect): React.CSSProperties {
  const left = Math.max(4, Math.min(rect.left, window.innerWidth - MENU_WIDTH_EST - 4));
  return { position: "fixed", top: rect.bottom + 2, left, zIndex: 9999 };
}

/**
 * Position a nested submenu (appears to the right of the trigger row).
 * Flips left if the submenu would overflow the right edge.
 * Clamps vertically so it stays within the viewport.
 */
function calcSubmenuStyle(anchorRect: DOMRect, depth: number): React.CSSProperties {
  const vpW = window.innerWidth;
  const vpH = window.innerHeight;

  const preferredLeft = anchorRect.right + 4;
  const left =
    preferredLeft + MENU_WIDTH_EST > vpW
      ? Math.max(4, anchorRect.left - MENU_WIDTH_EST - 4)
      : preferredLeft;

  // Align top with the trigger row. Only push up enough to keep at least
  // 80px of the menu visible — the panel's own max-h + overflow-y-auto
  // handles the case where content is taller than the remaining space.
  const MIN_VISIBLE = 80;
  const top = Math.max(4, Math.min(anchorRect.top, vpH - MIN_VISIBLE - 4));

  return { position: "fixed", top, left, zIndex: 9999 + depth };
}

// ─── Small UI helpers ───────────────────────────────────────────────────────
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
        open
          ? "bg-daw-surface-high text-daw-text"
          : "text-daw-dim hover:bg-daw-surface-high hover:text-daw-text",
      ].join(" ")}
    >
      {label}
    </button>
  );
}

function ReportBugBtn() {
  return (
    <button
      type="button"
      title="Report a bug"
      onClick={() =>
        window.open("https://tally.so/r/816zak", "_blank")
      }
      className="app-no-drag flex h-7 items-center gap-1.5 rounded-md border px-2 text-[10px] font-semibold transition-colors"
      style={{
        background: "rgba(251,191,36,0.07)",
        borderColor: "rgba(251,191,36,0.22)",
        color: "rgba(251,191,36,0.70)",
      }}
      onMouseEnter={(e) => {
        (e.currentTarget as HTMLButtonElement).style.background = "rgba(251,191,36,0.14)";
        (e.currentTarget as HTMLButtonElement).style.borderColor = "rgba(251,191,36,0.40)";
        (e.currentTarget as HTMLButtonElement).style.color = "rgba(251,191,36,0.95)";
      }}
      onMouseLeave={(e) => {
        (e.currentTarget as HTMLButtonElement).style.background = "rgba(251,191,36,0.07)";
        (e.currentTarget as HTMLButtonElement).style.borderColor = "rgba(251,191,36,0.22)";
        (e.currentTarget as HTMLButtonElement).style.color = "rgba(251,191,36,0.70)";
      }}
    >
      <Bug size={11} />
      <span>Report bug</span>
    </button>
  );
}

// ─── Recursive MenuPanel ────────────────────────────────────────────────────
/**
 * Generic recursive menu panel.
 *
 * Path semantics:
 *   depth   = which index in openPath this panel's items control.
 *   depth 1 = root panel  (openPath[1] = which of my children is open)
 *   depth 2 = first child (openPath[2] = which of its children is open)
 *   …
 *
 * A submenu child is visible when openPath[depth] === item.id.
 * Hovering a submenu trigger sets openPath[depth] = item.id and stores the
 * trigger's DOMRect in layers[depth], which positions the child portal.
 * Hovering a leaf item clears openPath[depth+] and layers[depth+].
 */
function MenuPanel({
  items,
  depth,
  openPath,
  layers,
  onPathChange,
  getItemState,
  onAction,
}: {
  items: AppMenuGroup["children"];
  depth: number;
  openPath: string[];
  layers: OpenMenuLayer[];
  onPathChange: (newPath: string[], newLayers: OpenMenuLayer[]) => void;
  getItemState: (item: AppMenuItem) => Partial<Pick<CommandMenuItem, "checked" | "enabled">>;
  onAction: (item: CommandMenuItem) => void;
}) {
  return (
    <div className="min-w-[13rem] max-h-[70vh] overflow-y-auto text-xs rounded-md border border-daw-border bg-daw-surface p-1 shadow-[0_12px_36px_rgba(0,0,0,0.52)]">
      {items.map((item) => {
        if (item.type === "separator") {
          return <div key={item.id} className="my-0.5 h-px bg-daw-border" />;
        }

        const state = getItemState(item);
        const enabled = state.enabled ?? item.enabled ?? true;

        // ── Submenu item ──────────────────────────────────────────────────
        if (item.type === "submenu") {
          const isOpen = openPath[depth] === item.id;
          const childLayer = isOpen ? layers[depth] : undefined;

          return (
            <Fragment key={item.id}>
              <button
                type="button"
                disabled={!enabled}
                onPointerEnter={(e) => {
                  if (!enabled) {
                    onPathChange(openPath.slice(0, depth), layers.slice(0, depth));
                    return;
                  }
                  const rect = e.currentTarget.getBoundingClientRect();
                  onPathChange(
                    [...openPath.slice(0, depth), item.id],
                    [...layers.slice(0, depth), { id: item.id, depth, anchorRect: rect }],
                  );
                }}
                className={[
                  "flex h-6 w-full items-center gap-1.5 rounded px-2 text-left text-[11px] text-daw-text transition-colors hover:bg-daw-surface-high disabled:cursor-not-allowed disabled:opacity-35",
                  isOpen ? "bg-daw-surface-high" : "",
                ].join(" ")}
              >
                <span className="w-3 shrink-0" />
                <span className="min-w-0 flex-1 truncate">{item.label}</span>
                <ChevronRight size={11} className="shrink-0 text-daw-faint" />
              </button>

              {/* Child panel rendered into document.body to avoid any overflow clipping */}
              {isOpen && childLayer &&
                createPortal(
                  <div
                    data-daw-menu
                    style={calcSubmenuStyle(childLayer.anchorRect, depth)}
                  >
                    <MenuPanel
                      items={item.children}
                      depth={depth + 1}
                      openPath={openPath}
                      layers={layers}
                      onPathChange={onPathChange}
                      getItemState={getItemState}
                      onAction={onAction}
                    />
                  </div>,
                  document.body,
                )}
            </Fragment>
          );
        }

        // ── Leaf command item ─────────────────────────────────────────────
        const checked = state.checked ?? item.checked ?? false;

        return (
          <button
            key={item.id}
            type="button"
            disabled={!enabled}
            onPointerEnter={() => {
              if (openPath[depth] !== undefined) {
                onPathChange(openPath.slice(0, depth), layers.slice(0, depth));
              }
            }}
            onClick={() => {
              if (enabled) onAction(item);
            }}
            className={[
              "flex h-6 w-full items-center gap-1.5 rounded px-2 text-left text-[11px] transition-colors hover:bg-daw-surface-high disabled:cursor-not-allowed disabled:opacity-35",
              item.danger ? "text-daw-red" : "text-daw-text",
            ].join(" ")}
          >
            <span className="flex w-3 shrink-0 items-center justify-center">
              {checked && <Check size={10} className="text-daw-accent" />}
            </span>
            <span className="min-w-0 flex-1 truncate">{item.label}</span>
            {item.accelerator && (
              <span className="shrink-0 pl-4 text-right text-[10px] text-daw-faint">{item.accelerator}</span>
            )}
          </button>
        );
      })}
    </div>
  );
}

// ─── TransportBar ───────────────────────────────────────────────────────────
export function TransportBar({ onImport, onSave }: { onImport?: () => void; onSave?: () => void }) {
  const { isPlaying, playheadTime, setIsPlaying, isRecording, recordStartBeat: _recordStartBeat, setIsRecording } = useTransportStore();
  const { project, setBpm, setTimeSignature } = useProjectStore();
  const {
    panels,
    togglePanel,
    loopEnabled,
    toggleLoop,
    snapToGrid,
    arrangementGridDivision,
    currentTool,
    selectedClipIds,
    selectedTrackId,
    focusedPanel,
    selectedBrowserFileId,
  } = useUIStore();
  const { enabled: metronomeEnabled, toggle: toggleMetronome, countInEnabled } =
    useMetronomeStore();
  const audioInputDeviceId = useAudioSettingsStore((s) => s.audioInputDeviceId);

  // ── Layout mode ───────────────────────────────────────────────────────────
  const [layoutMode, setLayoutMode] = useState<MenuLayoutMode>("partial");
  const [visibleMenuCount, setVisibleMenuCount] = useState(APP_MENUS.length);

  // ── Path-based open menu state ─────────────────────────────────────────────
  //
  // openPath examples:
  //   full mode,     Edit open:                   ["edit"]
  //   full mode,     Edit → Snap Settings:        ["edit", "snapSettings"]
  //   overflow mode, ⋯ open:                      ["overflow"]
  //   overflow mode, ⋯ → Edit:                    ["overflow", "edit"]
  //   overflow mode, ⋯ → Edit → Snap Settings:   ["overflow", "edit", "snapSettings"]
  //
  // layers[N].anchorRect positions the panel at depth N+1.
  const [openPath, setOpenPath] = useState<string[]>([]);
  const [layers, setLayers] = useState<OpenMenuLayer[]>([]);
  const [projectDropdownOpen, setProjectDropdownOpen] = useState(false);

  // ── Refs ──────────────────────────────────────────────────────────────────
  const barRef = useRef<HTMLDivElement>(null);
  const menuAreaRef = useRef<HTMLDivElement>(null);
  const menuBtnRefs = useRef<(HTMLDivElement | null)[]>([]);
  const menuBtnWidths = useRef<number[]>([]);
  const overflowBtnRef = useRef<HTMLDivElement>(null);

  const timeSig = project.timeSignature ?? { numerator: 4, denominator: 4 };
  const saveStatus = useUIStore((s) => s.saveStatus);
  const waveformStatus = useProjectStore((s) => s.waveformStatus);
  const missingAssetCount = project.files.filter((file) => waveformStatus.get(file.id) === "missing" || file.storageProvider === "missing").length;

  // ── Close helper ──────────────────────────────────────────────────────────
  const closeAllMenus = useCallback(() => {
    setOpenPath([]);
    setLayers([]);
  }, []);

  const updatePath = useCallback((newPath: string[], newLayers: OpenMenuLayer[]) => {
    setOpenPath(newPath);
    setLayers(newLayers);
  }, []);

  // ── Close menus when layout mode changes ──────────────────────────────────
  useEffect(() => {
    closeAllMenus();
  }, [layoutMode, closeAllMenus]);

  // ── Measure button widths once on mount (all buttons visible at this point) ──
  useLayoutEffect(() => {
    menuBtnWidths.current = menuBtnRefs.current.map((el) => el?.offsetWidth ?? 0);
  }, []);

  // ── ResizeObserver + hard breakpoints ─────────────────────────────────────
  useEffect(() => {
    const container = menuAreaRef.current;
    if (!container) return;
    const OVERFLOW_BTN_W = 34;

    const check = () => {
      const windowW = window.innerWidth;

      if (windowW >= FULL_MENU_MIN_WIDTH) {
        setLayoutMode("full");
        setVisibleMenuCount(APP_MENUS.length);
        return;
      }

      if (windowW < OVERFLOW_ONLY_WIDTH) {
        setLayoutMode("overflow");
        setVisibleMenuCount(0);
        return;
      }

      setLayoutMode("partial");
      if (menuBtnWidths.current.every((w) => w === 0)) {
        menuBtnWidths.current = menuBtnRefs.current.map((el) => el?.offsetWidth ?? 0);
      }
      const available = container.clientWidth - OVERFLOW_BTN_W;
      let acc = 0;
      let count = 0;
      for (const w of menuBtnWidths.current) {
        if (acc + w <= available) {
          acc += w;
          count++;
        } else break;
      }
      const totalW = menuBtnWidths.current.reduce((s, w) => s + w, 0);
      setVisibleMenuCount(totalW <= container.clientWidth ? APP_MENUS.length : count);
    };

    const ro = new ResizeObserver(check);
    ro.observe(container);
    window.addEventListener("resize", check);
    check();
    return () => {
      ro.disconnect();
      window.removeEventListener("resize", check);
    };
  }, []);

  // ── Click-outside (portal-aware via data-daw-menu attribute) ─────────────
  const anyMenuOpen = openPath.length > 0;

  useEffect(() => {
    if (!anyMenuOpen) return;
    const handleMouseDown = (e: MouseEvent) => {
      const target = e.target as HTMLElement;
      if (barRef.current?.contains(target)) return;
      if (target.closest("[data-daw-menu]")) return;
      closeAllMenus();
    };
    window.addEventListener("mousedown", handleMouseDown);
    return () => window.removeEventListener("mousedown", handleMouseDown);
  }, [anyMenuOpen, closeAllMenus]);

  // ── Escape key ────────────────────────────────────────────────────────────
  useEffect(() => {
    if (!anyMenuOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeAllMenus();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [anyMenuOpen, closeAllMenus]);

  // ── Menu bar button handlers ───────────────────────────────────────────────
  const handleMenuBtnClick = (menu: AppMenuGroup, i: number) => {
    if (openPath[0] === menu.id) {
      closeAllMenus();
    } else {
      const rect = menuBtnRefs.current[i]?.getBoundingClientRect();
      if (rect) {
        setOpenPath([menu.id]);
        setLayers([{ id: menu.id, depth: 0, anchorRect: rect }]);
      }
      setProjectDropdownOpen(false);
    }
  };

  const handleMenuBtnHover = (menu: AppMenuGroup, i: number) => {
    // Hover-switch: if any full-mode menu is open and this is a different one
    if (openPath.length > 0 && openPath[0] !== "overflow" && openPath[0] !== menu.id) {
      const rect = menuBtnRefs.current[i]?.getBoundingClientRect();
      if (rect) {
        setOpenPath([menu.id]);
        setLayers([{ id: menu.id, depth: 0, anchorRect: rect }]);
      }
    }
  };

  const handleOverflowBtnClick = () => {
    if (openPath[0] === "overflow") {
      closeAllMenus();
    } else {
      const rect = overflowBtnRef.current?.getBoundingClientRect();
      if (rect) {
        setOpenPath(["overflow"]);
        setLayers([{ id: "overflow", depth: 0, anchorRect: rect }]);
      }
      setProjectDropdownOpen(false);
    }
  };

  // ── Transport ─────────────────────────────────────────────────────────────
  const handlePlay = async () => {
    await activeAudioEngine.play();
    setIsPlaying(true);
  };

  const handlePause = () => {
    activeAudioEngine.pause();
    setIsPlaying(false);
  };

  const handleStop = () => {
    activeAudioEngine.stop();
    setIsPlaying(false);
  };

  const handleRecord = async () => {
    if (isRecording) {
      setIsRecording(false);
      try {
        const results = await activeAudioEngine.stopRecording();
        await commitRecordingResults(results);
      } catch (e) {
        console.error("[TransportBar] stopRecording error:", e);
      }
      return;
    }

    const projectRoot = platform.folderProject.getProjectRoot();
    if (!projectRoot) {
      console.warn("[TransportBar] Recording requires a saved folder project");
      return;
    }

    const armedTracks = project.tracks.filter((t) => t.armed && t.type === "audio");
    if (armedTracks.length === 0) {
      console.warn("[TransportBar] No armed audio tracks — arm at least one track before recording");
      return;
    }

    const startBeat = playheadTime * (project.bpm / 60);
    const sessionId = `rec-${Date.now()}`;

    const tracks = armedTracks.map((t) => {
      const pair = t.routing?.input?.channelPair;
      const inputChannels: number[] = pair ? [pair[0] - 1, pair[1] - 1] : [0, 1];
      return { trackId: t.id, inputChannels, name: t.name };
    });

    try {
      await activeAudioEngine.startRecording({
        projectRoot,
        sessionId,
        bpm: project.bpm,
        startBeat,
        sampleRate: project.sampleRate,
        inputDeviceId: audioInputDeviceId ?? null,
        tracks,
      });
      setIsRecording(true, startBeat);
      if (!isPlaying) {
        await activeAudioEngine.play(playheadTime);
        setIsPlaying(true);
      }
    } catch (e) {
      console.error("[TransportBar] startRecording error:", e);
    }
  };

  // Build a unified selection snapshot for menu enabled-state predicates.
  // This is purely read-only — does not affect any store.
  const selectionState = buildSelectionState({
    focusedPanel,
    selectedTrackId,
    selectedClipIds,
    selectedBrowserFileId,
  });

  const hasClipSel  = selectedClipIds.length > 0;
  const hasTrackSel = !!selectedTrackId;

  // ── Item state ────────────────────────────────────────────────────────────
  const getMenuItemState = (item: AppMenuItem) => {
    if (item.type === "separator" || item.type === "submenu") return {};

    switch (item.action) {
      case "transport:toggle-loop":
        return { checked: loopEnabled };
      case "transport:toggle-metronome":
        return { checked: metronomeEnabled };
      case "transport:toggle-count-in":
        return { checked: countInEnabled };

      case "timeline:toggle-snap":
        return { checked: snapToGrid };
      case "timeline:set-snap-off":
        return { checked: !snapToGrid };
      case "timeline:set-snap-bar":
        return { checked: snapToGrid && arrangementGridDivision === "1bar" };
      case "timeline:set-snap-whole":
        return { checked: snapToGrid && arrangementGridDivision === "1/1" };
      case "timeline:set-snap-beat":
        return { checked: snapToGrid && arrangementGridDivision === "1/4" };
      case "timeline:set-snap-eighth":
        return { checked: snapToGrid && arrangementGridDivision === "1/8" };
      case "timeline:set-snap-sixteenth":
        return { checked: snapToGrid && arrangementGridDivision === "1/16" };
      case "timeline:set-snap-thirty-second":
        return { checked: snapToGrid && arrangementGridDivision === "1/32" };
      case "timeline:set-snap-sixty-fourth":
        return { checked: snapToGrid && arrangementGridDivision === "1/64" };
      case "timeline:set-snap-quarter-triplet":
        return { checked: snapToGrid && arrangementGridDivision === "1/4T" };
      case "timeline:set-snap-eighth-triplet":
        return { checked: snapToGrid && arrangementGridDivision === "1/8T" };
      case "timeline:set-snap-sixteenth-triplet":
        return { checked: snapToGrid && arrangementGridDivision === "1/16T" };

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
      case "panel:toggle-device-panel":
        return { checked: false };

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

      case "edit:duplicate":
        return { enabled: canDuplicateSelection(selectionState) };
      case "edit:copy":
      case "edit:cut":
        return { enabled: canCopySelection(selectionState) };
      case "edit:delete":
        return { enabled: canDeleteSelection(selectionState) };
      case "clip:split-at-playhead":
        return { enabled: hasClipSel };
      case "edit:select-track-clips":
        return { enabled: hasTrackSel };
      case "track:duplicate":
      case "track:rename":
      case "track:delete":
        return { enabled: hasTrackSel };

      case "track:freeze":
      case "track:flatten":
        return { enabled: false };

      default:
        return {};
    }
  };

  // ── Action handler ────────────────────────────────────────────────────────
  const handleMenuAction = (item: CommandMenuItem) => {
    closeAllMenus();

    switch (item.action) {
      case "file:import-audio":
        onImport?.();
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
      default:
        if (item.action) runAction(item.action);
        break;
    }
  };

  // ── Derived: root portal ──────────────────────────────────────────────────
  //
  // The root panel is always at depth=1.
  // layers[0] = anchor of the button that was clicked.
  // layers[N] = anchor of the trigger hovered at depth N, positioning the panel at depth N+1.
  //
  // In overflow mode the root panel lists all APP_MENUS as submenus.
  // In full mode the root panel lists the selected menu's children.
  const rootId = openPath[0] ?? null;
  const rootLayer = layers[0] ?? null;

  const rootItems: AppMenuGroup["children"] =
    rootId === "overflow"
      ? APP_MENUS.map((g) => ({
          type: "submenu" as const,
          id: g.id,
          label: g.label,
          children: g.children,
        }))
      : (APP_MENUS.find((m) => m.id === rootId)?.children ?? []);

  const showOverflowBtn = visibleMenuCount < APP_MENUS.length;

  // ─────────────────────────────────────────────────────────────────────────
  return (
    <div
      ref={barRef}
      className={`drag-region-app relative z-[100] flex h-9 shrink-0 select-none items-stretch border-b border-daw-border bg-daw-sunken px-2 shadow-[0_8px_24px_rgba(0,0,0,0.22)] ${WCO_CLASS}`}
      style={wcoStyle}
    >
      <div className="flex w-full min-w-0 items-center justify-between gap-4">
        <div className="flex min-w-0 flex-1 items-center gap-2">
          {!_isMac && (
          <div className="app-no-drag flex min-w-0 shrink items-center gap-0.5">

            {/* Menu buttons — overflow-hidden clips anything that doesn't fit */}
            <div
              ref={menuAreaRef}
              className="flex min-w-0 shrink items-center gap-0.5 overflow-hidden"
            >
              {APP_MENUS.map((menu, i) => (
                <div
                  key={menu.id}
                  ref={(el) => { menuBtnRefs.current[i] = el; }}
                  className={["relative shrink-0 ", i >= visibleMenuCount ? "hidden" : ""].join(" ")}
                >
                  <TopMenuButton
                    label={menu.label}
                    open={openPath[0] === menu.id}
                    onClick={() => handleMenuBtnClick(menu, i)}
                    onMouseEnter={() => handleMenuBtnHover(menu, i)}
                  />
                </div>
              ))}
            </div>

            {/* ⋯ overflow button — sibling of overflow-hidden so its portal renders above */}
            {showOverflowBtn && (
              <div ref={overflowBtnRef} className="shrink-0">
                <button
                  type="button"
                  onClick={handleOverflowBtnClick}
                  title="More menus"
                  className={[
                    "app-no-drag flex h-6 w-6 items-center justify-center rounded transition-colors",
                    openPath[0] === "overflow"
                      ? "bg-daw-surface-high text-daw-text"
                      : "text-daw-dim hover:bg-daw-surface-high hover:text-daw-text",
                  ].join(" ")}
                >
                  <MoreHorizontal size={13} />
                </button>
              </div>
            )}
          </div>
          )}

          {!_isMac && <div className="h-5 w-px shrink-0 bg-daw-border" />}

          <div className="relative flex min-w-0 max-w-[220px] items-center px-1">
            <button
              type="button"
              onClick={() => {
                setProjectDropdownOpen((v) => !v);
                closeAllMenus();
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
              {saveStatus === "unsaved"
                ? "Unsaved"
                : saveStatus === "saving"
                  ? "Saving..."
                  : saveStatus === "error"
                    ? "Error"
                    : missingAssetCount > 0
                      ? `Saved · ${missingAssetCount} missing`
                      : "Saved"}
            </span>
            {projectDropdownOpen && (
              <ProjectDropdown onClose={() => setProjectDropdownOpen(false)} />
            )}
          </div>
        </div>

        {/* Right-side transport controls */}
        <div className="app-no-drag flex min-w-0 shrink-0 flex-wrap items-center justify-end gap-0.5 sm:flex-nowrap">
          <IconBtn icon={SkipBack} label="Return to start [Enter]" onClick={() => activeAudioEngine.seekSeconds(0)} />
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
          <IconBtn
            icon={Circle}
            label={isRecording ? "Stop Recording" : "Record"}
            accent
            danger
            active={isRecording}
            size={12}
            onClick={handleRecord}
          />
          <IconBtn icon={Repeat2} label="Loop [L]" active={loopEnabled} onClick={toggleLoop} size={13} />
          <IconBtn
            icon={Timer}
            label="Metronome [K]"
            active={metronomeEnabled}
            onClick={toggleMetronome}
            size={13}
          />

          <Divider />

          <div className="flex h-7 min-w-[6.5rem] items-center justify-center px-1 text-[13px] font-semibold tabular-nums text-daw-text sm:min-w-[7.75rem]">
            {formatBarBeatTick(playheadTime, project.bpm, timeSig)}
          </div>

          <Divider />

          <div className="flex h-7 items-center gap-1 px-1">
            <span className="text-[8px] font-medium text-daw-faint">BPM</span>
            <NumberInput
              className="w-12 !h-5"
              align="center"
              min={20}
              max={300}
              value={project.bpm}
              ariaLabel="Project tempo BPM"
              onChange={(value) => setBpm(Math.round(value) || 120)}
            />
          </div>

          <div className="flex h-7 items-center gap-0.5 px-1">
            <DawSelect
              value={String(timeSig.numerator)}
              onChange={(val) =>
                setTimeSignature({ ...timeSig, numerator: parseInt(val) })
              }
              className="!h-5 !w-5 !bg-transparent !border-none !px-0"
              hideChevron
              options={TIME_SIG_NUMERATORS.map((n) => ({
                value: String(n),
                label: String(n),
              }))}
            />
            <span className="text-[10px] opacity-25">/</span>
            <DawSelect
              value={String(timeSig.denominator)}
              onChange={(val) =>
                setTimeSignature({ ...timeSig, denominator: parseInt(val) })
              }
              className="!h-5 !w-4 !bg-transparent !border-none !px-0"
              hideChevron
              options={TIME_SIG_DENOMINATORS.map((n) => ({
                value: String(n),
                label: String(n),
              }))}
            />
          </div>

          <Divider />

          <div className="flex gap-1">
            <IconBtn
              icon={FolderOpen}
              label="Toggle Browser [B]"
              active={panels.browser?.visible}
              onClick={() => togglePanel("browser")}
            />
            <IconBtn
              icon={PanelBottom}
              label="Toggle Mixer [M]"
              active={panels.mixer?.visible}
              onClick={() => togglePanel("mixer")}
            />
            <IconBtn
              icon={PanelRight}
              label="Toggle Inspector [I]"
              active={panels.inspector?.visible}
              onClick={() => togglePanel("inspector")}
            />
          </div>

          <Divider />

          <IconBtn icon={FolderOpen} label="Import Audio" onClick={onImport} />
          <IconBtn icon={Save} label="Save Project" onClick={onSave} />
          <IconBtn icon={Share2} label="Share" disabled />

          <Divider />

          <ReportBugBtn />
        </div>
      </div>

      {/* ── Root portal menu ───────────────────────────────────────────────────
          Rendered at document.body so no overflow-hidden ancestor can clip it.
          data-daw-menu lets the click-outside handler identify menu elements. */}
      {rootId && rootLayer &&
        createPortal(
          <div data-daw-menu style={calcRootStyle(rootLayer.anchorRect)}>
            <MenuPanel
              items={rootItems}
              depth={1}
              openPath={openPath}
              layers={layers}
              onPathChange={updatePath}
              getItemState={getMenuItemState}
              onAction={handleMenuAction}
            />
          </div>,
          document.body,
        )}
    </div>
  );
}
