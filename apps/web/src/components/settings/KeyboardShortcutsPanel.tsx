import { useMemo, useState } from "react";
import { Search } from "lucide-react";
import {
  getShortcutCommands,
  detectShortcutConflicts,
  ALL_SHORTCUT_CATEGORIES,
  type ShortcutCategory,
  type ShortcutCommand,
} from "../../commands/shortcutCatalog";

// ── Keycap ────────────────────────────────────────────────────────────────────

function ShortcutKeycap({ token }: { token: string }) {
  return (
    <kbd
      className="inline-flex items-center justify-center rounded px-1.5 py-0.5 text-[10px] font-semibold leading-none tabular-nums"
      style={{
        background: "#161b22",
        border: "1px solid rgba(255,255,255,0.12)",
        color: "rgba(255,255,255,0.65)",
        fontFamily: "'JetBrains Mono', 'Fira Code', 'Cascadia Code', ui-monospace, monospace",
        boxShadow: "0 1px 0 rgba(0,0,0,0.4)",
      }}
    >
      {token}
    </kbd>
  );
}

/** Renders a full key chord like "Ctrl+Shift+S" as individual keycaps. */
function KeyCombo({ combo }: { combo: string }) {
  const tokens = combo.split("+").filter(Boolean);
  return (
    <span className="flex items-center gap-[3px]">
      {tokens.map((token, i) => (
        <ShortcutKeycap key={i} token={token} />
      ))}
    </span>
  );
}

// ── Row ───────────────────────────────────────────────────────────────────────

function ShortcutRow({
  cmd,
  hasConflict,
}: {
  cmd: ShortcutCommand;
  hasConflict: boolean;
}) {
  return (
    <div className="group flex min-h-[38px] items-center gap-3 border-b border-[rgba(255,255,255,0.04)] px-1 py-1.5 last:border-0">
      {/* Label + description */}
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="text-[12px] font-medium text-[rgba(255,255,255,0.82)] leading-none">
            {cmd.label}
          </span>
          {hasConflict && (
            <span
              className="rounded px-1 py-0.5 text-[8px] font-bold uppercase tracking-wide leading-none"
              style={{ background: "rgba(239,68,68,0.15)", color: "#f87171" }}
            >
              Conflict
            </span>
          )}
          {cmd.keys.length === 0 && (
            <span
              className="rounded px-1 py-0.5 text-[8px] font-semibold uppercase tracking-wide leading-none"
              style={{ background: "rgba(255,255,255,0.04)", color: "rgba(255,255,255,0.28)" }}
            >
              Unassigned
            </span>
          )}
        </div>
        {cmd.description && (
          <div className="mt-0.5 text-[10px] leading-snug text-[rgba(255,255,255,0.3)]">
            {cmd.description}
          </div>
        )}
        {import.meta.env.DEV && (
          <div className="mt-0.5 text-[9px] font-mono leading-none text-[rgba(255,255,255,0.18)]">
            {cmd.id}
          </div>
        )}
      </div>

      {/* Keycaps */}
      <div className="flex shrink-0 items-center gap-2">
        {cmd.keys.map((combo, i) => (
          <KeyCombo key={i} combo={combo} />
        ))}
      </div>

      {/* Edit placeholder — disabled for now */}
      <button
        type="button"
        disabled
        title="Shortcut editing coming soon"
        className="hidden h-5 w-5 shrink-0 items-center justify-center rounded opacity-0 transition-opacity group-hover:opacity-100"
        style={{ color: "rgba(255,255,255,0.25)" }}
      >
        <svg width="11" height="11" viewBox="0 0 12 12" fill="none">
          <path d="M9 1.5L10.5 3L4.5 9H3V7.5L9 1.5Z" stroke="currentColor" strokeWidth="1.2" strokeLinejoin="round"/>
        </svg>
      </button>
    </div>
  );
}

// ── Category group header ─────────────────────────────────────────────────────

function CategoryHeader({ label, count }: { label: string; count: number }) {
  return (
    <div className="flex items-center gap-2 pb-1 pt-4 first:pt-0">
      <span className="text-[9px] font-semibold uppercase tracking-[0.12em] text-[rgba(255,255,255,0.28)]">
        {label}
      </span>
      <span className="text-[9px] tabular-nums text-[rgba(255,255,255,0.18)]">{count}</span>
      <div className="flex-1 border-t border-[rgba(255,255,255,0.04)]" />
    </div>
  );
}

// ── Main panel ────────────────────────────────────────────────────────────────

type FilterMode = "all" | "unassigned" | "conflicts";

