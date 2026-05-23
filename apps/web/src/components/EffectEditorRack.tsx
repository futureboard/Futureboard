import { useEffect, useRef, useState } from "react";
import { GripVertical, Plus, Sliders, X } from "lucide-react";
import { useUIStore } from "../store/uiStore";
import { useProjectStore } from "../store/projectStore";
import { getTrackInserts } from "../store/selectors";
import type { InsertDevice } from "../types/daw";
import { BUILT_IN_PLUGINS, findPlugin } from "../plugins/registry";
import { mixer } from "../engine/Mixer";

type Param = { name: string; value: number; min: number; max: number; unit: string };

let lastRackInsertAdd:
  | { trackId: string; pluginId: string; at: number }
  | null = null;

const GENERIC_TEMPLATES: Record<string, Param[]> = {
  Compressor: [
    { name: "Thresh", value: -24, min: -60, max: 0,   unit: "dB" },
    { name: "Ratio",  value: 4,   min: 1,   max: 20,  unit: ":1" },
    { name: "Attack", value: 10,  min: 0.1, max: 200, unit: "ms" },
  ],
  Reverb: [
    { name: "Size", value: 0.5, min: 0, max: 1, unit: "" },
    { name: "Damp", value: 0.5, min: 0, max: 1, unit: "" },
    { name: "Wet",  value: 0.3, min: 0, max: 1, unit: "" },
  ],
  Delay: [
    { name: "Time", value: 250,  min: 1,   max: 2000, unit: "ms" },
    { name: "Fdbk", value: 0.3,  min: 0,   max: 0.95, unit: "" },
    { name: "Wet",  value: 0.25, min: 0,   max: 1,    unit: "" },
  ],
  Saturation: [
    { name: "Drive", value: 0.2, min: 0, max: 1, unit: "" },
    { name: "Char",  value: 0.5, min: 0, max: 1, unit: "" },
  ],
};

function getGenericParams(name: string): Param[] {
  return (
    GENERIC_TEMPLATES[name] ?? [
      { name: "Param 1", value: 0.5, min: 0, max: 1, unit: "" },
      { name: "Param 2", value: 0.5, min: 0, max: 1, unit: "" },
    ]
  );
}

