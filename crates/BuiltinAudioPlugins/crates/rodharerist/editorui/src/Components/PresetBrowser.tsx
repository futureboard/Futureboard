import { useMemo, useState } from "react";
import { categories, type Preset } from "../data";

type PresetBrowserProps = {
  presets: Preset[];
  currentPresetId: string;
  onLoadPreset: (id: string) => void;
};

export function PresetBrowser({
  presets,
  currentPresetId,
  onLoadPreset,
}: PresetBrowserProps) {
  const [query, setQuery] = useState("");
  const [tab, setTab] = useState<"presets" | "impulses">("presets");

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return presets;
    return presets.filter(
      (p) =>
        p.id.toLowerCase().includes(q) || p.name.toLowerCase().includes(q),
    );
  }, [presets, query]);

  return (
    <aside className="browser">
      <div className="browser-tabs">
        <button
          className={`browser-tab${tab === "presets" ? " active" : ""}`}
          onClick={() => setTab("presets")}
          type="button"
        >
          Presets
        </button>
        <button
          className={`browser-tab${tab === "impulses" ? " active" : ""}`}
          onClick={() => setTab("impulses")}
          type="button"
        >
          Impulses
        </button>
      </div>
      <div className="search-wrap">
        <input
          type="text"
          className="search"
          placeholder="ค้นหา preset…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
      </div>
      <div className="preset-list">
        {tab === "presets"
          ? filtered.map((p) => {
              const c = categories[p.category];
              return (
                <div
                  key={p.id}
                  className={`preset-item${p.id === currentPresetId ? " active" : ""}`}
                  style={{ ["--pc" as string]: c.color }}
                  onClick={() => onLoadPreset(p.id)}
                >
                  <span className="dot" />
                  <span className="pid">{p.id}</span>
                  <span className="pname">{p.name}</span>
                </div>
              );
            })
          : (
            <div className="preset-item" style={{ opacity: 0.5 }}>
              <span className="dot" />
              <span className="pid">—</span>
              <span className="pname">No impulses loaded</span>
            </div>
          )}
      </div>
    </aside>
  );
}
