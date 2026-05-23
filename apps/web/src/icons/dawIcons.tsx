/**
 * Central icon registry for the DAW.
 * Lucide = general UI icons.
 * Tabler = DAW/audio-specific icons that Lucide lacks.
 *
 * Usage:  <DawIcon name="waveform" size={14} />
 * All icons render at the same visual weight (strokeWidth 1.75)
 * and inherit currentColor so they blend with surrounding text.
 */

import React from "react";

// ── Lucide (general UI) ───────────────────────────────────────────────────────
import {
  Activity,
  AlertTriangle,
  ArrowDown,
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  BarChart3,
  Binary,
  Book,
  BookOpen,
  Box,
  Bug,
  Camera,
  Check,
  ChevronDown,
  ChevronRight,
  ChevronUp,
  ChevronsDown,
  ChevronsUp,
  Circle,
  CircleAlert,
  Clipboard,
  ClipboardCopy,
  Cloud,
  CloudDownload,
  Copy,
  CopyPlus,
  CornerDownLeft,
  Cpu,
  Database,
  Download,
  FastForward,
  FileAudio2,
  FileIcon,
  FilePlus,
  Folder,
  FolderOpen,
  FolderSearch,
  Gauge,
  GitFork,
  GitMerge,
  GripVertical,
  History,
  Info,
  Keyboard,
  LayoutDashboard,
  LayoutTemplate,
  Layers,
  Link2,
  List,
  Magnet,
  Map,
  MapPin,
  Maximize,
  Maximize2,
  MessageSquare,
  Mic2,
  Minus,
  Monitor,
  MousePointer2,
  Music,
  Music2,
  Newspaper,
  Package,
  PanelBottom,
  PanelLeft,
  PanelRight,
  Pause,
  Pen,
  Pencil,
  Pin,
  Play,
  Plus,
  Plug2,
  Radio,
  RefreshCw,
  Repeat2,
  Rewind,
  RotateCcw,
  Save,
  Scissors,
  Search,
  Share2,
  Shield,
  SkipBack,
  SkipForward,
  SlidersHorizontal,
  Snowflake,
  Sparkles,
  Square,
  Terminal,
  Timer,
  Trash2,
  Undo2,
  Redo2,
  Upload,
  Volume2,
  VolumeX,
  Wand2,
  Waves,
  X,
  Zap,
  ZoomIn,
  ZoomOut,
} from "lucide-react";

// ── Tabler (DAW/audio-specific) ────────────────────────────────────────────────
import {
  IconMetronome,
  IconMicrophone2,
  IconPiano,
  IconPlug,
  IconPlugConnected,
  IconRoute,
  IconSend,
  IconWaveSine,
  IconFileMusic,
} from "@tabler/icons-react";

// ── Shared props type ─────────────────────────────────────────────────────────

export type DawIconName =
  // General
  | "file"
  | "project"
  | "save"
  | "saveAs"
  | "open"
  | "folder"
  | "history"
  | "settings"
  | "keyboard"
  | "close"
  | "add"
  | "remove"
  | "trash"
  | "undo"
  | "redo"
  | "check"
  | "chevronDown"
  | "chevronRight"
  | "gripVertical"
  | "upload"
  | "download"
  | "share"
  | "cloud"
  | "info"
  | "warning"
  | "cpu"
  | "memory"
  | "performance"
  | "zap"
  // DAW transport
  | "play"
  | "pause"
  | "stop"
  | "skipBack"
  | "repeat"
  | "record"
  | "metronome"
  | "snap"
  // Arrangement tools
  | "pointer"
  | "pen"
  | "cut"
  | "glue"
  | "mute"
  | "automation"
  | "time"
  // Track types — must cover every TrackType
  | "audio"
  | "midi"
  | "instrument"
  | "plugin"
  | "bus"
  | "return"
  | "group"
  | "master"
  // Audio / MIDI specific
  | "waveform"
  | "piano"
  | "importAudio"
  | "importMidi"
  | "exportAudio"
  | "exportStems"
  // Mixer / routing
  | "mixer"
  | "route"
  | "send"
  | "insert"
  | "effect"
  // Panels
  | "inspector"
  | "browser"
  | "editor"
  | "panelBottom"
  | "panelRight";