export function KeyboardShortcutsPanel() {
  const [search, setSearch] = useState("");
  const [category, setCategory] = useState<ShortcutCategory | "all">("all");
  const [filterMode, setFilterMode] = useState<FilterMode>("all");

  const allCommands = useMemo(() => getShortcutCommands(), []);
  const conflicts   = useMemo(() => detectShortcutConflicts(allCommands), [allCommands]);

  const conflictKeys = useMemo(() => {
    const keys = new Set<string>();
    for (const c of conflicts) {
      for (const id of c.commandIds) keys.add(id);
    }
    return keys;
  }, [conflicts]);

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    return allCommands.filter((cmd) => {
      // Category filter
      if (category !== "all" && cmd.category !== category) return false;
      // Mode filter
      if (filterMode === "unassigned" && cmd.keys.length > 0) return false;
      if (filterMode === "conflicts"  && !conflictKeys.has(cmd.id)) return false;
      // Search
      if (q) {
        const inLabel    = cmd.label.toLowerCase().includes(q);
        const inId       = cmd.id.toLowerCase().includes(q);
        const inCategory = cmd.category.toLowerCase().includes(q);
        const inKeys     = cmd.keys.some((k) => k.toLowerCase().includes(q));
        if (!inLabel && !inId && !inCategory && !inKeys) return false;
      }
      return true;
    });
  }, [allCommands, category, filterMode, search, conflictKeys]);

  // Group by category for display (only when "all" category selected and no search)
  const grouped = useMemo(() => {
    if (category !== "all" || search.trim()) {
      return [{ label: null, commands: filtered }];
    }
    const map = new Map<ShortcutCategory, ShortcutCommand[]>();
    for (const cmd of filtered) {
      if (!map.has(cmd.category)) map.set(cmd.category, []);
      map.get(cmd.category)!.push(cmd);
    }
    return Array.from(map.entries()).map(([label, commands]) => ({ label, commands }));
  }, [filtered, category, search]);

  const filterBtnCls = (active: boolean) =>
    [
      "h-6 rounded px-2 text-[10px] font-semibold transition-colors",
      active
        ? "bg-[rgba(114,215,215,0.12)] text-[rgba(114,215,215,0.9)] border border-[rgba(114,215,215,0.25)]"
        : "text-[rgba(255,255,255,0.38)] hover:text-[rgba(255,255,255,0.6)] hover:bg-[rgba(255,255,255,0.04)] border border-transparent",
    ].join(" ");

  const catBtnCls = (active: boolean) =>
    [
      "shrink-0 h-6 rounded px-2 text-[10px] font-semibold whitespace-nowrap transition-colors",
      active
        ? "bg-[rgba(114,215,215,0.12)] text-[rgba(114,215,215,0.9)] border border-[rgba(114,215,215,0.25)]"
        : "text-[rgba(255,255,255,0.38)] hover:text-[rgba(255,255,255,0.6)] hover:bg-[rgba(255,255,255,0.04)] border border-transparent",
    ].join(" ");

  return (
    <div className="flex flex-col gap-0 -mx-5 -mt-1">
      {/* ── Search + filter bar ── */}
      <div className="border-b border-[rgba(255,255,255,0.05)] px-4 pt-2 pb-2.5 space-y-2">
        {/* Search input */}
        <label
          className="flex h-7 items-center gap-2 rounded-lg border bg-[#13161c] px-2.5 transition-colors focus-within:border-[rgba(114,215,215,0.4)]"
          style={{ borderColor: "rgba(255,255,255,0.07)" }}
        >
          <Search size={11} className="shrink-0 text-[rgba(255,255,255,0.28)]" />
          <input
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search commands or shortcuts…"
            className="min-w-0 flex-1 bg-transparent text-[11px] text-[rgba(255,255,255,0.78)] outline-none placeholder:text-[rgba(255,255,255,0.25)]"
          />
          {search && (
            <button
              type="button"
              onClick={() => setSearch("")}
              className="shrink-0 text-[rgba(255,255,255,0.28)] hover:text-[rgba(255,255,255,0.6)]"
            >
              ✕
            </button>
          )}
        </label>

        {/* Mode filter pills */}
        <div className="flex items-center gap-1.5">
          {(["all", "unassigned", "conflicts"] as FilterMode[]).map((mode) => (
            <button
              key={mode}
              type="button"
              onClick={() => setFilterMode(mode)}
              className={filterBtnCls(filterMode === mode)}
            >
              {mode === "all" ? "All" : mode === "unassigned" ? "Unassigned" : `Conflicts${conflicts.length > 0 ? ` (${conflicts.length})` : ""}`}
            </button>
          ))}
          <div className="flex-1" />
          <span className="text-[9px] tabular-nums text-[rgba(255,255,255,0.22)]">
            {filtered.length} of {allCommands.length}
          </span>
        </div>

        {/* Category scroll row */}
        <div className="flex items-center gap-1 overflow-x-auto scrollbar-none pb-0.5">
          <button
            type="button"
            onClick={() => setCategory("all")}
            className={catBtnCls(category === "all")}
          >
            All categories
          </button>
          {ALL_SHORTCUT_CATEGORIES.map((cat) => (
            <button
              key={cat}
              type="button"
              onClick={() => setCategory(cat)}
              className={catBtnCls(category === cat)}
            >
              {cat}
            </button>
          ))}
        </div>
      </div>

      {/* ── Shortcut list ── */}
      <div className="px-4 pb-4">
        {filtered.length === 0 ? (
          <div className="flex items-center justify-center py-12 text-[11px] text-[rgba(255,255,255,0.25)]">
            No commands match
          </div>
        ) : (
          grouped.map(({ label, commands }, gi) => (
            <div key={label ?? gi}>
              {label && <CategoryHeader label={label} count={commands.length} />}
              {commands.map((cmd) => (
                <ShortcutRow
                  key={cmd.id}
                  cmd={cmd}
                  hasConflict={conflictKeys.has(cmd.id)}
                />
              ))}
            </div>
          ))
        )}
      </div>
    </div>
  );
}
