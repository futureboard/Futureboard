import {
  CheckCircle2,
  ChevronDown,
  ChevronUp,
  ChevronsUpDown,
  Folder,
  Info,
  Loader2,
  Music2,
  Plus,
  RefreshCw,
  Search,
  ShieldAlert,
  SlidersHorizontal,
  X,
} from "lucide-react";
import { useEffect, useMemo, useState, type ReactNode } from "react";
import { platform } from "../../platform";
import type { AudioPluginRegistryEntry } from "../../platform/platform.types";
import { useWindowStore } from "../../store/windowStore";

type Props = {
  windowId: string;
  external?: boolean;
};

type PluginFormat = "VST3" | "CLAP" | "AU";
type SortKey = "name" | "vendor" | "category" | "format";
type SortDir = "asc" | "desc";
type SidebarFilter =
  | { type: "all" }
  | { type: "instrument" }
  | { type: "effect" }
  | { type: "format"; value: PluginFormat };

function pluginDisplayCategory(plugin: AudioPluginRegistryEntry): string {
  return plugin.subCategories?.trim() || plugin.category || plugin.rawCategory || "Uncategorized";
}

const FORMAT_BADGE: Record<string, string> = {
  VST3: "text-blue-300/85 bg-blue-300/[0.09] border-blue-300/20",
  CLAP: "text-emerald-300/85 bg-emerald-300/[0.09] border-emerald-300/20",
  AU: "text-amber-300/60 bg-amber-300/[0.06] border-amber-300/15",
};