export type DawIconSource = "lucide" | "tabler";

type IconEntry =
  | { src: "lucide"; component: React.ElementType }
  | { src: "tabler"; component: React.ElementType };

// ── Registry ──────────────────────────────────────────────────────────────────

const REGISTRY: Record<DawIconName, IconEntry> = {
  // General
  file:         { src: "lucide", component: FileIcon },
  project:      { src: "lucide", component: FileAudio2 },
  save:         { src: "lucide", component: Save },
  saveAs:       { src: "lucide", component: Save },
  open:         { src: "lucide", component: FolderOpen },
  folder:       { src: "lucide", component: FolderOpen },
  history:      { src: "lucide", component: History },
  settings:     { src: "lucide", component: SlidersHorizontal },
  keyboard:     { src: "lucide", component: Keyboard },
  close:        { src: "lucide", component: X },
  add:          { src: "lucide", component: Plus },
  remove:       { src: "lucide", component: X },
  trash:        { src: "lucide", component: Trash2 },
  undo:         { src: "lucide", component: Undo2 },
  redo:         { src: "lucide", component: Redo2 },
  check:        { src: "lucide", component: Check },
  chevronDown:  { src: "lucide", component: ChevronDown },
  chevronRight: { src: "lucide", component: ChevronRight },
  gripVertical: { src: "lucide", component: GripVertical },
  upload:       { src: "lucide", component: Upload },
  download:     { src: "lucide", component: Download },
  share:        { src: "lucide", component: Share2 },
  cloud:        { src: "lucide", component: Cloud },
  info:         { src: "lucide", component: Info },
  warning:      { src: "lucide", component: AlertTriangle },
  cpu:          { src: "lucide", component: Cpu },
  memory:       { src: "lucide", component: Binary },
  performance:  { src: "lucide", component: Gauge },
  zap:          { src: "lucide", component: Zap },
  // Transport
  play:         { src: "lucide", component: Play },
  pause:        { src: "lucide", component: Pause },
  stop:         { src: "lucide", component: Square },
  skipBack:     { src: "lucide", component: SkipBack },
  repeat:       { src: "lucide", component: Repeat2 },
  record:       { src: "lucide", component: Circle },
  metronome:    { src: "tabler", component: IconMetronome },
  snap:         { src: "lucide", component: Magnet },
  // Arrangement tools
  pointer:      { src: "lucide", component: MousePointer2 },
  pen:          { src: "lucide", component: Pen },
  cut:          { src: "lucide", component: Scissors },
  glue:         { src: "lucide", component: Link2 },
  mute:         { src: "lucide", component: VolumeX },
  automation:   { src: "lucide", component: Activity },
  time:         { src: "lucide", component: Timer },
  // Track types — must cover every TrackType ("audio"|"midi"|"instrument"|"plugin"|"bus"|"return"|"group"|"master")
  audio:        { src: "lucide", component: Mic2 },
  midi:         { src: "tabler", component: IconPiano },
  instrument:   { src: "lucide", component: Cpu },
  plugin:       { src: "tabler", component: IconPlug },
  bus:          { src: "lucide", component: GitMerge },
  return:       { src: "lucide", component: CornerDownLeft },
  group:        { src: "lucide", component: GitFork },
  master:       { src: "lucide", component: Volume2 },
  // Audio / MIDI
  waveform:     { src: "tabler", component: IconWaveSine },
  piano:        { src: "tabler", component: IconPiano },
  importAudio:  { src: "tabler", component: IconMicrophone2 },
  importMidi:   { src: "tabler", component: IconFileMusic },
  exportAudio:  { src: "lucide", component: Download },
  exportStems:  { src: "lucide", component: Download },
  // Mixer / routing
  mixer:        { src: "lucide", component: SlidersHorizontal },
  route:        { src: "tabler", component: IconRoute },
  send:         { src: "tabler", component: IconSend },
  insert:       { src: "tabler", component: IconPlugConnected },
  effect:       { src: "lucide", component: Sparkles },
  // Panels
  inspector:    { src: "lucide", component: PanelRight },
  browser:      { src: "lucide", component: FolderOpen },
  editor:       { src: "lucide", component: Pencil },
  panelBottom:  { src: "lucide", component: PanelBottom },
  panelRight:   { src: "lucide", component: PanelRight },
};

