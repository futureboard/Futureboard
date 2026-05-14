import { ChevronRight, FileAudio2, FlaskConical, FolderOpen, Layers, Search, Upload } from "lucide-react";
import { useMemo, useState } from "react";
import { useProjectStore } from "../store/projectStore";
import { useUIStore } from "../store/uiStore";
import { BROWSER_WIDTH } from "../theme";
import type { DawFile } from "../types/daw";
import { decodeAndAddAudioFile } from "../utils/importAudioToProject";

function fileBadge(file: DawFile) {
  if (file.mimeType.includes("mpeg") || file.name.toLowerCase().endsWith(".mp3")) return "MP3";
  if (file.mimeType.includes("wav") || file.name.toLowerCase().endsWith(".wav")) return "WAV";
  return "Audio";
}

// ─── Collapsible section wrapper ──────────────────────────────────────────────

function Section({
  label,
  icon: Icon,
  count,
  defaultOpen = true,
  children,
}: {
  label: string;
  icon: React.ElementType;
  count?: number;
  defaultOpen?: boolean;
  children: React.ReactNode;
}) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <div className="border-b border-daw-border last:border-b-0">
      <button
        onClick={() => setOpen((o) => !o)}
        className="flex w-full items-center gap-1.5 px-2 py-1.5 text-left transition-colors hover:bg-white/[0.03]"
      >
        <ChevronRight
          size={10}
          className="shrink-0 text-daw-faint transition-transform"
          style={{ transform: open ? "rotate(90deg)" : "none" }}
        />
        <Icon size={10} className="shrink-0 text-daw-faint" />
        <span className="flex-1 text-[10px] font-semibold uppercase tracking-widest text-daw-faint">
          {label}
        </span>
        {count !== undefined && (
          <span className="text-[9px] tabular-nums text-daw-faint opacity-50">{count}</span>
        )}
      </button>
      {open && <div>{children}</div>}
    </div>
  );
}

// ─── Placeholder row (coming soon) ────────────────────────────────────────────

function ComingSoonRow({ label }: { label: string }) {
  return (
    <div className="flex h-8 items-center gap-2 px-6 text-daw-faint">
      <div className="h-px flex-1 bg-daw-border" />
      <span className="text-[9px] font-medium">{label}</span>
      <div className="h-px flex-1 bg-daw-border" />
    </div>
  );
}

// ─── File row ─────────────────────────────────────────────────────────────────

function FileRow({ file }: { file: DawFile }) {
  const selectedBrowserFileId = useUIStore((s) => s.selectedBrowserFileId);
  const setSelectedBrowserFileId = useUIStore((s) => s.setSelectedBrowserFileId);
  const selected = selectedBrowserFileId === file.id;

  return (
    <button
      draggable
      onClick={() => setSelectedBrowserFileId(selected ? null : file.id)}
      onDragStart={(e) => {
        e.dataTransfer.setData("application/x-mochi-file-id", file.id);
        e.dataTransfer.effectAllowed = "copy";
        setSelectedBrowserFileId(file.id);
      }}
      className="group flex w-full items-center gap-2 border-b border-daw-border px-3 py-1.5 text-left transition-colors hover:bg-white/[0.035] cursor-pointer"
      style={{
        background: selected ? "rgba(86,199,201,0.08)" : "transparent",
      }}
    >
      <FileAudio2
        size={11}
        className="shrink-0"
        style={{ color: selected ? "#56c7c9" : undefined }}
      />
      <span
        className="min-w-0 flex-1 truncate text-[11px]"
        style={{ color: selected ? "#a8d8d9" : undefined }}
      >
        {file.name}
      </span>
      <span className="shrink-0 rounded border border-daw-border bg-daw-bg px-1 py-0.5 text-[8px] text-daw-faint">
        {fileBadge(file)}
      </span>
    </button>
  );
}

// ─── Browser Panel ────────────────────────────────────────────────────────────