export function AudioPluginManager({ windowId, external = false }: Props) {
  const isElectron = platform.kind === "electron";
  const [plugins, setPlugins] = useState<AudioPluginRegistryEntry[]>([]);
  const [scanPaths, setScanPaths] = useState<string[]>([]);
  const [statusText, setStatusText] = useState("Loading plug-in registry...");
  const [query, setQuery] = useState("");
  const [scanning, setScanning] = useState(false);
  const [failedCount, setFailedCount] = useState(0);
  const [generatedPresets, setGeneratedPresets] = useState(0);
  const [sidebarFilter, setSidebarFilter] = useState<SidebarFilter>({ type: "all" });
  const [sortKey, setSortKey] = useState<SortKey>("name");
  const [sortDir, setSortDir] = useState<SortDir>("asc");
  const [selectedId, setSelectedId] = useState<string | null>(null);

  const close = () => {
    if (external && platform.kind === "electron") platform.window.close();
    else useWindowStore.getState().closeWindow(windowId);
  };
  void close;

  const scan = async () => {
    if (!platform.pluginHost.isSupported || scanning) return;
    setScanning(true);
    setStatusText("Scanning VST3/CLAP folders and generating .pst presets...");
    setPlugins([]);
    setFailedCount(0);
    setGeneratedPresets(0);
    try {
      const result = await platform.pluginHost.scanVst3();
      setPlugins(result.plugins);
      setScanPaths(result.status.defaultScanPaths);
      setFailedCount(result.failed.length);
      setGeneratedPresets(result.generatedPresets);
      setStatusText(`Generated ${result.generatedPresets} preset files. Cached ${result.plugins.length} plug-ins.`);
    } catch (error) {
      setStatusText(`Scan failed: ${String(error)}`);
    } finally {
      setScanning(false);
    }
  };

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const [status, cached] = await Promise.all([
        platform.pluginHost.getStatus(),
        platform.pluginHost.listPlugins(),
      ]);
      if (cancelled) return;
      setScanPaths(status.defaultScanPaths);
      setPlugins(cached);
      setStatusText(status.available ? status.message : "PluginHost native backend is unavailable.");
    })();
    return () => { cancelled = true; };
  }, []);

  useEffect(() => {
    if (!platform.pluginHost.isSupported) return;
    return platform.pluginHost.onScanProgress((event) => {
      if (event.type === "started") {
        setScanning(true);
        setFailedCount(0);
        setGeneratedPresets(0);
        setScanPaths(event.scannedPaths);
        setStatusText("Scanning VST3/CLAP folders...");
        return;
      }
      if (event.type === "folder") {
        setStatusText(`Searching ${event.path} • ${event.discovered} plug-ins found`);
        return;
      }
      if (event.type === "plugin") {
        setGeneratedPresets(event.generatedPresets);
        setPlugins((current) => {
          const next = current.filter((p) => p.id !== event.plugin.id);
          next.push(event.plugin);
          return next.sort((a, b) => {
            const kind = a.kind.localeCompare(b.kind);
            if (kind !== 0) return kind;
            const vendor = a.vendor.localeCompare(b.vendor);
            return vendor !== 0 ? vendor : a.name.localeCompare(b.name);
          });
        });
        setStatusText(`Cached ${event.plugin.name}`);
        return;
      }
      if (event.type === "failed") {
        setFailedCount((count) => count + 1);
        setStatusText(`Skipped ${event.path}: ${event.error}`);
        return;
      }
      if (event.type === "complete") {
        setPlugins(event.result.plugins);
        setScanPaths(event.result.status.defaultScanPaths);
        setGeneratedPresets(event.result.generatedPresets);
        setFailedCount(event.result.failed.length);
        setScanning(false);
        setStatusText(`Generated ${event.result.generatedPresets} preset files. Cached ${event.result.plugins.length} plug-ins.`);
      }
    });
  }, []);

  const counts = useMemo(
    () => ({
      all: plugins.length,
      instruments: plugins.filter((p) => p.kind === "instrument").length,
      effects: plugins.filter((p) => p.kind !== "instrument").length,
      vst3: plugins.filter((p) => p.format === "VST3").length,
      clap: plugins.filter((p) => p.format === "CLAP").length,
      au: plugins.filter((p) => p.format === "AU").length,
    }),
    [plugins],
  );

  const visiblePlugins = useMemo(() => {
    let result = [...plugins];

    if (sidebarFilter.type === "instrument") {
      result = result.filter((p) => p.kind === "instrument");
    } else if (sidebarFilter.type === "effect") {
      result = result.filter((p) => p.kind !== "instrument");
    } else if (sidebarFilter.type === "format") {
      result = result.filter((p) => p.format === sidebarFilter.value);
    }

    const q = query.trim().toLowerCase();
    if (q) {
      result = result.filter((p) =>
        `${p.name} ${p.vendor} ${pluginDisplayCategory(p)} ${p.rawCategory ?? ""} ${p.path}`.toLowerCase().includes(q),
      );
    }

    result.sort((a, b) => {
      let cmp = 0;
      if (sortKey === "name") cmp = a.name.localeCompare(b.name);
      else if (sortKey === "vendor") cmp = a.vendor.localeCompare(b.vendor);
      else if (sortKey === "category") cmp = pluginDisplayCategory(a).localeCompare(pluginDisplayCategory(b));
      else if (sortKey === "format") cmp = (a.format ?? "").localeCompare(b.format ?? "");
      return sortDir === "asc" ? cmp : -cmp;
    });

    return result;
  }, [plugins, query, sidebarFilter, sortKey, sortDir]);

  function handleSort(key: SortKey) {
    if (sortKey === key) setSortDir((d) => (d === "asc" ? "desc" : "asc"));
    else { setSortKey(key); setSortDir("asc"); }
  }

  function filterActive(f: SidebarFilter) {
    return JSON.stringify(sidebarFilter) === JSON.stringify(f);
  }

  const COLS = "grid-cols-[minmax(180px,2fr)_minmax(110px,1fr)_minmax(100px,1fr)_86px_94px]";

  return (
    <div className={`flex h-full min-h-0 flex-col bg-[#0e1319] text-daw-text ${external ? "" : "rounded-b-xl"}`}>
      {!isElectron && (
        <div className="mx-3 mt-3 flex shrink-0 items-center gap-2 rounded-md border border-amber-300/20 bg-amber-300/[0.08] px-3 py-2 text-[11px] text-amber-100/80">
          <ShieldAlert size={14} className="shrink-0" />
          Audio plug-in management is available only in the Desktop Version.
        </div>
      )}

      {/* Toolbar */}
      <div className="flex h-10 shrink-0 items-center gap-2 border-b border-white/[0.06] px-3">
        <div className="flex h-7 min-w-0 flex-1 items-center gap-1.5 rounded-md border border-white/[0.08] bg-[#0b1016] px-2">
          <Search size={12} className="shrink-0 text-daw-faint" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search plug-ins..."
            className="min-w-0 flex-1 bg-transparent text-[11px] text-daw-text outline-none placeholder:text-daw-faint"
          />
          {query && (
            <button type="button" onClick={() => setQuery("")} className="shrink-0 text-daw-faint hover:text-daw-dim">
              <X size={11} />
            </button>
          )}
        </div>
        <span className="shrink-0 tabular-nums text-[11px] text-daw-faint">
          {visiblePlugins.length} plug-in{visiblePlugins.length !== 1 ? "s" : ""}
        </span>
        <button
          type="button"
          disabled={!isElectron || scanning}
          onClick={scan}
          className="flex h-7 items-center gap-1.5 rounded-md border border-daw-accent/25 bg-daw-accent/10 px-3 text-[11px] font-semibold text-daw-accent hover:bg-daw-accent/15 disabled:opacity-50"
          title="Scan VST3/CLAP folders and generate .pst presets"
        >
          {scanning ? <Loader2 size={12} className="animate-spin" /> : <RefreshCw size={12} />}
          {scanning ? "Scanning…" : "Rescan"}
        </button>
      </div>

      {/* Content */}
      <div className="grid min-h-0 flex-1 grid-cols-[196px_minmax(0,1fr)] overflow-hidden">
        {/* Sidebar */}
        <aside className="flex min-h-0 flex-col overflow-y-auto border-r border-white/[0.06] bg-[#0b1016]">
          <div className="flex-1 py-1">
            <SidebarSection label="Library">
              <SidebarItem
                label="All Plug-ins"
                count={counts.all}
                active={filterActive({ type: "all" })}
                onClick={() => setSidebarFilter({ type: "all" })}
              />
            </SidebarSection>

            <SidebarSection label="Kind">
              <SidebarItem
                label="Instruments"
                count={counts.instruments}
                active={filterActive({ type: "instrument" })}
                onClick={() => setSidebarFilter({ type: "instrument" })}
              />
              <SidebarItem
                label="Effects"
                count={counts.effects}
                active={filterActive({ type: "effect" })}
                onClick={() => setSidebarFilter({ type: "effect" })}
              />
            </SidebarSection>

            <SidebarSection label="Format">
              <SidebarItem
                label="VST3"
                count={counts.vst3}
                active={filterActive({ type: "format", value: "VST3" })}
                onClick={() => setSidebarFilter({ type: "format", value: "VST3" })}
              />
              <SidebarItem
                label="CLAP"
                count={counts.clap}
                active={filterActive({ type: "format", value: "CLAP" })}
                onClick={() => setSidebarFilter({ type: "format", value: "CLAP" })}
              />
              <SidebarItem label="AU" count={counts.au} disabled />
            </SidebarSection>
          </div>

          {/* Scan locations pinned at bottom */}
          <div className="border-t border-white/[0.06] py-1">
            <SidebarSection label="Scan Locations">
              {scanPaths.length === 0 ? (
                <div className="px-2.5 py-0.5 text-[10px] text-daw-faint/50">
                  No scan paths detected
                </div>
              ) : (
                scanPaths.map((path) => (
                  <div
                    key={path}
                    title={path}
                    className="flex items-center gap-1.5 px-2.5 py-[5px] text-[10px] text-daw-faint"
                  >
                    <Folder size={11} className="shrink-0 opacity-60" />
                    <span className="min-w-0 truncate">{path}</span>
                  </div>
                ))
              )}
              <div className="px-2 pt-1">
                <button
                  type="button"
                  disabled
                  className="flex h-7 w-full items-center justify-center gap-1.5 rounded-md border border-white/[0.06] bg-white/[0.02] text-[10px] text-daw-faint opacity-45"
                  title="Backend not implemented"
                >
                  <Plus size={11} />
                  Add Location
                </button>
              </div>
            </SidebarSection>
          </div>
        </aside>

        {/* Main list */}
        <main className="flex min-h-0 flex-col">
          {/* Column headers */}
          <div
            className={`grid h-8 shrink-0 ${COLS} items-center border-b border-white/[0.055] bg-white/[0.015] px-3`}
          >
            <ColHeader label="Name" sortKey="name" currentSort={sortKey} currentDir={sortDir} onSort={handleSort} />
            <ColHeader label="Vendor" sortKey="vendor" currentSort={sortKey} currentDir={sortDir} onSort={handleSort} />
            <ColHeader label="Category" sortKey="category" currentSort={sortKey} currentDir={sortDir} onSort={handleSort} />
            <ColHeader label="Format" sortKey="format" currentSort={sortKey} currentDir={sortDir} onSort={handleSort} />
            <span className="text-[10px] font-semibold uppercase tracking-[0.12em] text-daw-faint">
              Status
            </span>
          </div>

          {/* Rows */}
          <div className="min-h-0 flex-1 overflow-y-auto">
            {visiblePlugins.length === 0 ? (
              <div className="flex h-full min-h-[120px] items-center justify-center text-[11px] text-daw-faint">
                {scanning
                  ? "Scanning… plug-ins will appear here one by one."
                  : plugins.length === 0
                    ? "No cached plug-ins yet. Run Rescan to build the registry."
                    : "No plug-ins match the current filter."}
              </div>
            ) : (
              visiblePlugins.map((plugin) => (
                <PluginRow
                  key={plugin.id}
                  plugin={plugin}
                  cols={COLS}
                  selected={selectedId === plugin.id}
                  onSelect={() => setSelectedId(plugin.id === selectedId ? null : plugin.id)}
                  onReveal={() => void platform.pluginHost.revealPreset(plugin.id)}
                />
              ))
            )}
          </div>
        </main>
      </div>

      {/* Status bar */}
      <div className="flex h-8 shrink-0 items-center justify-between border-t border-white/[0.07] bg-black/20 px-3">
        <div className="flex items-center gap-2 text-[10px] text-daw-faint">
          <Info size={11} />
          <span>{statusText}</span>
          {scanning && generatedPresets > 0 && (
            <span className="text-daw-accent">• {generatedPresets} generated</span>
          )}
          {failedCount > 0 && (
            <span className="text-amber-300/80">• {failedCount} failed</span>
          )}
        </div>
        <span className="tabular-nums text-[10px] text-daw-faint">
          {plugins.length} cached
        </span>
      </div>
    </div>
  );
}