// ── Lucide slug map ───────────────────────────────────────────────────────────
// Covers kebab-case icon names used in menuItems.ts icon fields.
// Only Lucide components here — Tabler icons belong in REGISTRY only.

const LUCIDE_SLUG_MAP: Record<string, React.ElementType> = {
  // File menu
  "file":               FileIcon,
  "file-plus":          FilePlus,
  "folder-open":        FolderOpen,
  "folder-input":       FolderSearch,
  "folder":             Folder,
  "history":            History,
  "save":               Save,
  "save-all":           Save,
  "copy":               Copy,
  "copy-plus":          CopyPlus,
  "camera":             Camera,
  "rotate-ccw":         RotateCcw,
  "refresh-ccw":        RotateCcw,
  "refresh-cw":         RefreshCw,
  "archive":            Package,
  "download":           Download,
  "upload":             Upload,
  "files":              Copy,
  "repeat":             Repeat2,
  "repeat-2":           Repeat2,
  "cloud-upload":       Cloud,
  "cloud-download":     CloudDownload,
  "download-cloud":     CloudDownload,
  "share-2":            Share2,
  "git-branch":         GitFork,
  "git-fork":           GitFork,
  "git-merge":          GitMerge,
  "x":                  X,
  "x-circle":           X,
  "power":              Zap,
  // Edit menu
  "undo-2":             Undo2,
  "redo-2":             Redo2,
  "scissors":           Scissors,
  "clipboard":          Clipboard,
  "clipboard-copy":     ClipboardCopy,
  "trash-2":            Trash2,
  "scan":               MousePointer2,
  "scan-x":             X,
  "mouse-pointer-2":    MousePointer2,
  "split":              Scissors,
  "move-left":          SkipBack,
  "move-right":         SkipForward,
  "crop":               Scissors,
  "combine":            Link2,
  "magnet":             Magnet,
  "grid-3x3":           SlidersHorizontal,
  "settings":           SlidersHorizontal,
  "settings-2":         SlidersHorizontal,
  "pencil":             Pencil,
  // MIDI menu
  "align-start-vertical": Magnet,
  "arrow-left":         ArrowLeft,
  "arrow-right":        ArrowRight,
  "arrow-up":           ArrowUp,
  "arrow-down":         ArrowDown,
  "chevron-down":       ChevronDown,
  "chevron-right":      ChevronRight,
  "chevron-up":         ChevronUp,
  "chevrons-up":        ChevronsUp,
  "chevrons-down":      ChevronsDown,
  // Project menu
  "gauge":              Gauge,
  "music":              Music,
  "music-2":            Music2,
  "activity":           Activity,
  "binary":             Binary,
  "mic":                Mic2,
  "mic-2":              Mic2,
  "cpu":                Cpu,
  "route":              GitMerge,
  "corner-down-left":   CornerDownLeft,
  "layers":             Layers,
  "layers-2":           Layers,
  "layout-template":    LayoutTemplate,
  "database":           Database,
  "package":            Package,
  "sparkles":           Sparkles,
  "bar-chart-3":        BarChart3,
  "bar-chart-4":        BarChart3,
  "map-pin":            MapPin,
  "skip-back":          SkipBack,
  "skip-forward":       SkipForward,
  "list":               List,
  "palette":            Wand2,
  "snowflake":          Snowflake,
  // Audio menu
  "play":               Play,
  "square":             Square,
  "circle":             Circle,
  "timer":              Timer,
  "badge-123":          Timer,
  "step-back":          SkipBack,
  "step-forward":       SkipForward,
  "rewind":             Rewind,
  "fast-forward":       FastForward,
  "waves":              Waves,
  "waveform":           Activity,
  "move-up-right":      ArrowUp,
  "move-down-right":    ArrowDown,
  "blend":              Sparkles,
  "volume-2":           Volume2,
  "disc-3":             Download,
  "box":                Box,
  "sliders-horizontal": SlidersHorizontal,
  "speaker":            Volume2,
  "search":             Search,
  "blocks":             Layers,
  "plug":               Plug2,
  "plug-2":             Plug2,
  "plug-zap":           Zap,
  // Window menu
  "minus":              Minus,
  "maximize":           Maximize,
  "maximize-2":         Maximize2,
  "pin":                Pin,
  "panel-right":        PanelRight,
  "panel-bottom":       PanelBottom,
  "panel-left":         PanelLeft,
  "zoom-in":            ZoomIn,
  "zoom-out":           ZoomOut,
  "layout-dashboard":   LayoutDashboard,
  "workflow":           GitFork,
  // Tools menu
  "terminal":           Terminal,
  "radio":              Radio,
  "keyboard-music":     Keyboard,
  "gamepad-2":          SlidersHorizontal,
  "shield":             Shield,
  "bug":                Bug,
  "monitor-dot":        Monitor,
  "folder-search":      FolderSearch,
  "orbit":              RefreshCw,
  "guitar":             Music,
  // Help menu
  "book-open":          BookOpen,
  "book":               Book,
  "keyboard":           Keyboard,
  "newspaper":          Newspaper,
  "map":                Map,
  "github":             Info,
  "circle-alert":       CircleAlert,
  "messages-square":    MessageSquare,
  "stethoscope":        Activity,
  "info":               Info,
  "check":              Check,
};

