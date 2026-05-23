import {
  AlertTriangle,
  ChevronRight,
  Disc3,
  FileAudio2,
  FlaskConical,
  FolderOpen,
  FolderSearch,
  HardDrive,
  LayoutTemplate,
  Layers,
  Library,
  Link2,
  Loader2,
  Pause,
  Play,
  Repeat,
  Search,
  SlidersHorizontal,
  Upload,
  X,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useProjectStore } from "../store/projectStore";
import { useSettingsStore } from "../store/settingsStore";
import { useUIStore } from "../store/uiStore";
import { BROWSER_WIDTH } from "../theme";
import type { DawFile, DawProjectAsset } from "../types/daw";
import { decodeAndAddAudioFile, importNativeAudioPathToBrowser } from "../utils/importAudioToProject";
import { audioAssetManager } from "../engine/AudioAssetManager";
import { platform } from "../platform";
import type {
  BrowserFileEntry,
  BrowserIndexStatus,
  BrowserRootEntry,
} from "../platform/platform.types";

// ─── Constants ────────────────────────────────────────────────────────────────

const EMPTY_ASSETS: DawProjectAsset[] = [];
const NATIVE_AUDIO_DRAG_TYPE = "application/x-futureboard-native-audio-path";

// ─── Types ────────────────────────────────────────────────────────────────────

type NativeBrowserPath = { path: string; name: string };

type NativeTreeEntry = {
  name: string;
  path: string;
  kind: BrowserRootEntry["kind"] | BrowserFileEntry["kind"];
  size?: number;
  mimeType?: string;
};

// ─── Format helpers ───────────────────────────────────────────────────────────

