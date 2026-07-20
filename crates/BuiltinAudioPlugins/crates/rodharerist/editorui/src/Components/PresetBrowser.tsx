import { useMemo, useState } from "react";
import { categories, type Preset } from "../data";

type PresetBrowserProps = {
  presets: Preset[];
  currentPresetId: string;
  modifiedIds?: ReadonlySet<string>;
  onLoadPreset: (id: string) => void;
};

export function PresetBrowser({
  presets,
  currentPresetId,
  modifiedIds,
  onLoadPreset,
}: PresetBrowserProps) {
  const [query, setQuery] = useState("");

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
      <div className="browser-head">Presets</div>
      <div className="search-wrap">
        <input
          type="text"
          className="search"
          placeholder="Search presets…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
      </div>
      <div className="preset-list">
        {filtered.map((p) => {
          const c = categories[p.category];
          const dirty = modifiedIds?.has(p.id);
          return (
            <button
              key={p.id}
              type="button"
              className={`preset-item${p.id === currentPresetId ? " active" : ""}`}
              style={{ ["--pc" as string]: c.color }}
              onClick={() => onLoadPreset(p.id)}
            >
              <span className="dot" />
              <span className="pid">{p.id}</span>
              <span className="pname">
                {p.name}
                {dirty ? " *" : ""}
              </span>
            </button>
          );
        })}
      </div>
    </aside>
  );
}
