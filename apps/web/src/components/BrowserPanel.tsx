import { Database, FileAudio2, Search, Upload } from "lucide-react";
import { useMemo, useState } from "react";
import { useProjectStore } from "../store/projectStore";
import { BROWSER_WIDTH } from "../theme";
import type { DawFile } from "../types/daw";

function formatDuration(seconds: number) {
  if (!Number.isFinite(seconds) || seconds <= 0) return "--:--";
  const minutes = Math.floor(seconds / 60);
  const rest = Math.floor(seconds % 60);
  return `${minutes}:${String(rest).padStart(2, "0")}`;
}

function fileBadge(file: DawFile) {
  if (file.mimeType.includes("mpeg") || file.name.toLowerCase().endsWith(".mp3")) return "MP3";
  if (file.mimeType.includes("wav") || file.name.toLowerCase().endsWith(".wav")) return "WAV";
  return "Audio";
}

export function BrowserPanel({ onImport }: { onImport?: () => void }) {
  const files = useProjectStore((s) => s.project.files);
  const [query, setQuery] = useState("");
  const visibleFiles = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    if (!normalized) return files;
    return files.filter((file) => file.name.toLowerCase().includes(normalized));
  }, [files, query]);

  return (
    <aside
      className="flex shrink-0 flex-col overflow-hidden border-r border-daw-border bg-daw-surface"
      style={{ width: BROWSER_WIDTH, minWidth: BROWSER_WIDTH }}
    >
      <div className="border-b border-daw-border bg-daw-sunken px-2.5 py-2.5">
        <div className="mb-2.5 flex items-center justify-between gap-2">
          <div className="flex min-w-0 items-center gap-2">
            <div className="flex h-7 w-7 items-center justify-center rounded bg-daw-surface-high text-daw-accent">
              <Database size={15} />
            </div>
            <div className="min-w-0">
              <div className="truncate text-[13px] font-semibold text-daw-text">Browser</div>
              <div className="text-[11px] text-daw-faint">{files.length} project assets</div>
            </div>
          </div>
          <button
            onClick={onImport}
            title="Import audio"
            className="flex h-7 w-7 items-center justify-center rounded border border-daw-border bg-daw-surface-high text-daw-dim transition-colors hover:border-daw-border-light hover:text-daw-text"
          >
            <Upload size={14} />
          </button>
        </div>

        <label className="flex h-7 items-center gap-2 rounded border border-daw-border bg-daw-bg px-2 text-daw-faint focus-within:border-daw-accent">
          <Search size={13} />
          <input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search audio"
            className="min-w-0 flex-1 bg-transparent text-[12px] text-daw-text outline-none placeholder:text-daw-faint"
          />
        </label>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto p-2.5">
        {visibleFiles.length === 0 ? (
          <div className="flex h-full min-h-52 flex-col items-center justify-center rounded border border-dashed border-daw-border px-4 py-6 text-center">
            <FileAudio2 size={28} className="mb-3 text-daw-faint" />
            <div className="text-[12px] font-medium text-daw-dim">
              {files.length === 0 ? "No audio imported" : "No matching audio"}
            </div>
            <div className="mt-1 max-w-40 text-[11px] leading-5 text-daw-faint">
              {files.length === 0 ? "Import WAV or MP3 files to build the arrangement." : "Adjust the search term."}
            </div>
            {files.length === 0 && (
              <button
                onClick={onImport}
                className="mt-3 h-7 rounded bg-daw-accent px-3 text-[12px] font-semibold text-daw-ink transition-colors hover:bg-daw-accent-h"
              >
                Import Audio
              </button>
            )}
          </div>
        ) : (
          <div className="flex flex-col gap-1">
            {visibleFiles.map((file) => (
              <button
                key={file.id}
                className="group flex w-full items-center gap-2 rounded border border-transparent bg-transparent px-2 py-2 text-left transition-colors hover:border-daw-border hover:bg-daw-surface-high"
              >
                <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded bg-daw-bg text-daw-cyan group-hover:text-daw-text">
                  <FileAudio2 size={15} />
                </div>
                <div className="min-w-0 flex-1">
                  <div className="truncate text-[12px] font-medium text-daw-text">{file.name}</div>
                  <div className="mt-0.5 flex items-center gap-2 text-[10px] text-daw-faint">
                    <span>{formatDuration(file.duration)}</span>
                    <span>{file.channels}ch</span>
                    <span>{Math.round(file.sampleRate / 1000)}kHz</span>
                  </div>
                </div>
                <span className="rounded border border-daw-border bg-daw-bg px-1.5 py-0.5 text-[10px] text-daw-faint">
                  {fileBadge(file)}
                </span>
              </button>
            ))}
          </div>
        )}
      </div>
    </aside>
  );
}