function SidebarSection({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="mb-1">
      <div className="px-3 pb-0.5 pt-2 text-[9px] font-semibold uppercase tracking-[0.15em] text-daw-faint/55">
        {label}
      </div>
      <div className="px-1">{children}</div>
    </div>
  );
}

function SidebarItem({
  label,
  count,
  active,
  onClick,
  disabled,
}: {
  label: string;
  count?: number;
  active?: boolean;
  onClick?: () => void;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className={`flex w-full items-center gap-2 rounded-md px-2 py-[5px] text-left text-[11px] transition-colors ${
        active
          ? "bg-daw-accent/[0.12] text-daw-accent"
          : disabled
            ? "cursor-default text-daw-faint/35"
            : "text-daw-dim hover:bg-white/[0.04] hover:text-daw-text"
      }`}
    >
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {count !== undefined && (
        <span className={`tabular-nums text-[10px] ${active ? "text-daw-accent/65" : "text-daw-faint/55"}`}>
          {count}
        </span>
      )}
    </button>
  );
}

function ColHeader({
  label,
  sortKey: key,
  currentSort,
  currentDir,
  onSort,
}: {
  label: string;
  sortKey: SortKey;
  currentSort: SortKey;
  currentDir: SortDir;
  onSort: (key: SortKey) => void;
}) {
  const active = currentSort === key;
  return (
    <button
      type="button"
      onClick={() => onSort(key)}
      className={`flex items-center gap-1 text-[10px] font-semibold uppercase tracking-[0.12em] transition-colors ${
        active ? "text-daw-accent/80" : "text-daw-faint hover:text-daw-dim"
      }`}
    >
      {label}
      <span>
        {active ? (
          currentDir === "asc" ? <ChevronUp size={10} /> : <ChevronDown size={10} />
        ) : (
          <ChevronsUpDown size={10} className="opacity-25" />
        )}
      </span>
    </button>
  );
}