export function EffectEditorRack() {
  const { selectedTrackId } = useUIStore();
  const { project, addInsertDevice, toggleInsertDevice, removeInsertDevice, updateInsertDeviceParams } =
    useProjectStore();
  const [showAddMenu, setShowAddMenu] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  const track = selectedTrackId ? project.tracks.find((t) => t.id === selectedTrackId) : null;

  useEffect(() => {
    if (!showAddMenu) return;
    const handler = (e: MouseEvent) => {
      if (!menuRef.current?.contains(e.target as Node)) setShowAddMenu(false);
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [showAddMenu]);

  const addInsert = (plugin: typeof BUILT_IN_PLUGINS[number]) => {
    if (!selectedTrackId) return;
    const now = performance.now();
    if (
      lastRackInsertAdd &&
      lastRackInsertAdd.trackId === selectedTrackId &&
      lastRackInsertAdd.pluginId === plugin.id &&
      now - lastRackInsertAdd.at < 450
    ) {
      setShowAddMenu(false);
      return;
    }
    lastRackInsertAdd = { trackId: selectedTrackId, pluginId: plugin.id, at: now };
    const device: InsertDevice = {
      id: crypto.randomUUID(),
      type: plugin.type,
      name: plugin.name,
      enabled: true,
      order: 0,
      params: plugin.defaultParams(),
    };
    addInsertDevice(selectedTrackId, device);
    setShowAddMenu(false);
  };

  const toggleBypass = (insertId: string) => {
    if (!selectedTrackId) return;
    toggleInsertDevice(selectedTrackId, insertId);
  };

  const removeInsert = (insertId: string) => {
    if (!selectedTrackId) return;
    removeInsertDevice(selectedTrackId, insertId);
  };

  if (!track) {
    return (
      <div className="flex min-h-0 flex-1 items-center justify-center">
        <div className="flex flex-col items-center gap-2 text-center">
          <div className="flex h-9 w-9 items-center justify-center rounded-lg border border-white/[0.06] bg-white/[0.025] text-daw-faint">
            <Sliders size={16} />
          </div>
          <p className="text-[12px] font-semibold text-daw-dim">No track selected</p>
          <p className="max-w-[28ch] text-[11px] leading-relaxed text-daw-faint">
            Select a track to view and edit its effect chain.
          </p>
        </div>
      </div>
    );
  }

  const inserts = getTrackInserts(track);

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      {/* Track info strip */}
      <div
        className="flex h-8 shrink-0 items-center gap-2 border-b px-3"
        style={{ borderColor: "rgba(255,255,255,0.06)" }}
      >
        <div className="h-2.5 w-2.5 shrink-0 rounded-full" style={{ background: track.color }} />
        <span className="text-[12px] font-semibold text-daw-text">{track.name}</span>
        <span className="text-[10px] text-daw-faint">· Effect Chain</span>
        <span className="ml-auto text-[10px] tabular-nums text-daw-faint">
          {inserts.length} device{inserts.length !== 1 ? "s" : ""}
        </span>
      </div>

      {/* Horizontal rack */}
      <div className="flex min-h-0 flex-1 overflow-x-auto overflow-y-hidden" style={{ background: "#0f1218" }}>
        <div className="flex h-full gap-1.5 p-2">
          {inserts.map((ins) => (
            <DeviceCard
              key={ins.id}
              insert={ins}
              trackId={track.id}
              onToggleBypass={() => toggleBypass(ins.id)}
              onRemove={() => removeInsert(ins.id)}
              onParamsChange={(patch) =>
                selectedTrackId && updateInsertDeviceParams(selectedTrackId, ins.id, patch)
              }
            />
          ))}

          {/* Add Device slot */}
          <div ref={menuRef} className="relative flex h-full flex-col">
            <button
              type="button"
              onClick={() => setShowAddMenu((v) => !v)}
              className="flex h-full w-36 shrink-0 flex-col items-center justify-center gap-2 rounded-lg border border-dashed transition-colors hover:border-white/25 hover:bg-white/[0.03]"
              style={{ borderColor: "rgba(255,255,255,0.1)", color: "rgba(180,192,204,0.5)" }}
            >
              <Plus size={14} />
              <span className="text-[11px] font-medium">Add Device</span>
            </button>

            {showAddMenu && (
              <div
                className="absolute bottom-full left-0 z-20 mb-1 w-52 overflow-hidden rounded-lg border shadow-2xl"
                style={{
                  background: "#1a1e26",
                  borderColor: "rgba(255,255,255,0.1)",
                  boxShadow: "0 8px 32px rgba(0,0,0,0.55)",
                }}
              >
                {/* EQ */}
                <AddMenuSection label="EQ">
                  {BUILT_IN_PLUGINS.filter((p) => p.category === "eq").map((p) => (
                    <AddMenuItem key={p.id} plugin={p} onAdd={addInsert} />
                  ))}
                </AddMenuSection>

                {/* Space */}
                <AddMenuSection label="Space">
                  {BUILT_IN_PLUGINS.filter((p) => p.category === "space").map((p) => (
                    <AddMenuItem key={p.id} plugin={p} onAdd={addInsert} />
                  ))}
                </AddMenuSection>

                {/* Dynamics */}
                <AddMenuSection label="Dynamics">
                  {BUILT_IN_PLUGINS.filter((p) => p.category === "dynamics").map((p) => (
                    <AddMenuItem key={p.id} plugin={p} onAdd={addInsert} />
                  ))}
                </AddMenuSection>
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function AddMenuSection({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="pb-1">
      <div className="px-2.5 py-2 text-[9px] font-semibold uppercase tracking-widest text-daw-faint">
        {label}
      </div>
      {children}
    </div>
  );
}

function AddMenuItem({
  plugin,
  onAdd,
}: {
  plugin: typeof BUILT_IN_PLUGINS[number];
  onAdd: (p: typeof BUILT_IN_PLUGINS[number]) => void;
}) {
  return (
    <button
      type="button"
      onClick={() => onAdd(plugin)}
      className="flex w-full items-center gap-2 px-2.5 py-1.5 text-[11px] text-daw-dim transition-colors hover:bg-white/[0.06] hover:text-daw-text"
    >
      <span
        className="h-[7px] w-[7px] shrink-0 rounded-full"
        style={{ background: plugin.color, boxShadow: `0 0 6px ${plugin.color}80` }}
      />
      <span>{plugin.name}</span>
      <span className="ml-auto text-[9px] text-daw-faint">{plugin.shortName}</span>
    </button>
  );
}

function DeviceCard({
  insert,
  trackId,
  onToggleBypass,
  onRemove,
  onParamsChange,
}: {
  insert: InsertDevice;
  trackId: string;
  onToggleBypass: () => void;
  onRemove: () => void;
  onParamsChange: (patch: Record<string, number | string | boolean>) => void;
}) {
  // Try to find a built-in plugin editor for this insert
  const plugin = findPlugin(insert.name) ?? findPlugin(insert.type);

  if (plugin) {
    return (
      <plugin.Editor
        params={insert.params}
        enabled={insert.enabled}
        onParamsChange={onParamsChange}
        onToggleEnabled={onToggleBypass}
        onReset={() => onParamsChange(plugin.defaultParams())}
        getSpectrum={() => mixer.getSpectrum(trackId)}
      />
    );
  }

  // Generic fallback card for unknown devices
  return <GenericDeviceCard insert={insert} onToggleBypass={onToggleBypass} onRemove={onRemove} />;
}

function GenericDeviceCard({
  insert,
  onToggleBypass,
  onRemove,
}: {
  insert: InsertDevice;
  onToggleBypass: () => void;
  onRemove: () => void;
}) {
  const [params, setParams] = useState<Param[]>(() => getGenericParams(insert.name));

  const setParam = (i: number, v: number) => {
    setParams((prev) => prev.map((p, idx) => (idx === i ? { ...p, value: v } : p)));
  };

  function formatParamValue(p: Param): string {
    if (!p.unit) return `${Math.round(p.value * 100)}%`;
    if (p.unit === "dB") return `${p.value >= 0 ? "+" : ""}${p.value.toFixed(1)}dB`;
    if (p.unit === ":1") return `${p.value.toFixed(1)}:1`;
    if (p.unit === "ms") return `${Math.round(p.value)}ms`;
    return `${p.value.toFixed(2)}${p.unit}`;
  }

  return (
    <div
      className="flex h-full w-44 shrink-0 flex-col overflow-hidden rounded-lg border transition-opacity"
      style={{
        borderColor: !insert.enabled ? "rgba(255,255,255,0.06)" : "rgba(255,255,255,0.12)",
        background: !insert.enabled ? "#0e1116" : "#161b22",
        opacity: !insert.enabled ? 0.55 : 1,
      }}
    >
      <div
        className="flex h-8 shrink-0 items-center gap-1 border-b px-1.5"
        style={{ borderColor: "rgba(255,255,255,0.07)", background: "rgba(0,0,0,0.2)" }}
      >
        <GripVertical size={10} className="cursor-grab text-daw-faint opacity-40" />
        <span className="flex-1 truncate px-0.5 text-[11px] font-semibold text-daw-dim">{insert.name}</span>
        <button
          type="button"
          onClick={onToggleBypass}
          title={!insert.enabled ? "Enable device" : "Bypass device"}
          className="flex h-5 w-5 shrink-0 items-center justify-center rounded transition-colors"
          style={{
            color: !insert.enabled ? "rgba(180,192,204,0.3)" : "#56c7c9",
            background: !insert.enabled ? "transparent" : "rgba(86,199,201,0.12)",
          }}
        >
          <span style={{ fontSize: "9px", fontWeight: 700 }}>⏻</span>
        </button>
        <button
          type="button"
          onClick={onRemove}
          title="Remove device"
          className="flex h-5 w-5 shrink-0 items-center justify-center rounded text-daw-faint transition-colors hover:bg-red-500/15 hover:text-red-400"
        >
          <X size={9} />
        </button>
      </div>

      <div className="flex flex-1 flex-col justify-center gap-3 overflow-y-auto px-3 py-2">
        {params.map((p, i) => (
          <div key={p.name} className="flex flex-col gap-1">
            <div className="flex items-center justify-between">
              <span className="text-[9px] text-daw-faint">{p.name}</span>
              <span className="text-[9px] tabular-nums text-daw-dim">{formatParamValue(p)}</span>
            </div>
            <input
              type="range"
              min={p.min}
              max={p.max}
              step={(p.max - p.min) / 200}
              value={p.value}
              onChange={(e) => setParam(i, parseFloat(e.target.value))}
              className="w-full cursor-ew-resize appearance-none"
              style={{ accentColor: "#56c7c9", height: "3px" }}
            />
          </div>
        ))}
      </div>
    </div>
  );
}