// ── DawIcon component ─────────────────────────────────────────────────────────

export type DawIconProps = {
  /** Semantic DawIcon name or a Lucide kebab-case slug used in menuItems.ts */
  name: DawIconName | string;
  size?: number;
  strokeWidth?: number;
  className?: string;
  color?: string;
  style?: React.CSSProperties;
};

const DEFAULT_SIZE         = 14;
const DEFAULT_STROKE_WIDTH = 1.75;

export function DawIcon({
  name,
  size = DEFAULT_SIZE,
  strokeWidth = DEFAULT_STROKE_WIDTH,
  className,
  color,
  style,
}: DawIconProps) {
  const entry = REGISTRY[name as DawIconName];

  let Component: React.ElementType | undefined;
  let src: DawIconSource | undefined;

  if (entry) {
    Component = entry.component;
    src = entry.src;
  } else {
    const slugComponent = LUCIDE_SLUG_MAP[name];
    if (slugComponent) {
      Component = slugComponent;
      src = "lucide";
    } else {
      if (import.meta.env.DEV) {
        console.warn(`[DawIcon] Unknown icon name: "${name}"`);
      }
      Component = AlertTriangle;
      src = "lucide";
    }
  }

  if (src === "tabler") {
    // Tabler uses `stroke` for stroke width — do NOT pass `strokeWidth` to avoid
    // it being forwarded as a DOM attribute.
    return (
      <Component
        size={size}
        stroke={strokeWidth}
        className={className}
        color={color}
        style={style}
        aria-hidden
      />
    );
  }

  return (
    <Component
      size={size}
      strokeWidth={strokeWidth}
      className={className}
      color={color}
      style={style}
      aria-hidden
    />
  );
}

// ── MenuDawIcon — fixed-width icon slot for menu rows ─────────────────────────

export function MenuDawIcon({ icon, size = 14 }: { icon?: string; size?: number }) {
  if (!icon) return <span className="flex w-5 shrink-0 justify-center" />;
  return (
    <span className="flex w-5 shrink-0 items-center justify-center text-daw-faint">
      <DawIcon name={icon} size={size} />
    </span>
  );
}