function PluginRow({
  plugin,
  cols,
  selected,
  onSelect,
  onReveal,
}: {
  plugin: AudioPluginRegistryEntry;
  cols: string;
  selected: boolean;
  onSelect: () => void;
  onReveal: () => void;
}) {
  const isInstrument = plugin.kind === "instrument";
  const fmtBadge = FORMAT_BADGE[plugin.format] ?? "text-daw-faint bg-white/[0.05] border-white/[0.1]";

  return (
    <div
      onClick={onSelect}
      className={`grid min-h-[36px] cursor-pointer ${cols} items-center border-b border-white/[0.04] px-3 text-[11px] ${
        selected ? "bg-daw-accent/[0.07]" : "hover:bg-white/[0.025]"
      }`}
    >
      <div className="flex min-w-0 items-center gap-2">
        {isInstrument ? (
          <Music2 size={12} className="shrink-0 text-daw-accent/70" />
        ) : (
          <SlidersHorizontal size={12} className="shrink-0 text-emerald-400/60" />
        )}
        <span className="truncate font-medium text-daw-text" title={plugin.name}>
          {plugin.name}
        </span>
      </div>

      <span className="truncate text-daw-dim" title={plugin.vendor}>
        {plugin.vendor}
      </span>

      <span className="truncate text-daw-dim" title={plugin.rawCategory ? `${pluginDisplayCategory(plugin)} • SDK: ${plugin.rawCategory}` : pluginDisplayCategory(plugin)}>
        {pluginDisplayCategory(plugin)}
      </span>

      <span>
        <span className={`inline-block rounded border px-1.5 py-0.5 text-[9px] font-semibold ${fmtBadge}`}>
          {plugin.format}
        </span>
      </span>

      <span>
        <button
          type="button"
          onClick={(e) => { e.stopPropagation(); onReveal(); }}
          title={`Reveal preset: ${plugin.presetPath}`}
          className="inline-flex items-center gap-1 rounded border border-emerald-300/20 bg-emerald-300/[0.08] px-1.5 py-0.5 text-[9px] font-semibold text-emerald-200/80 hover:bg-emerald-300/[0.14]"
        >
          <CheckCircle2 size={9} />
          Available
        </button>
      </span>
    </div>
  );
}
