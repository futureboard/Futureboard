import {
  AlertTriangle,
  CheckCircle2,
  Folder,
  Info,
  ListFilter,
  Loader2,
  Music2,
  Plus,
  RefreshCw,
  Search,
  ShieldAlert,
  SlidersHorizontal,
  Trash2,
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
type PluginStatus = "available" | "blocked" | "missing";

const FORMAT_STATE: Array<{ format: PluginFormat; enabled: boolean; detail: string }> = [
  { format: "VST3", enabled: true, detail: "Native scanner, .pst generation, SQLite cache" },
  { format: "CLAP", enabled: false, detail: "UI placeholder" },
  { format: "AU", enabled: false, detail: "macOS only" },
];

const STATUS_STYLE: Record<PluginStatus, { label: string; className: string }> = {
  available: {
    label: "Available",
    className: "border-emerald-300/20 bg-emerald-300/10 text-emerald-200/85",
  },
  blocked: {
    label: "Blocked",
    className: "border-amber-300/20 bg-amber-300/10 text-amber-200/85",
  },
  missing: {
    label: "Missing",
    className: "border-red-300/20 bg-red-300/10 text-red-200/85",
  },
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
  const close = () => {
    if (external && platform.kind === "electron") platform.window.close();
    else useWindowStore.getState().closeWindow(windowId);
  };
  const refresh = async () => {
    const [status, cached] = await Promise.all([
      platform.pluginHost.getStatus(),
      platform.pluginHost.listPlugins(),
    ]);
    setScanPaths(status.defaultScanPaths);
    setPlugins(cached);
    setStatusText(status.available ? status.message : "PluginHost native backend is unavailable.");
  };
  const scan = async () => {
    if (!platform.pluginHost.isSupported || scanning) return;
    setScanning(true);
    setStatusText("Scanning VST3 folders and generating .pst presets...");
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
    void refresh();
  }, []);

  useEffect(() => {
    if (!platform.pluginHost.isSupported) return;
    return platform.pluginHost.onScanProgress((event) => {
      if (event.type === "started") {
        setScanning(true);
        setFailedCount(0);
        setGeneratedPresets(0);
        setScanPaths(event.scannedPaths);
        setStatusText("Scanning VST3 folders...");
        return;
      }
      if (event.type === "folder") {
        setStatusText(`Searching ${event.path} • ${event.discovered} plug-ins found`);
        return;
      }
      if (event.type === "plugin") {
        setGeneratedPresets(event.generatedPresets);
        setPlugins((current) => {
          const next = current.filter((plugin) => plugin.id !== event.plugin.id);
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

  const visiblePlugins = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return plugins;
    return plugins.filter((plugin) =>
      `${plugin.name} ${plugin.vendor} ${plugin.category} ${plugin.path}`.toLowerCase().includes(q),
    );
  }, [plugins, query]);

  return (
    <div className={`flex h-full min-h-0 flex-col bg-[#0e1319] text-daw-text ${external ? "" : "rounded-b-xl"}`}>
      <div className="flex h-10 shrink-0 items-center justify-between border-b border-white/[0.07] bg-[#111820] px-3">
        <div className="flex min-w-0 items-center gap-2">
          <SlidersHorizontal size={15} className="text-daw-accent" />
          <div className="min-w-0">
            <div className="truncate text-[12px] font-semibold">Audio Plug-in Manager</div>
            <div className="truncate text-[10px] text-daw-faint">
              VST3 scanner cache, Mochi .pst presets, and Electron userData registry
            </div>
          </div>
        </div>
        <button
          type="button"
          onClick={close}
          title="Close"
          className="flex h-7 w-7 items-center justify-center rounded-md text-daw-faint hover:bg-white/[0.06] hover:text-daw-text"
        >
          <X size={14} />
        </button>
      </div>

      {!isElectron && (
        <div className="mx-3 mt-3 flex shrink-0 items-center gap-2 rounded-md border border-amber-300/20 bg-amber-300/[0.08] px-3 py-2 text-[11px] text-amber-100/80">
          <ShieldAlert size={14} className="shrink-0" />
          Audio plug-in management is available only in the Electron client.
        </div>
      )}

      <div className="grid min-h-0 flex-1 grid-cols-[260px_minmax(0,1fr)_280px] overflow-hidden">
        <aside className="flex min-h-0 flex-col border-r border-white/[0.06] bg-[#0b1016]">
          <SectionTitle>Scan Locations</SectionTitle>
          <div className="flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto px-2 pb-2">
            {(scanPaths.length ? scanPaths : ["No VST3 scan folders detected"]).map((path) => (
              <div key={path} className="group rounded-md border border-white/[0.06] bg-white/[0.025] px-2 py-2">
                <div className="flex items-center gap-2">
                  <Folder size={13} className="shrink-0 text-daw-accent/80" />
                  <span className="min-w-0 flex-1 truncate text-[11px] text-daw-text" title={path}>
                    {path}
                  </span>
                </div>
                <div className="mt-1 truncate pl-5 text-[10px] text-daw-faint">VST3 scan source</div>
              </div>
            ))}
          </div>
          <div className="border-t border-white/[0.06] p-2">
            <button
              type="button"
              disabled
              className="flex h-8 w-full items-center justify-center gap-2 rounded-md border border-white/[0.08] bg-white/[0.03] text-[11px] font-medium text-daw-faint opacity-55"
              title="Backend not implemented yet"
            >
              <Plus size={13} />
              Add Location
            </button>
          </div>
        </aside>

        <main className="flex min-h-0 flex-col">
          <div className="flex h-11 shrink-0 items-center gap-2 border-b border-white/[0.06] px-3">
            <div className="flex h-7 min-w-0 flex-1 items-center gap-2 rounded-md border border-white/[0.08] bg-[#0b1016] px-2">
              <Search size={13} className="shrink-0 text-daw-faint" />
              <input
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                placeholder="Search plug-ins"
                className="min-w-0 flex-1 bg-transparent text-[11px] text-daw-text outline-none placeholder:text-daw-faint disabled:opacity-70"
              />
            </div>
            <button
              type="button"
              disabled
              className="flex h-7 items-center gap-1.5 rounded-md border border-white/[0.08] bg-white/[0.03] px-2 text-[11px] text-daw-faint opacity-55"
              title="Filter UI only"
            >
              <ListFilter size={13} />
              Filter
            </button>
            <button
              type="button"
              disabled={!isElectron || scanning}
              onClick={scan}
              className="flex h-7 items-center gap-1.5 rounded-md border border-daw-accent/25 bg-daw-accent/10 px-2 text-[11px] font-semibold text-daw-accent hover:bg-daw-accent/15 disabled:opacity-55"
              title="Scan VST3 folders and generate Mochi .pst presets"
            >
              {scanning ? <Loader2 size={13} className="animate-spin" /> : <RefreshCw size={13} />}
              Scan
            </button>
          </div>

          <div className="grid h-8 shrink-0 grid-cols-[minmax(180px,1.4fr)_minmax(120px,0.8fr)_72px_110px_96px_82px] items-center border-b border-white/[0.055] bg-white/[0.018] px-3 text-[10px] font-semibold uppercase tracking-[0.13em] text-daw-faint">
            <span>Plug-in</span>
            <span>Vendor</span>
            <span>Format</span>
            <span>Category</span>
            <span>Type</span>
            <span>Status</span>
          </div>

          <div className="min-h-0 flex-1 overflow-y-auto">
            {visiblePlugins.map((plugin) => (
              <PluginRowView key={plugin.id} plugin={plugin} onReveal={() => void platform.pluginHost.revealPreset(plugin.id)} />
            ))}
            {visiblePlugins.length === 0 && (
              <div className="flex h-full min-h-[160px] items-center justify-center text-[11px] text-daw-faint">
                {scanning ? "Scanning... plug-ins will appear here one by one." : plugins.length === 0 ? "No cached plug-ins yet. Run Scan to build the registry." : "No plug-ins match the current search."}
              </div>
            )}
          </div>
        </main>

        <aside className="flex min-h-0 flex-col border-l border-white/[0.06] bg-[#0b1016]">
          <SectionTitle>Formats</SectionTitle>
          <div className="space-y-1 px-2">
            {FORMAT_STATE.map((item) => (
              <div key={item.format} className="rounded-md border border-white/[0.06] bg-white/[0.025] px-2 py-2">
                <div className="flex items-center justify-between">
                  <span className="text-[11px] font-semibold text-daw-text">{item.format}</span>
                  <span className={`rounded border px-1.5 py-0.5 text-[9px] ${item.enabled ? "border-daw-accent/30 text-daw-accent" : "border-white/[0.08] text-daw-faint"}`}>
                    {item.enabled ? "Shown" : "Disabled"}
                  </span>
                </div>
                <div className="mt-1 text-[10px] leading-snug text-daw-faint">{item.detail}</div>
              </div>
            ))}
          </div>

          <SectionTitle>Safety</SectionTitle>
          <div className="mx-2 rounded-md border border-amber-300/[0.18] bg-amber-300/[0.08] p-2">
            <div className="flex items-center gap-2 text-[11px] font-semibold text-amber-100/85">
              <AlertTriangle size={13} />
              Scanner safety
            </div>
            <p className="mt-1 text-[10px] leading-snug text-daw-faint">
              Scanner reads VST3 factory metadata and writes preset/cache files. It does not instantiate processors or route realtime audio yet.
            </p>
          </div>

          <SectionTitle>Actions</SectionTitle>
          <div className="grid gap-1 px-2">
            <ActionButton icon={<RefreshCw size={13} />} label="Rescan Selected" />
            <ActionButton icon={<Trash2 size={13} />} label="Forget Missing" />
          </div>
        </aside>
      </div>

      <div className="flex h-9 shrink-0 items-center justify-between border-t border-white/[0.07] bg-black/20 px-3">
        <div className="flex items-center gap-2 text-[10px] text-daw-faint">
          <Info size={12} />
          {statusText}
          {scanning ? <span className="text-daw-accent"> • {generatedPresets} generated</span> : null}
          {failedCount > 0 ? <span className="text-amber-200/80"> • {failedCount} failed</span> : null}
        </div>
        <button
          type="button"
          onClick={close}
          className="h-7 rounded-md border border-daw-accent/30 bg-daw-accent/10 px-3 text-[11px] font-semibold text-daw-accent hover:bg-daw-accent/15"
        >
          Done
        </button>
      </div>
    </div>
  );
}

function SectionTitle({ children }: { children: ReactNode }) {
  return (
    <div className="px-3 pb-2 pt-3 text-[10px] font-semibold uppercase tracking-[0.14em] text-daw-faint">
      {children}
    </div>
  );
}

function PluginRowView({ plugin, onReveal }: { plugin: AudioPluginRegistryEntry; onReveal: () => void }) {
  const status = STATUS_STYLE.available;
  return (
    <div className="grid min-h-[44px] grid-cols-[minmax(180px,1.4fr)_minmax(120px,0.8fr)_72px_110px_96px_82px] items-center border-b border-white/[0.045] px-3 text-[11px] hover:bg-white/[0.025]">
      <div className="min-w-0">
        <div className="flex items-center gap-2">
          {plugin.kind === "instrument" ? (
            <Music2 size={13} className="shrink-0 text-daw-accent/85" />
          ) : (
            <CheckCircle2 size={13} className="shrink-0 text-emerald-300/80" />
          )}
          <span className="truncate font-semibold text-daw-text">{plugin.name}</span>
        </div>
        <div className="mt-0.5 truncate pl-5 text-[10px] text-daw-faint" title={plugin.presetPath}>
          {plugin.presetPath}
        </div>
      </div>
      <span className="truncate text-daw-dim">{plugin.vendor}</span>
      <span className="tabular-nums text-daw-dim">{plugin.format}</span>
      <span className="truncate text-daw-dim">{plugin.category}</span>
      <span className="truncate text-daw-dim">{plugin.kind === "instrument" ? "Instrument" : "Effect"}</span>
      <span className={`w-fit rounded border px-1.5 py-0.5 text-[9px] font-semibold ${status.className}`}>
        <button type="button" onClick={onReveal} className="outline-none">
          {status.label}
        </button>
      </span>
    </div>
  );
}

function ActionButton({ icon, label }: { icon: ReactNode; label: string }) {
  return (
    <button
      type="button"
      disabled
      className="flex h-8 items-center gap-2 rounded-md border border-white/[0.08] bg-white/[0.03] px-2 text-left text-[11px] text-daw-faint opacity-55"
      title="Backend not implemented yet"
    >
      {icon}
      {label}
    </button>
  );
}