function formatBytes(bytes?: number) {
  if (!bytes || bytes <= 0) return "";
  if (bytes >= 1_048_576) return `${(bytes / 1_048_576).toFixed(1)} MB`;
  if (bytes >= 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${bytes} B`;
}

function getFormatLabel(name: string, mimeType?: string): string {
  const n = name.toLowerCase();
  if (mimeType?.includes("mpeg") || n.endsWith(".mp3")) return "MP3";
  if (mimeType?.includes("wav") || n.endsWith(".wav")) return "WAV";
  if (n.endsWith(".flac")) return "FLAC";
  if (n.endsWith(".aiff") || n.endsWith(".aif")) return "AIFF";
  if (n.endsWith(".ogg")) return "OGG";
  if (n.endsWith(".m4a")) return "M4A";
  return "Audio";
}

type BadgeStyle = { color: string; background: string; borderColor: string };

function getFormatBadgeStyle(label: string): BadgeStyle {
  switch (label) {
    case "WAV":  return { color: "#85E0A3", background: "rgba(133,224,163,0.09)", borderColor: "rgba(133,224,163,0.22)" };
    case "MP3":  return { color: "#7BC4F0", background: "rgba(123,196,240,0.09)", borderColor: "rgba(123,196,240,0.22)" };
    case "FLAC": return { color: "#B7ABFF", background: "rgba(183,171,255,0.09)", borderColor: "rgba(183,171,255,0.22)" };
    case "AIFF": return { color: "#EFA66D", background: "rgba(239,166,109,0.09)", borderColor: "rgba(239,166,109,0.22)" };
    case "OGG":  return { color: "#F4CF7A", background: "rgba(244,207,122,0.09)", borderColor: "rgba(244,207,122,0.22)" };
    case "M4A":  return { color: "#D982B6", background: "rgba(217,130,182,0.09)", borderColor: "rgba(217,130,182,0.22)" };
    default:     return { color: "#6B7888",  background: "rgba(42,50,64,0.5)",     borderColor: "rgba(58,69,84,0.7)" };
  }
}

// ─── Icon helpers ─────────────────────────────────────────────────────────────

function getFactoryFolderIcon(name: string): React.ElementType {
  const n = name.toLowerCase();
  if (n.includes("loop")) return Repeat;
  if (n.includes("sample")) return FlaskConical;
  if (n.includes("preset")) return SlidersHorizontal;
  if (n.includes("template")) return LayoutTemplate;
  if (n.includes("loop")) return Layers;
  return FolderOpen;
}

function getEntryIcon(entry: NativeTreeEntry): React.ElementType {
  if (entry.kind === "drive") return HardDrive;
  if (entry.kind === "factory") return Library;
  if (entry.kind === "factory-folder") return getFactoryFolderIcon(entry.name);
  if (entry.kind === "folder") return FolderOpen;
  return FileAudio2;
}

// ─── IndexBadge — spinner or ready dot ───────────────────────────────────────

function IndexBadge({ status }: { status?: BrowserIndexStatus }) {
  if (!status || status.status === "idle") return null;

  if (status.status === "indexing") {
    return (
      <span className="flex shrink-0 items-center gap-1">
        <Loader2 size={8} className="animate-spin" style={{ color: "#5FCED0" }} />
        <span className="text-[9px]" style={{ color: "rgba(154,167,184,0.7)" }}>
          Indexing…
        </span>
      </span>
    );
  }

  if (status.status === "done") {
    return (
      <span
        className="h-1.5 w-1.5 shrink-0 rounded-full"
        style={{ background: "rgba(133,224,163,0.55)" }}
        title={`${status.audioFiles} audio files indexed`}
      />
    );
  }

  if (status.status === "error") {
    return (
      <span className="shrink-0 text-[9px]" style={{ color: "rgba(240,122,114,0.8)" }}>
        err
      </span>
    );
  }

  return null;
}

// ─── FormatBadge ─────────────────────────────────────────────────────────────

function FormatBadge({ name, mimeType }: { name: string; mimeType?: string }) {
  const label = getFormatLabel(name, mimeType);
  const { color, background, borderColor } = getFormatBadgeStyle(label);
  return (
    <span
      className="shrink-0 rounded px-1 py-px text-[8px] font-medium leading-none"
      style={{ color, background, border: `1px solid ${borderColor}` }}
    >
      {label}
    </span>
  );
}

// ─── Section — collapsible group header ──────────────────────────────────────

function Section({
  label,
  icon: Icon,
  defaultOpen = true,
  children,
}: {
  label: string;
  icon: React.ElementType;
  defaultOpen?: boolean;
  children: React.ReactNode;
}) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <div>
      <button
        onClick={() => setOpen((o) => !o)}
        className="flex w-full items-center gap-1.5 px-2 text-left transition-colors"
        style={{
          height: 22,
          background: "rgba(255,255,255,0.016)",
          borderBottom: `1px solid rgba(58,69,84,${open ? "0.6" : "0.45"})`,
          borderTop: "1px solid rgba(58,69,84,0.35)",
        }}
      >
        <ChevronRight
          size={8}
          className="shrink-0 transition-transform"
          style={{
            color: open ? "rgba(95,206,208,0.55)" : "rgba(95,108,124,0.5)",
            transform: open ? "rotate(90deg)" : "none",
          }}
        />
        <Icon
          size={9}
          className="shrink-0"
          style={{ color: open ? "rgba(95,206,208,0.5)" : "rgba(95,108,124,0.45)" }}
        />
        <span
          className="flex-1 text-[8.5px] font-bold uppercase"
          style={{ color: "rgba(107,120,136,0.65)", letterSpacing: "0.1em" }}
        >
          {label}
        </span>
      </button>
      {open && <div>{children}</div>}
    </div>
  );
}

// ─── SectionDivider ───────────────────────────────────────────────────────────

function SectionDivider() {
  return <div className="border-t" style={{ borderColor: "rgba(58,69,84,0.4)" }} />;
}

// ─── EmptyRow ────────────────────────────────────────────────────────────────

function EmptyRow({ label }: { label: string }) {
  return (
    <div className="flex h-7 items-center justify-center px-4">
      <span className="text-[9px]" style={{ color: "rgba(107,120,136,0.45)" }}>
        {label}
      </span>
    </div>
  );
}

// ─── AssetRow — folder-project asset manifest entry ──────────────────────────

function AssetRow({ asset }: { asset: DawProjectAsset }) {
  const selectedBrowserFileId = useUIStore((s) => s.selectedBrowserFileId);
  const setSelectedBrowserFileId = useUIStore((s) => s.setSelectedBrowserFileId);
  const status = useProjectStore((s) => s.waveformStatus.get(asset.id));
  const selected = selectedBrowserFileId === asset.id;
  const missing = asset.missing || status === "missing";

  const statusLabel = missing
    ? "Missing"
    : status === "loading"
      ? "Loading…"
      : "Ready";

  const projectRoot = platform.folderProject.getProjectRoot();
  const absPath = projectRoot
    ? `${projectRoot}/${asset.relativePath}`.replace(/\\/g, "/")
    : null;

  const formatLabel = getFormatLabel(asset.name, asset.mimeType);
  const { color: badgeColor, background: badgeBg, borderColor: badgeBorder } =
    getFormatBadgeStyle(formatLabel);

  return (
    <div
      draggable={!missing}
      role="button"
      tabIndex={0}
      onClick={() => setSelectedBrowserFileId(selected ? null : asset.id)}
      onDragStart={(e) => {
        if (missing) return;
        e.dataTransfer.setData("application/x-mochi-file-id", asset.id);
        e.dataTransfer.effectAllowed = "copy";
        setSelectedBrowserFileId(asset.id);
      }}
      className="group flex w-full cursor-pointer items-center gap-1.5 border-b px-3 text-left transition-colors"
      style={{
        height: 26,
        borderColor: "rgba(58,69,84,0.38)",
        background: selected ? "rgba(95,206,208,0.065)" : "transparent",
        opacity: missing ? 0.72 : 1,
        boxShadow: selected ? "inset 2px 0 0 #5FCED0" : "none",
      }}
      onMouseEnter={(e) => {
        if (!selected) (e.currentTarget as HTMLElement).style.background = "rgba(255,255,255,0.022)";
      }}
      onMouseLeave={(e) => {
        if (!selected) (e.currentTarget as HTMLElement).style.background = "transparent";
      }}
    >
      {missing ? (
        <AlertTriangle size={10} className="shrink-0" style={{ color: "#F4877F" }} />
      ) : (
        <FileAudio2
          size={10}
          className="shrink-0"
          style={{ color: selected ? "#5FCED0" : "rgba(107,120,136,0.8)" }}
        />
      )}

      <span
        className="min-w-0 flex-1 truncate text-[11px]"
        style={{
          color: missing
            ? "rgba(244,135,127,0.9)"
            : selected
              ? "#a8d8d9"
              : "rgba(154,167,184,0.9)",
        }}
        title={asset.relativePath}
      >
        {asset.name}
      </span>

      {/* Format badge */}
      <span
        className="shrink-0 rounded px-1 py-px text-[8px] font-medium leading-none"
        style={{ color: badgeColor, background: badgeBg, border: `1px solid ${badgeBorder}` }}
      >
        {formatLabel}
      </span>

      {/* Status badge */}
      <span
        className="shrink-0 rounded px-1 py-px text-[8px] leading-none"
        style={{
          color: missing
            ? "rgba(244,135,127,0.85)"
            : "rgba(107,120,136,0.65)",
          background: "rgba(42,50,64,0.5)",
          border: "1px solid rgba(58,69,84,0.6)",
        }}
      >
        {statusLabel}
      </span>

      {/* Reveal */}
      {!missing && absPath && platform.capabilities.osFilePaths && (
        <button
          type="button"
          title="Reveal in file manager"
          className="hidden h-5 w-5 shrink-0 items-center justify-center rounded border text-daw-faint transition-colors hover:border-daw-accent hover:text-daw-text group-hover:flex"
          style={{ borderColor: "rgba(58,69,84,0.6)", background: "rgba(26,32,42,0.6)" }}
          onClick={async (e) => {
            e.preventDefault();
            e.stopPropagation();
            await platform.fileSystem.revealInFileManager(absPath).catch(console.warn);
          }}
        >
          <FolderSearch size={9} />
        </button>
      )}

      {/* Relink */}
      {missing && (
        <button
          type="button"
          title="Relink missing audio"
          className="flex h-5 w-5 shrink-0 items-center justify-center rounded border text-daw-faint transition-colors hover:border-daw-accent hover:text-daw-text"
          style={{ borderColor: "rgba(58,69,84,0.6)", background: "rgba(26,32,42,0.6)" }}
          onClick={async (e) => {
            e.preventDefault();
            e.stopPropagation();
            const files = await platform.fileSystem.pickAudioFiles();
            const picked = files[0];
            if (!picked) return;
            await audioAssetManager
              .relinkMissingAsset(asset.id, picked)
              .catch((error) => console.warn("[BrowserPanel] relink failed:", error));
          }}
        >
          <Link2 size={9} />
        </button>
      )}
    </div>
  );
}

// ─── FileRow — legacy IndexedDB / non-folder mode ────────────────────────────

function FileRow({ file }: { file: DawFile }) {
  const selectedBrowserFileId = useUIStore((s) => s.selectedBrowserFileId);
  const setSelectedBrowserFileId = useUIStore((s) => s.setSelectedBrowserFileId);
  const status = useProjectStore((s) => s.waveformStatus.get(file.id));
  const selected = selectedBrowserFileId === file.id;
  const missing = status === "missing" || file.storageProvider === "missing";

  const assetLabel =
    status === "missing"
      ? "Missing"
      : file.storageProvider === "indexeddb"
        ? "Cached"
        : "Ready";

  const formatLabel = getFormatLabel(file.name, file.mimeType);
  const { color: badgeColor, background: badgeBg, borderColor: badgeBorder } =
    getFormatBadgeStyle(formatLabel);

  return (
    <div
      draggable
      role="button"
      tabIndex={0}
      onClick={() => setSelectedBrowserFileId(selected ? null : file.id)}
      onDragStart={(e) => {
        e.dataTransfer.setData("application/x-mochi-file-id", file.id);
        e.dataTransfer.effectAllowed = "copy";
        setSelectedBrowserFileId(file.id);
      }}
      className="group flex w-full cursor-pointer items-center gap-1.5 border-b px-3 text-left transition-colors"
      style={{
        height: 26,
        borderColor: "rgba(58,69,84,0.38)",
        background: selected ? "rgba(95,206,208,0.065)" : "transparent",
        boxShadow: selected ? "inset 2px 0 0 #5FCED0" : "none",
      }}
      onMouseEnter={(e) => {
        if (!selected) (e.currentTarget as HTMLElement).style.background = "rgba(255,255,255,0.022)";
      }}
      onMouseLeave={(e) => {
        if (!selected) (e.currentTarget as HTMLElement).style.background = "transparent";
      }}
    >
      <FileAudio2
        size={10}
        className="shrink-0"
        style={{ color: selected ? "#5FCED0" : "rgba(107,120,136,0.8)" }}
      />
      <span
        className="min-w-0 flex-1 truncate text-[11px]"
        style={{ color: selected ? "#a8d8d9" : "rgba(154,167,184,0.9)" }}
      >
        {file.name}
      </span>

      <span
        className="shrink-0 rounded px-1 py-px text-[8px] font-medium leading-none"
        style={{ color: badgeColor, background: badgeBg, border: `1px solid ${badgeBorder}` }}
      >
        {formatLabel}
      </span>

      <span
        className="shrink-0 rounded px-1 py-px text-[8px] leading-none"
        style={{
          color: missing ? "rgba(244,135,127,0.85)" : "rgba(107,120,136,0.65)",
          background: "rgba(42,50,64,0.5)",
          border: "1px solid rgba(58,69,84,0.6)",
        }}
      >
        {assetLabel}
      </span>

      {missing && (
        <button
          type="button"
          title="Relink missing audio"
          className="flex h-5 w-5 shrink-0 items-center justify-center rounded border text-daw-faint transition-colors hover:border-daw-accent hover:text-daw-text"
          style={{ borderColor: "rgba(58,69,84,0.6)", background: "rgba(26,32,42,0.6)" }}
          onClick={async (e) => {
            e.preventDefault();
            e.stopPropagation();
            const files = await platform.fileSystem.pickAudioFiles();
            const picked = files[0];
            if (!picked) return;
            await audioAssetManager
              .relinkMissingAsset(file.id, picked)
              .catch((error) => console.warn("[BrowserPanel] relink failed:", error));
          }}
        >
          <Link2 size={9} />
        </button>
      )}
    </div>
  );
}

// ─── NativePlaceRow — root location button (kept for compat) ──────────────────

export function NativePlaceRow({
  root,
  selected,
  onOpen,
}: {
  root: BrowserRootEntry;
  selected: boolean;
  onOpen: (path: NativeBrowserPath) => void;
}) {
  const Icon = root.kind === "drive" ? HardDrive : root.kind === "factory" ? Library : FolderOpen;
  return (
    <button
      type="button"
      onClick={() => onOpen({ path: root.path, name: root.name })}
      className="flex w-full items-center gap-2 border-b px-3 text-left transition-colors"
      style={{
        height: 26,
        borderColor: "rgba(58,69,84,0.38)",
        background: selected ? "rgba(95,206,208,0.065)" : "transparent",
        boxShadow: selected ? "inset 2px 0 0 #5FCED0" : "none",
      }}
      onMouseEnter={(e) => {
        if (!selected) (e.currentTarget as HTMLElement).style.background = "rgba(255,255,255,0.022)";
      }}
      onMouseLeave={(e) => {
        if (!selected) (e.currentTarget as HTMLElement).style.background = "transparent";
      }}
      title={root.path}
    >
      <Icon
        size={11}
        className="shrink-0"
        style={{ color: selected ? "#5FCED0" : "rgba(107,120,136,0.65)" }}
      />
      <span
        className="min-w-0 flex-1 truncate text-[11px]"
        style={{ color: selected ? "#a8d8d9" : "rgba(154,167,184,0.85)" }}
      >
        {root.name}
      </span>
    </button>
  );
}

// ─── NativeFileRow — flat file row (kept for compat) ─────────────────────────

export function NativeFileRow({
  entry,
  activePreviewPath,
  onOpenFolder,
  onPreview,
  onImport,
  onReveal,
}: {
  entry: BrowserFileEntry;
  activePreviewPath: string | null;
  onOpenFolder: (entry: BrowserFileEntry) => void;
  onPreview: (entry: BrowserFileEntry) => void;
  onImport: (entry: BrowserFileEntry) => void;
  onReveal: (entry: BrowserFileEntry) => void;
}) {
  const isFolder = entry.kind === "folder";
  const playing = activePreviewPath === entry.path;

  return (
    <div
      role="button"
      tabIndex={0}
      onDoubleClick={() => (isFolder ? onOpenFolder(entry) : onImport(entry))}
      className="group flex w-full items-center gap-1.5 border-b px-3 text-left transition-colors hover:bg-white/[0.025]"
      style={{ height: 26, borderColor: "rgba(58,69,84,0.38)" }}
      title={entry.path}
    >
      <button
        type="button"
        title={isFolder ? "Open folder" : playing ? "Stop preview" : "Preview audio"}
        onClick={(e) => {
          e.preventDefault();
          e.stopPropagation();
          if (isFolder) onOpenFolder(entry);
          else onPreview(entry);
        }}
        className="flex h-5 w-5 shrink-0 items-center justify-center rounded transition-colors hover:bg-white/[0.06]"
        style={{ color: "rgba(107,120,136,0.7)" }}
      >
        {isFolder ? (
          <FolderOpen size={10} />
        ) : playing ? (
          <Pause size={10} />
        ) : (
          <Play size={10} />
        )}
      </button>

      <span
        className="min-w-0 flex-1 truncate text-[11px]"
        style={{ color: "rgba(154,167,184,0.85)" }}
      >
        {entry.name}
      </span>

      {!isFolder && (
        <span
          className="hidden shrink-0 text-[9px] tabular-nums group-hover:inline"
          style={{ color: "rgba(107,120,136,0.6)" }}
        >
          {formatBytes(entry.size)}
        </span>
      )}

      {!isFolder && <FormatBadge name={entry.name} mimeType={entry.mimeType} />}

      {!isFolder && (
        <button
          type="button"
          title="Import"
          onClick={(e) => {
            e.preventDefault();
            e.stopPropagation();
            onImport(entry);
          }}
          className="hidden h-5 w-5 shrink-0 items-center justify-center rounded border transition-colors hover:border-daw-accent hover:text-daw-text group-hover:flex"
          style={{ borderColor: "rgba(58,69,84,0.6)", background: "rgba(26,32,42,0.6)", color: "rgba(107,120,136,0.7)" }}
        >
          <Upload size={9} />
        </button>
      )}

      <button
        type="button"
        title="Reveal in file manager"
        onClick={(e) => {
          e.preventDefault();
          e.stopPropagation();
          onReveal(entry);
        }}
        className="hidden h-5 w-5 shrink-0 items-center justify-center rounded border transition-colors hover:border-daw-accent hover:text-daw-text group-hover:flex"
        style={{ borderColor: "rgba(58,69,84,0.6)", background: "rgba(26,32,42,0.6)", color: "rgba(107,120,136,0.7)" }}
      >
        <FolderSearch size={9} />
      </button>
    </div>
  );
}

// ─── NativeTreeNode ───────────────────────────────────────────────────────────

function NativeTreeNode({
  entry,
  depth,
  expandedPaths,
  loadingPaths,
  childrenByPath,
  activePreviewPath,
  indexStatuses,
  selectedPath,
  onToggle,
  onPreview,
  onImport,
  onReveal,
  onSelect,
}: {
  entry: NativeTreeEntry;
  depth: number;
  expandedPaths: Set<string>;
  loadingPaths: Set<string>;
  childrenByPath: Record<string, BrowserFileEntry[]>;
  activePreviewPath: string | null;
  indexStatuses: Record<string, BrowserIndexStatus>;
  selectedPath: string | null;
  onToggle: (entry: NativeTreeEntry) => void;
  onPreview: (entry: BrowserFileEntry) => void;
  onImport: (entry: BrowserFileEntry) => void;
  onReveal: (entry: NativeTreeEntry) => void;
  onSelect: (path: string) => void;
}) {
  const isFolder =
    entry.kind === "folder" ||
    entry.kind === "drive" ||
    entry.kind === "factory" ||
    entry.kind === "factory-folder";
  const isAudio = entry.kind === "audio";
  const expanded = expandedPaths.has(entry.path);
  const loading = loadingPaths.has(entry.path);
  const children = childrenByPath[entry.path];
  const playing = activePreviewPath === entry.path;
  const selected = selectedPath === entry.path;

  const EntryIcon = getEntryIcon(entry);

  const indexStatus = indexStatuses[entry.path];

  const audioEntry: BrowserFileEntry = {
    name: entry.name,
    path: entry.path,
    kind: "audio",
    size: entry.size,
    mimeType: entry.mimeType,
  };

  // Icon color by kind
  let iconColor = "rgba(107,120,136,0.72)";
  if (entry.kind === "drive") iconColor = "rgba(107,120,136,0.65)";
  if (entry.kind === "factory") iconColor = "rgba(95,206,208,0.7)";
  if (entry.kind === "factory-folder") iconColor = "rgba(95,206,208,0.55)";
  if (selected) iconColor = "#5FCED0";

  return (
    <>
      <div
        role="treeitem"
        draggable={isAudio}
        aria-expanded={isFolder ? expanded : undefined}
        tabIndex={0}
        onDragStart={(e) => {
          if (!isAudio) return;
          e.dataTransfer.setData(NATIVE_AUDIO_DRAG_TYPE, entry.path);
          e.dataTransfer.setData("text/plain", entry.path);
          e.dataTransfer.effectAllowed = "copy";
        }}
        onClick={() => {
          onSelect(entry.path);
          if (isFolder) onToggle(entry);
          else if (isAudio) onPreview(audioEntry);
        }}
        onDoubleClick={() => {
          if (isAudio) onImport(audioEntry);
        }}
        className="group relative flex w-full items-center gap-1 border-b text-left transition-colors"
        style={{
          height: 26,
          paddingLeft: (selected ? 8 : 6) + depth * 14,
          borderColor: "rgba(58,69,84,0.3)",
          background: selected
            ? "rgba(95,206,208,0.065)"
            : "transparent",
          cursor: isAudio ? "default" : "pointer",
          boxShadow: selected ? "inset 2px 0 0 #5FCED0" : "none",
        }}
        onMouseEnter={(e) => {
          if (!selected)
            (e.currentTarget as HTMLElement).style.background = "rgba(255,255,255,0.022)";
        }}
        onMouseLeave={(e) => {
          if (!selected)
            (e.currentTarget as HTMLElement).style.background = "transparent";
        }}
        title={entry.path}
      >
        {/* Expand/collapse chevron or play/pause for audio */}
        <button
          type="button"
          title={
            isFolder
              ? expanded
                ? "Collapse"
                : "Expand"
              : playing
                ? "Stop preview"
                : "Preview"
          }
          onClick={(e) => {
            e.stopPropagation();
            if (isFolder) {
              onSelect(entry.path);
              onToggle(entry);
            } else if (isAudio) {
              onPreview(audioEntry);
            }
          }}
          className="flex h-5 w-5 shrink-0 items-center justify-center rounded transition-colors hover:bg-white/[0.06]"
          style={{ color: "rgba(107,120,136,0.55)" }}
        >
          {isFolder ? (
            loading ? (
              <Loader2 size={9} className="animate-spin" style={{ color: "#5FCED0" }} />
            ) : (
              <ChevronRight
                size={9}
                className="transition-transform"
                style={{ transform: expanded ? "rotate(90deg)" : "none" }}
              />
            )
          ) : playing ? (
            <Pause size={9} />
          ) : (
            <Play size={9} style={{ color: "rgba(107,120,136,0.5)" }} />
          )}
        </button>

        {/* Entry icon */}
        <EntryIcon size={11} className="shrink-0" style={{ color: iconColor }} />

        {/* Name */}
        <span
          className="min-w-0 flex-1 truncate text-[11px]"
          style={{
            color: selected
              ? "#a8d8d9"
              : isAudio
                ? "rgba(154,167,184,0.88)"
                : "rgba(154,167,184,0.95)",
            paddingLeft: 3,
          }}
        >
          {entry.name}
        </span>

        {/* Index badge on folders */}
        {isFolder && <IndexBadge status={indexStatus} />}

        {/* Audio: size (hover only) + format badge */}
        {isAudio && (
          <>
            <span
              className="hidden shrink-0 text-[9px] tabular-nums group-hover:inline"
              style={{ color: "rgba(107,120,136,0.55)" }}
            >
              {formatBytes(entry.size)}
            </span>
            <FormatBadge name={entry.name} mimeType={entry.mimeType} />
          </>
        )}

        {/* Action buttons — appear on hover */}
        {isAudio && (
          <button
            type="button"
            title="Import to project"
            onClick={(e) => {
              e.preventDefault();
              e.stopPropagation();
              onImport(audioEntry);
            }}
            className="hidden h-5 w-5 shrink-0 items-center justify-center rounded border transition-colors hover:border-daw-accent hover:text-daw-text group-hover:flex"
            style={{
              borderColor: "rgba(58,69,84,0.6)",
              background: "rgba(26,32,42,0.6)",
              color: "rgba(107,120,136,0.7)",
            }}
          >
            <Upload size={9} />
          </button>
        )}

        <button
          type="button"
          title="Reveal in file manager"
          onClick={(e) => {
            e.preventDefault();
            e.stopPropagation();
            onReveal(entry);
          }}
          className="hidden h-5 w-5 shrink-0 items-center justify-center rounded border transition-colors hover:border-daw-accent hover:text-daw-text group-hover:flex"
          style={{
            borderColor: "rgba(58,69,84,0.6)",
            background: "rgba(26,32,42,0.6)",
            color: "rgba(107,120,136,0.7)",
          }}
        >
          <FolderSearch size={9} />
        </button>
      </div>

      {/* Children */}
      {isFolder && expanded && children?.map((child) => (
        <NativeTreeNode
          key={child.path}
          entry={child}
          depth={depth + 1}
          expandedPaths={expandedPaths}
          loadingPaths={loadingPaths}
          childrenByPath={childrenByPath}
          activePreviewPath={activePreviewPath}
          indexStatuses={indexStatuses}
          selectedPath={selectedPath}
          onToggle={onToggle}
          onPreview={onPreview}
          onImport={onImport}
          onReveal={onReveal}
          onSelect={onSelect}
        />
      ))}
    </>
  );
}

// ─── BrowserPanel ─────────────────────────────────────────────────────────────

export function BrowserPanel({
  onImport,
  width,
}: {
  onImport?: () => void;
  width?: number;
}) {
  const files = useProjectStore((s) => s.project.files);
  const assets = useProjectStore((s) => s.project.assets ?? EMPTY_ASSETS);
  const extraFolders = useSettingsStore((s) => s.extraFolders);

  const [query, setQuery] = useState("");
  const [activeTab, setActiveTab] = useState(() => platform.kind === "electron" ? "library" : "imports");
  const [nativeRoots, setNativeRoots] = useState<BrowserRootEntry[]>([]);
  const [nativePath, setNativePath] = useState<NativeBrowserPath | null>(null);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [expandedPaths, setExpandedPaths] = useState<Set<string>>(() => new Set());
  const [treeChildren, setTreeChildren] = useState<Record<string, BrowserFileEntry[]>>({});
  const [treeLoadingPaths, setTreeLoadingPaths] = useState<Set<string>>(() => new Set());
  const [indexStatuses, setIndexStatuses] = useState<Record<string, BrowserIndexStatus>>({});
  const [nativeError, setNativeError] = useState<string | null>(null);
  const [previewPath, setPreviewPath] = useState<string | null>(null);
  const previewAudioRef = useRef<HTMLAudioElement | null>(null);
  const previewUrlRef = useRef<string | null>(null);

  const isFolderProject = platform.kind === "electron" && platform.folderProject.isSupported;
  const isElectron = platform.kind === "electron";
  const hasAssets = isFolderProject && assets.length > 0;
  const projectRoot = isFolderProject ? platform.folderProject.getProjectRoot() : null;
  const extraFolderRoots = useMemo<BrowserRootEntry[]>(
    () =>
      extraFolders
        .filter((folder) => folder.enabled)
        .map((folder) => ({
          id: `extra:${folder.path}`,
          name: folder.name,
          path: folder.path,
          kind: "folder",
        })),
    [extraFolders],
  );

  // ── Preview helpers ──────────────────────────────────────────────────────────

  const stopPreview = () => {
    previewAudioRef.current?.pause();
    previewAudioRef.current = null;
    if (previewUrlRef.current) URL.revokeObjectURL(previewUrlRef.current);
    previewUrlRef.current = null;
    setPreviewPath(null);
  };

  // ── Bootstrap: load roots ────────────────────────────────────────────────────

  useEffect(() => {
    if (!isElectron) return;
    let cancelled = false;

    platform.fileSystem
      .ensureFactoryLibrary()
      .then(() => platform.fileSystem.browserRoots())
      .then((roots) => {
        if (cancelled) return;
        const byPath = new Map<string, BrowserRootEntry>();
        for (const root of [...roots, ...extraFolderRoots]) byPath.set(root.path, root);
        const nextRoots = [...byPath.values()];
        setNativeRoots(nextRoots);
        const preferred = nextRoots.find((r) => r.id === "factory") ?? nextRoots[0] ?? null;
        if (preferred) {
          setNativePath({ path: preferred.path, name: preferred.name });
          setExpandedPaths(new Set([preferred.path]));
          void loadTreeChildren(preferred.path);
          void platform.fileSystem
            .browserIndexStart(preferred.path)
            .then((status) =>
              setIndexStatuses((prev) => ({ ...prev, [status.rootPath]: status })),
            )
            .catch(console.warn);
        }
      })
      .catch((error: unknown) => {
        if (!cancelled)
          setNativeError(error instanceof Error ? error.message : String(error));
      });

    return () => {
      cancelled = true;
      stopPreview();
    };
  }, [isElectron, extraFolderRoots]);

  // ── Index status polling ─────────────────────────────────────────────────────

  useEffect(() => {
    if (!isElectron) return;
    const timer = window.setInterval(() => {
      const paths = Array.from(
        new Set([
          ...nativeRoots.map((r) => r.path),
          ...Array.from(expandedPaths),
        ]),
      );
      if (paths.length === 0) return;
      platform.fileSystem
        .browserIndexStatus(paths)
        .then((statuses) => {
          setIndexStatuses((prev) => {
            const next = { ...prev };
            for (const s of statuses) next[s.rootPath] = s;
            return next;
          });
        })
        .catch(console.warn);
    }, 800);
    return () => window.clearInterval(timer);
  }, [isElectron, nativeRoots, expandedPaths]);

  // ── Filtering ────────────────────────────────────────────────────────────────

  const filteredAssets = useMemo(() => {
    const q = query.trim().toLowerCase();
    return q ? assets.filter((a) => a.name.toLowerCase().includes(q)) : assets;
  }, [assets, query]);

  const legacyFiles = useMemo(() => {
    if (!isFolderProject) return files;
    const assetIds = new Set(assets.map((a) => a.id));
    return files.filter((f) => !assetIds.has(f.id));
  }, [files, assets, isFolderProject]);

  const filteredLegacyFiles = useMemo(() => {
    const q = query.trim().toLowerCase();
    return q ? legacyFiles.filter((f) => f.name.toLowerCase().includes(q)) : legacyFiles;
  }, [legacyFiles, query]);

  // ── Root buckets ─────────────────────────────────────────────────────────────

  const hasFactoryRoot = nativeRoots.some((r) => r.kind === "factory");
  const factoryRoots = nativeRoots.filter(
    (r) => r.kind === "factory" || (!hasFactoryRoot && r.kind === "factory-folder"),
  );
  const extraRoots = nativeRoots.filter((r) => r.id.startsWith("extra:"));
  const driveRoots = nativeRoots.filter((r) => r.kind === "drive");

  // Project section virtual entries (shown when folder project is open)
  const projectEntries: NativeTreeEntry[] = projectRoot
    ? [
        { name: "Media", path: `${projectRoot}/Media`, kind: "folder" },
        { name: "Rendered", path: `${projectRoot}/Rendered`, kind: "folder" },
        { name: "Cache", path: `${projectRoot}/Cache`, kind: "folder" },
      ]
    : [];

  // ── Tree helpers ─────────────────────────────────────────────────────────────

  async function loadTreeChildren(folderPath: string) {
    setTreeLoadingPaths((prev) => new Set(prev).add(folderPath));
    try {
      const entries = await platform.fileSystem.browserListDir(folderPath);
      setTreeChildren((prev) => ({ ...prev, [folderPath]: entries }));
      return entries;
    } catch (error) {
      console.warn("[BrowserPanel] tree list failed:", error);
      setTreeChildren((prev) => ({ ...prev, [folderPath]: [] }));
      return [];
    } finally {
      setTreeLoadingPaths((prev) => {
        const next = new Set(prev);
        next.delete(folderPath);
        return next;
      });
    }
  }

  const toggleTreeEntry = (entry: NativeTreeEntry) => {
    const isFolder =
      entry.kind === "folder" ||
      entry.kind === "drive" ||
      entry.kind === "factory" ||
      entry.kind === "factory-folder";
    if (!isFolder) return;

    setNativePath({ path: entry.path, name: entry.name });
    setExpandedPaths((prev) => {
      const next = new Set(prev);
      if (next.has(entry.path)) next.delete(entry.path);
      else next.add(entry.path);
      return next;
    });
    if (!treeChildren[entry.path]) void loadTreeChildren(entry.path);
    void platform.fileSystem
      .browserIndexStart(entry.path)
      .then((status) =>
        setIndexStatuses((prev) => ({ ...prev, [status.rootPath]: status })),
      )
      .catch(console.warn);
  };

  const importNativeEntry = async (entry: BrowserFileEntry) => {
    if (entry.kind !== "audio") return;
    await importNativeAudioPathToBrowser(entry.path);
  };

  const previewNativeEntry = async (entry: BrowserFileEntry) => {
    if (entry.kind !== "audio") return;
    if (previewPath === entry.path) {
      stopPreview();
      return;
    }
    stopPreview();
    const file = await platform.fileSystem.readAudioFile(entry.path);
    if (!file) return;
    const url = URL.createObjectURL(file);
    const audio = new Audio(url);
    previewAudioRef.current = audio;
    previewUrlRef.current = url;
    setPreviewPath(entry.path);
    audio.onended = stopPreview;
    audio.onerror = stopPreview;
    await audio.play().catch((error) => {
      console.warn("[BrowserPanel] preview failed:", error);
      stopPreview();
    });
  };

  const revealEntry = (entry: NativeTreeEntry) =>
    platform.fileSystem.revealInFileManager(entry.path).catch(console.warn);

  // ── Shared tree node props ───────────────────────────────────────────────────

  const treeProps = {
    expandedPaths,
    loadingPaths: treeLoadingPaths,
    childrenByPath: treeChildren,
    activePreviewPath: previewPath,
    indexStatuses,
    selectedPath,
    onToggle: toggleTreeEntry,
    onPreview: previewNativeEntry,
    onImport: importNativeEntry,
    onReveal: revealEntry,
    onSelect: setSelectedPath,
  };

  // ── Render ───────────────────────────────────────────────────────────────────

  const electronTabs = [
    { id: "library",  label: "Library",  Icon: Disc3 },
    { id: "project",  label: "Project",  Icon: FolderOpen },
    { id: "computer", label: "Computer", Icon: HardDrive },
  ] as const;

  const webTabs = [
    { id: "imports", label: "Files",    Icon: FileAudio2 },
    { id: "samples", label: "Samples",  Icon: FlaskConical },
    { id: "loops",   label: "Loops",    Icon: Layers },
  ] as const;

  const tabs = isElectron ? electronTabs : webTabs;

  return (
    <aside
      className="relative flex shrink-0 flex-col overflow-hidden border-r bg-daw-panel"
      style={{
        width: width ?? BROWSER_WIDTH,
        minWidth: width ?? BROWSER_WIDTH,
        borderColor: "rgba(58,69,84,0.7)",
      }}
      onDragOver={(e) => {
        if (![...e.dataTransfer.types].includes("Files")) return;
        e.preventDefault();
        e.dataTransfer.dropEffect = "copy";
      }}
      onDrop={async (e) => {
        if (![...e.dataTransfer.types].includes("Files")) return;
        e.preventDefault();
        const list = e.dataTransfer.files;
        if (!list?.length) return;
        for (const f of Array.from(list)) void decodeAndAddAudioFile(f);
      }}
    >
      {/* ── Header ─────────────────────────────────────────────────────────── */}
      <div
        className="flex h-8 shrink-0 items-center justify-between border-b px-3"
        style={{
          borderColor: "rgba(58,69,84,0.65)",
          background: "rgba(17,21,28,0.75)",
          boxShadow: "0 1px 0 rgba(0,0,0,0.18)",
        }}
      >
        <div className="flex items-center gap-2">
          <div className="h-3 w-[2px] rounded-full" style={{ background: "rgba(95,206,208,0.45)" }} />
          <span
            className="text-[9px] font-bold uppercase"
            style={{ color: "rgba(154,167,184,0.7)", letterSpacing: "0.13em" }}
          >
            Browser
          </span>
        </div>
        <div className="flex items-center gap-0.5">
          {previewPath && (
            <button
              type="button"
              title="Stop preview"
              onClick={stopPreview}
              className="flex h-5 w-5 items-center justify-center rounded transition-colors hover:bg-white/[0.06]"
              style={{ color: "#5FCED0" }}
            >
              <Pause size={10} />
            </button>
          )}
          {!isElectron && (
            <button
              onClick={onImport}
              title="Import audio [Ctrl+I]"
              className="flex h-5 w-5 items-center justify-center rounded transition-colors hover:bg-white/[0.06]"
              style={{ color: "rgba(107,120,136,0.7)" }}
            >
              <Upload size={10} />
            </button>
          )}
        </div>
      </div>

      {/* ── Tab bar ─────────────────────────────────────────────────────────── */}
      <div
        className="flex shrink-0 border-b"
        style={{ borderColor: "rgba(58,69,84,0.5)", background: "rgba(10,13,18,0.35)" }}
      >
        {tabs.map(({ id, label, Icon }) => {
          const active = activeTab === id;
          return (
            <button
              key={id}
              type="button"
              onClick={() => setActiveTab(id)}
              className="flex flex-1 flex-col items-center justify-center gap-[3px] transition-colors"
              style={{
                height: 32,
                color: active ? "#5FCED0" : "rgba(95,108,124,0.6)",
                background: active ? "rgba(95,206,208,0.05)" : "transparent",
                borderBottom: `1.5px solid ${active ? "#5FCED0" : "transparent"}`,
              }}
            >
              <Icon size={9} style={{ color: active ? "#5FCED0" : "rgba(95,108,124,0.5)" }} />
              <span
                className="text-[8px]"
                style={{ fontWeight: active ? 600 : 400, letterSpacing: "0.04em" }}
              >
                {label}
              </span>
            </button>
          );
        })}
      </div>

      {/* ── Search ─────────────────────────────────────────────────────────── */}
      <div
        className="shrink-0 border-b px-2"
        style={{
          borderColor: "rgba(58,69,84,0.5)",
          paddingTop: 5,
          paddingBottom: 5,
          background: "rgba(17,21,28,0.4)",
        }}
      >
        <label
          className="flex h-[22px] items-center gap-1.5 rounded px-2 transition-colors focus-within:ring-1"
          style={{
            background: "rgba(10,13,18,0.7)",
            border: "1px solid rgba(58,69,84,0.55)",
            outlineColor: "#5FCED0",
          }}
        >
          <Search size={8} style={{ color: "rgba(95,108,124,0.6)", flexShrink: 0 }} />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Filter…"
            className="min-w-0 flex-1 bg-transparent text-[10.5px] text-daw-text outline-none placeholder:text-[rgba(95,108,124,0.5)]"
            style={{ caretColor: "#5FCED0" }}
          />
          {query && (
            <button
              type="button"
              onClick={() => setQuery("")}
              className="flex shrink-0 items-center justify-center"
              style={{ color: "rgba(95,108,124,0.5)" }}
            >
              <X size={8} />
            </button>
          )}
        </label>
      </div>

      {/* ── Scroll area ────────────────────────────────────────────────────── */}
      <div className="min-h-0 flex-1 overflow-y-auto">

        {/* ── ELECTRON ─────────────────────────────────────────────────────── */}
        {isElectron && (
          <>
            {nativeError && (
              <div className="px-3 py-2">
                <p className="text-[10px]" style={{ color: "rgba(244,135,127,0.85)" }}>
                  {nativeError}
                </p>
              </div>
            )}

            {/* Library tab — factory roots + extra folders */}
            {activeTab === "library" && (
              <>
                {factoryRoots.length > 0 ? (
                  <Section label="Library" icon={Disc3}>
                    <div role="tree">
                      {factoryRoots.map((root) => (
                        <NativeTreeNode key={root.id} entry={root} depth={0} {...treeProps} />
                      ))}
                    </div>
                  </Section>
                ) : !nativeError && (
                  <EmptyRow label="Loading…" />
                )}

                {extraRoots.length > 0 && (
                  <>
                    <SectionDivider />
                    <Section label="Extra Folders" icon={FolderSearch}>
                      <div role="tree">
                        {extraRoots.map((root) => (
                          <NativeTreeNode key={root.id} entry={root} depth={0} {...treeProps} />
                        ))}
                      </div>
                    </Section>
                  </>
                )}
              </>
            )}

            {/* Project tab — project folders + imported assets */}
            {activeTab === "project" && (
              <>
                {projectRoot && projectEntries.length > 0 ? (
                  <Section label="Project Files" icon={FolderOpen}>
                    <div
                      className="flex items-center gap-1.5 border-b px-3 py-1"
                      style={{ borderColor: "rgba(58,69,84,0.35)" }}
                    >
                      <FolderOpen size={9} style={{ color: "rgba(95,206,208,0.5)" }} />
                      <span
                        className="min-w-0 flex-1 truncate text-[9px]"
                        style={{ color: "rgba(95,206,208,0.55)" }}
                        title={projectRoot}
                      >
                        {projectRoot.split(/[\\/]/).pop() ?? projectRoot}
                      </span>
                    </div>
                    <div role="tree">
                      {projectEntries.map((entry) => (
                        <NativeTreeNode key={entry.path} entry={entry} depth={0} {...treeProps} />
                      ))}
                    </div>
                  </Section>
                ) : (
                  <EmptyRow label="No project open" />
                )}

                {hasAssets && (
                  <>
                    <SectionDivider />
                    <Section label="Imports" icon={FileAudio2}>
                      {filteredAssets.length > 0 ? (
                        filteredAssets.map((a) => <AssetRow key={a.id} asset={a} />)
                      ) : (
                        <EmptyRow label={query ? `No results for "${query}"` : "No imported files"} />
                      )}
                    </Section>
                  </>
                )}
              </>
            )}

            {/* Computer tab — drive roots */}
            {activeTab === "computer" && (
              <>
                {driveRoots.length > 0 ? (
                  <div role="tree">
                    {driveRoots.map((root) => (
                      <NativeTreeNode key={root.id} entry={root} depth={0} {...treeProps} />
                    ))}
                  </div>
                ) : (
                  <EmptyRow label="No drives found" />
                )}
              </>
            )}

            {/* Status bar */}
            <div
              className="sticky bottom-0 flex h-5 items-center gap-2 border-t px-2"
              style={{
                borderColor: "rgba(58,69,84,0.4)",
                background: "rgba(10,13,18,0.88)",
              }}
            >
              <span
                className="min-w-0 flex-1 truncate text-[8.5px] tabular-nums"
                style={{ color: "rgba(95,108,124,0.45)" }}
                title={nativePath?.path}
              >
                {nativePath?.path ?? ""}
              </span>
            </div>
          </>
        )}

        {/* ── WEB ──────────────────────────────────────────────────────────── */}
        {!isElectron && (
          <>
            {/* Files/Imports tab */}
            {activeTab === "imports" && (
              <Section label="Imports" icon={FileAudio2} defaultOpen>
                {hasAssets ? (
                  filteredAssets.length > 0 ? (
                    filteredAssets.map((a) => <AssetRow key={a.id} asset={a} />)
                  ) : assets.length === 0 ? (
                    <EmptyImportsPlaceholder onImport={onImport} />
                  ) : (
                    <EmptyRow label={`No results for "${query}"`} />
                  )
                ) : filteredLegacyFiles.length > 0 ? (
                  filteredLegacyFiles.map((f) => <FileRow key={f.id} file={f} />)
                ) : files.length === 0 ? (
                  <EmptyImportsPlaceholder onImport={onImport} />
                ) : (
                  <EmptyRow label={`No results for "${query}"`} />
                )}

                {hasAssets && filteredLegacyFiles.length > 0 && (
                  <>
                    <div
                      className="flex h-6 items-center gap-2 px-3"
                      style={{ borderBottom: "1px solid rgba(58,69,84,0.4)" }}
                    >
                      <div className="h-px flex-1" style={{ background: "rgba(58,69,84,0.5)" }} />
                      <span className="text-[9px] font-medium" style={{ color: "rgba(107,120,136,0.55)" }}>
                        Session files
                      </span>
                      <div className="h-px flex-1" style={{ background: "rgba(58,69,84,0.5)" }} />
                    </div>
                    {filteredLegacyFiles.map((f) => <FileRow key={f.id} file={f} />)}
                  </>
                )}
              </Section>
            )}

            {/* Samples tab */}
            {activeTab === "samples" && (
              <Section label="Samples" icon={FlaskConical} defaultOpen>
                <div
                  className="mx-2 my-2 flex items-center justify-center rounded px-3 py-3"
                  style={{ border: "1px dashed rgba(58,69,84,0.45)", background: "rgba(255,255,255,0.008)" }}
                >
                  <span className="text-[9px]" style={{ color: "rgba(95,108,124,0.45)" }}>
                    Sample library — coming soon
                  </span>
                </div>
                {["Kick — Hard 01.wav", "Snare — Crackle.wav", "Hi-Hat — Open 16th.wav", "Bass — 808 Sub C.wav"].map(
                  (name) => (
                    <div key={name} className="flex items-center gap-1.5 border-b px-3 opacity-20"
                      style={{ height: 26, borderColor: "rgba(58,69,84,0.3)" }}>
                      <FileAudio2 size={9} style={{ color: "rgba(107,120,136,0.7)" }} />
                      <span className="min-w-0 flex-1 truncate text-[10.5px]" style={{ color: "rgba(154,167,184,0.8)" }}>{name}</span>
                      <FormatBadge name={name} />
                    </div>
                  ),
                )}
              </Section>
            )}

            {/* Loops tab */}
            {activeTab === "loops" && (
              <Section label="Loops" icon={Layers} defaultOpen>
                <div
                  className="mx-2 my-2 flex items-center justify-center rounded px-3 py-3"
                  style={{ border: "1px dashed rgba(58,69,84,0.45)", background: "rgba(255,255,255,0.008)" }}
                >
                  <span className="text-[9px]" style={{ color: "rgba(95,108,124,0.45)" }}>
                    Loop library — coming soon
                  </span>
                </div>
                {["Drum Loop — 120bpm.wav", "Guitar Riff — Am.wav", "Bass Loop — Funk.wav"].map(
                  (name) => (
                    <div key={name} className="flex items-center gap-1.5 border-b px-3 opacity-20"
                      style={{ height: 26, borderColor: "rgba(58,69,84,0.3)" }}>
                      <FileAudio2 size={9} style={{ color: "rgba(107,120,136,0.7)" }} />
                      <span className="min-w-0 flex-1 truncate text-[10.5px]" style={{ color: "rgba(154,167,184,0.8)" }}>{name}</span>
                      <FormatBadge name={name} />
                    </div>
                  ),
                )}
              </Section>
            )}
          </>
        )}
      </div>
    </aside>
  );
}

// ─── EmptyImportsPlaceholder ──────────────────────────────────────────────────

function EmptyImportsPlaceholder({ onImport }: { onImport?: () => void }) {
  return (
    <div className="mx-2 my-2 flex flex-col items-center gap-2 rounded px-3 py-5 text-center"
      style={{ border: "1px dashed rgba(58,69,84,0.55)", background: "rgba(255,255,255,0.012)" }}>
      <FileAudio2 size={14} style={{ color: "rgba(95,108,124,0.4)" }} />
      <p className="text-[9.5px] leading-relaxed" style={{ color: "rgba(107,120,136,0.5)" }}>
        No audio imported
      </p>
      {onImport && (
        <button
          onClick={onImport}
          className="flex h-6 items-center gap-1.5 rounded px-2.5 text-[9px] font-semibold transition-colors hover:bg-white/[0.06]"
          style={{ border: "1px solid rgba(95,206,208,0.35)", color: "rgba(95,206,208,0.75)", background: "rgba(95,206,208,0.06)" }}
        >
          <Upload size={8} />
          Import Audio
        </button>
      )}
    </div>
  );
}
