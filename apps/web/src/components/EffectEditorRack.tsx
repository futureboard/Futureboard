import { useEffect, useRef, useState } from "react";
import { GripVertical, Plus, Power, Sliders, X } from "lucide-react";
import { useUIStore } from "../store/uiStore";
import { useProjectStore } from "../store/projectStore";
import type { InsertDevice } from "../types/daw";

type Param = { name: string; value: number; min: number; max: number; unit: string };

const DEVICE_TEMPLATES: Record<string, Param[]> = {
  EQ: [
    { name: "Low", value: 0, min: -12, max: 12, unit: "dB" },
    { name: "Mid", value: 0, min: -12, max: 12, unit: "dB" },
    { name: "High", value: 0, min: -12, max: 12, unit: "dB" },
  ],
  Compressor: [
    { name: "Thresh", value: -24, min: -60, max: 0, unit: "dB" },
    { name: "Ratio", value: 4, min: 1, max: 20, unit: ":1" },
    { name: "Attack", value: 10, min: 0.1, max: 200, unit: "ms" },
  ],
  Reverb: [
    { name: "Size", value: 0.5, min: 0, max: 1, unit: "" },
    { name: "Damp", value: 0.5, min: 0, max: 1, unit: "" },
    { name: "Wet", value: 0.3, min: 0, max: 1, unit: "" },
  ],
  Delay: [
    { name: "Time", value: 250, min: 1, max: 2000, unit: "ms" },
    { name: "Fdbk", value: 0.3, min: 0, max: 0.95, unit: "" },
    { name: "Wet", value: 0.25, min: 0, max: 1, unit: "" },
  ],
  Saturation: [
    { name: "Drive", value: 0.2, min: 0, max: 1, unit: "" },
    { name: "Char", value: 0.5, min: 0, max: 1, unit: "" },
  ],
};

const BUILT_IN_DEVICES = ["EQ", "Compressor", "Reverb", "Delay", "Saturation"];

function getDefaultParams(name: string): Param[] {
  return (
    DEVICE_TEMPLATES[name] ?? [
      { name: "Param 1", value: 0.5, min: 0, max: 1, unit: "" },
      { name: "Param 2", value: 0.5, min: 0, max: 1, unit: "" },
    ]
  );
}

function patchTrackInserts(
  trackId: string,
  updater: (inserts: InsertDevice[]) => InsertDevice[]
) {
  useProjectStore.setState((state) => ({
    project: {
      ...state.project,
      tracks: state.project.tracks.map((t) =>
        t.id !== trackId ? t : { ...t, inserts: updater(t.inserts ?? []) }
      ),
    },
  }));
}

export function EffectEditorRack() {
  const { selectedTrackId } = useUIStore();
  const { project } = useProjectStore();
  const [showAddMenu, setShowAddMenu] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  const track = selectedTrackId
    ? project.tracks.find((t) => t.id === selectedTrackId)
    : null;

  // Close add-device menu on outside click
  useEffect(() => {
    if (!showAddMenu) return;
    const handler = (e: MouseEvent) => {
      if (!menuRef.current?.contains(e.target as Node)) setShowAddMenu(false);
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [showAddMenu]);

  const addInsert = (deviceName: string) => {
    if (!selectedTrackId) return;
    patchTrackInserts(selectedTrackId, (inserts) => [
      ...inserts,
      { id: crypto.randomUUID(), type: "custom", name: deviceName, enabled: true, order: inserts.length, params: {} },
    ]);
    setShowAddMenu(false);
  };

  const toggleBypass = (insertId: string) => {
    if (!selectedTrackId) return;
    patchTrackInserts(selectedTrackId, (inserts) =>
      inserts.map((ins) =>
        ins.id === insertId ? { ...ins, enabled: !ins.enabled } : ins
      )
    );
  };

  const removeInsert = (insertId: string) => {
    if (!selectedTrackId) return;
    patchTrackInserts(selectedTrackId, (inserts) =>
      inserts.filter((ins) => ins.id !== insertId)
    );
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

  const inserts = track.inserts ?? [];

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
      <div
        className="flex min-h-0 flex-1 overflow-x-auto overflow-y-hidden"
        style={{ background: "#0f1218" }}
      >
        <div className="flex h-full gap-1.5 p-2">
          {inserts.map((ins) => (
            <DeviceCard
              key={ins.id}
              insert={ins}
              onToggleBypass={() => toggleBypass(ins.id)}
              onRemove={() => removeInsert(ins.id)}
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
                className="absolute bottom-full left-0 z-20 mb-1 w-44 overflow-hidden rounded-lg border shadow-2xl"
                style={{
                  background: "#1a1e26",
                  borderColor: "rgba(255,255,255,0.1)",
                  boxShadow: "0 8px 32px rgba(0,0,0,0.55)",
                }}
              >
                <div className="px-2.5 py-2 text-[9px] font-semibold uppercase tracking-widest text-daw-faint">
                  Built-in Devices
                </div>
                <div className="pb-1">
                  {BUILT_IN_DEVICES.map((name) => (
                    <button
                      key={name}
                      type="button"
                      onClick={() => addInsert(name)}
                      className="flex w-full items-center gap-2 px-2.5 py-1.5 text-[11px] text-daw-dim transition-colors hover:bg-white/[0.06] hover:text-daw-text"
                    >
                      <Sliders size={10} className="shrink-0 text-daw-faint" />
                      {name}
                    </button>
                  ))}
                </div>
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function DeviceCard({
  insert,
  onToggleBypass,
  onRemove,
}: {
  insert: InsertDevice;
  onToggleBypass: () => void;
  onRemove: () => void;
}) {
  const [params, setParams] = useState<Param[]>(() => getDefaultParams(insert.name));

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
      {/* Device header */}
      <div
        className="flex h-8 shrink-0 items-center gap-1 border-b px-1.5"
        style={{ borderColor: "rgba(255,255,255,0.07)", background: "rgba(0,0,0,0.2)" }}
      >
        <GripVertical size={10} className="cursor-grab text-daw-faint opacity-40" />
        <span className="flex-1 truncate px-0.5 text-[11px] font-semibold text-daw-dim">
          {insert.name}
        </span>
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
          <Power size={9} />
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

      {/* Parameters */}
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