export function BrowserPanel({ onImport, width }: { onImport?: () => void; width?: number }) {
  const files = useProjectStore((s) => s.project.files);
  const [query, setQuery] = useState("");

  const filteredFiles = useMemo(() => {
    const q = query.trim().toLowerCase();
    return q ? files.filter((f) => f.name.toLowerCase().includes(q)) : files;
  }, [files, query]);

  return (
    <aside
      className="flex shrink-0 flex-col overflow-hidden border-r border-daw-border bg-daw-panel relative"
      style={{ width: width ?? BROWSER_WIDTH, minWidth: width ?? BROWSER_WIDTH }}
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
        for (const f of Array.from(list)) {
          await decodeAndAddAudioFile(f);
        }
      }}
    >
      {/* header */}
      <div className="flex h-6 shrink-0 items-center justify-between border-b border-daw-border bg-daw-surface px-3">
        <span className="text-[10px] font-semibold uppercase tracking-widest text-daw-faint">
          Browser
        </span>
        <button
          onClick={onImport}
          title="Import audio [Ctrl+I]"
          className="flex h-5 w-5 items-center justify-center rounded text-daw-faint transition-colors hover:bg-white/[0.06] hover:text-daw-text"
        >
          <Upload size={11} />
        </button>
      </div>

      {/* search */}
      <div className="border-b border-daw-border px-2 py-1.5">
        <label className="flex h-6 items-center gap-2 rounded border border-daw-border bg-daw-bg px-2 text-daw-faint transition-colors focus-within:border-daw-accent">
          <Search size={10} />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search…"
            className="min-w-0 flex-1 bg-transparent text-[11px] text-daw-text outline-none placeholder:text-daw-faint"
          />
        </label>
      </div>

      {/* sections */}
      <div className="min-h-0 flex-1 overflow-y-auto">

        {/* IMPORTS */}
        <Section label="Imports" icon={FolderOpen} count={files.length}>
          {filteredFiles.length > 0 ? (
            filteredFiles.map((f) => <FileRow key={f.id} file={f} />)
          ) : files.length === 0 ? (
            <div className="flex flex-col items-center gap-2 px-4 py-5 text-center">
              <FileAudio2 size={20} className="text-daw-faint opacity-30" />
              <p className="text-[10px] leading-relaxed text-daw-faint">
                No audio imported yet
              </p>
              <button
                onClick={onImport}
                className="mt-1 h-7 rounded-md bg-daw-accent px-3 text-[10px] font-semibold text-daw-ink transition-colors hover:bg-daw-accent-h"
              >
                Import Audio
              </button>
            </div>
          ) : (
            <p className="px-4 py-2 text-[10px] text-daw-faint">No results for "{query}"</p>
          )}
        </Section>

        {/* SAMPLES */}
        <Section label="Samples" icon={FlaskConical} defaultOpen={false}>
          <ComingSoonRow label="Sample library — coming soon" />
          {[
            "Kick — Hard 01.wav",
            "Snare — Crackle.wav",
            "Hi-Hat — Open 16th.wav",
            "Bass — 808 Sub C.wav",
          ].map((name) => (
            <button
              key={name}
              disabled
              className="flex w-full cursor-default items-center gap-2 border-b border-daw-border px-3 py-1.5 opacity-35"
            >
              <FileAudio2 size={11} className="shrink-0 text-daw-faint" />
              <span className="min-w-0 flex-1 truncate text-[11px] text-daw-dim">{name}</span>
              <span className="shrink-0 rounded border border-daw-border bg-daw-bg px-1 py-0.5 text-[8px] text-daw-faint">
                WAV
              </span>
            </button>
          ))}
        </Section>

        {/* LOOPS */}
        <Section label="Loops" icon={Layers} defaultOpen={false}>
          <ComingSoonRow label="Loop library — coming soon" />
          {[
            "Drum Loop — 120bpm.wav",
            "Guitar Riff — Am.wav",
            "Bass Loop — Funk.wav",
          ].map((name) => (
            <button
              key={name}
              disabled
              className="flex w-full cursor-default items-center gap-2 border-b border-daw-border px-3 py-1.5 opacity-35"
            >
              <FileAudio2 size={11} className="shrink-0 text-daw-faint" />
              <span className="min-w-0 flex-1 truncate text-[11px] text-daw-dim">{name}</span>
              <span className="shrink-0 rounded border border-daw-border bg-daw-bg px-1 py-0.5 text-[8px] text-daw-faint">
                WAV
              </span>
            </button>
          ))}
        </Section>

      </div>
    </aside>
  );
}
