// Compact tabbed sidebar: PRESET | IR | NAM, backed by real files under
// Documents/Futureboard Studio/Rodhareist/{Presets, IRs, NAMs} via the
// native file bridge (list/read/write postMessages). Listings rebuild
// wholesale on tab switch and after every write — never diffed.
//
// Presets and NAMs are text, so they come back through `readFile`. IRs are
// binary `.wav`: the page sends only the file name (`postLoadIr`) and native
// reads the bytes itself — see `instanceBridge.ts`.

import { useEffect, useRef, useState } from "react";
import { postLoadIr, subscribeIrLoadResult } from "../bridge";
import {
  onNativeMessage,
  postListFiles,
  postReadFile,
  postWriteFile,
  type FileEntry,
  type FileKind,
} from "../instanceBridge";
import {
  parsePresetFile,
  seedFactoryPresets,
  type PresetFile,
} from "../presetFiles";
import type { RigSnapshot } from "../Editor";

type PresetBrowserProps = {
  currentPresetId: string;
  modifiedIds?: ReadonlySet<string>;
  /** Apply a successfully parsed preset file to the rig. */
  onLoadPresetFile: (file: PresetFile) => void;
  /** Build the save payload for a user preset name; null cancels. */
  buildSavePayload: (name: string) => { fileName: string; content: string } | null;
  /** Editor's factorySnapshot — used once to seed an empty Presets folder. */
  buildFactorySnapshot: (id: string) => RigSnapshot | null;
  /** Route a `.nam` file's text into the amp slot's NAM engine. */
  onLoadNamFile: (name: string, json: string) => void;
  /** Called after an IR loads, so the editor can switch the cabinet slot to
   * the convolution engine — loading and selecting are separate steps. */
  onIrLoaded?: (name: string) => void;
};

const TABS: { kind: FileKind; label: string }[] = [
  { kind: "presets", label: "Preset" },
  { kind: "irs", label: "IR" },
  { kind: "nams", label: "NAM" },
];

/** `"01A Twin Sparkle.json"` → `{ pid: "01A", pname: "Twin Sparkle" }`. */
function displayParts(fileName: string): { pid: string; pname: string } {
  const stem = fileName.replace(/\.[^.]+$/, "");
  const space = stem.indexOf(" ");
  if (space > 0 && space <= 4) {
    return { pid: stem.slice(0, space), pname: stem.slice(space + 1) };
  }
  return { pid: "•", pname: stem };
}

export function PresetBrowser({
  currentPresetId,
  modifiedIds,
  onLoadPresetFile,
  buildSavePayload,
  buildFactorySnapshot,
  onLoadNamFile,
  onIrLoaded,
}: PresetBrowserProps) {
  const [tab, setTab] = useState<FileKind>("presets");
  const [query, setQuery] = useState("");
  const [lists, setLists] = useState<Partial<Record<FileKind, FileEntry[]>>>({});
  const [saving, setSaving] = useState(false);
  const [saveName, setSaveName] = useState("");
  const [status, setStatus] = useState<string | null>(null);
  /** File name of the IR the DSP currently has loaded, if any. */
  const [loadedIr, setLoadedIr] = useState<string | null>(null);
  const seededRef = useRef(false);
  const pendingSeedWrites = useRef(0);

  // One native-message subscription drives everything: listings, read
  // results (preset load / NAM load), and write acks (refresh + status).
  useEffect(
    () =>
      onNativeMessage((msg) => {
        if (msg.type === "futureboard.fileList") {
          setLists((prev) => ({ ...prev, [msg.kind]: msg.files }));
          // First-run factory seeding: empty Presets folder → write the
          // factory set once, then re-list when the last ack arrives.
          if (
            msg.kind === "presets" &&
            msg.files.length === 0 &&
            !seededRef.current
          ) {
            seededRef.current = true;
            pendingSeedWrites.current = seedFactoryPresets(
              buildFactorySnapshot,
              (fileName, content) => postWriteFile("presets", fileName, content),
            );
            if (pendingSeedWrites.current > 0) {
              setStatus("Creating factory presets…");
            }
          }
        } else if (msg.type === "futureboard.fileWritten") {
          if (pendingSeedWrites.current > 0) {
            pendingSeedWrites.current -= 1;
            if (pendingSeedWrites.current === 0) {
              setStatus(null);
              postListFiles("presets");
            }
          } else if (msg.kind === "presets") {
            setStatus(msg.ok ? null : `Save failed: ${msg.error ?? "unknown"}`);
            postListFiles("presets");
          }
        } else if (msg.type === "futureboard.fileContent") {
          if (!msg.ok || typeof msg.content !== "string") {
            setStatus(`Read failed: ${msg.error ?? "unknown"}`);
            return;
          }
          if (msg.kind === "presets") {
            const parsed = parsePresetFile(msg.content);
            if (parsed) {
              setStatus(null);
              onLoadPresetFile(parsed);
            } else {
              setStatus(`Not a Rodhareist preset: ${msg.fileName}`);
            }
          } else if (msg.kind === "nams") {
            onLoadNamFile(msg.fileName.replace(/\.nam$/i, ""), msg.content);
          }
        }
      }),
    // Stable callbacks come from Editor's useCallback wrappers.
    [buildFactorySnapshot, onLoadPresetFile, onLoadNamFile],
  );

  // IR loads report back on their own channel (the bytes never came through
  // the page, so there is no `fileContent` message to hang this off).
  useEffect(
    () =>
      subscribeIrLoadResult((result) => {
        if (!result.ok) {
          setLoadedIr(null);
          setStatus(`IR failed: ${result.error ?? "unknown"}`);
          return;
        }
        setLoadedIr(result.name);
        const detail = [
          result.stereo ? "stereo" : "mono",
          `${result.frames} frames`,
          result.truncated ? "truncated" : null,
        ]
          .filter(Boolean)
          .join(" · ");
        setStatus(`IR loaded: ${detail}`);
        onIrLoaded?.(result.name);
      }),
    [onIrLoaded],
  );

  // Initial + per-tab listing.
  useEffect(() => {
    postListFiles(tab);
  }, [tab]);

  const entries = (lists[tab] ?? []).filter((f) =>
    f.fileName.toLowerCase().includes(query.trim().toLowerCase()),
  );

  const commitSave = () => {
    const name = saveName.trim();
    if (!name) {
      setSaving(false);
      return;
    }
    const payload = buildSavePayload(name);
    if (payload) {
      postWriteFile("presets", payload.fileName, payload.content);
      setStatus("Saving…");
    }
    setSaving(false);
    setSaveName("");
  };

  return (
    <aside className="browser">
      <div className="browser-tabs" role="tablist" aria-label="Plugin files">
        {TABS.map((t) => (
          <button
            key={t.kind}
            type="button"
            role="tab"
            aria-selected={tab === t.kind}
            className={`browser-tab${tab === t.kind ? " active" : ""}`}
            onClick={() => setTab(t.kind)}
          >
            {t.label}
          </button>
        ))}
      </div>

      <div className="search-wrap">
        <input
          type="text"
          className="search"
          placeholder="Search…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          aria-label={`Search ${tab}`}
        />
      </div>

      <div className="preset-list" role="listbox">
        {entries.length === 0 && (
          <div className="browser-empty">
            {tab === "presets" && "No presets yet"}
            {tab === "irs" &&
              "Drop .wav IRs into Documents/Futureboard Studio/Rodhareist/IRs"}
            {tab === "nams" &&
              "Drop .nam captures into Documents/Futureboard Studio/Rodhareist/NAMs"}
          </div>
        )}
        {entries.map((f) => {
          const { pid, pname } = displayParts(f.fileName);
          const active =
            (tab === "presets" && pid === currentPresetId) ||
            (tab === "irs" && f.fileName === loadedIr);
          const dirty = tab === "presets" && !!modifiedIds?.has(pid);
          return (
            <button
              key={f.fileName}
              type="button"
              role="option"
              aria-selected={active}
              className={`preset-item${active ? " active" : ""}`}
              title={f.fileName}
              onClick={() => {
                if (tab === "presets") postReadFile("presets", f.fileName);
                else if (tab === "nams") postReadFile("nams", f.fileName);
                else if (tab === "irs") {
                  setStatus(`Loading ${f.fileName}…`);
                  postLoadIr(f.fileName);
                }
              }}
            >
              <span className="dot" />
              <span className="pid">{pid}</span>
              <span className="pname">
                {pname}
                {dirty ? " *" : ""}
              </span>
            </button>
          );
        })}
      </div>

      {tab === "presets" && (
        <div className="browser-save">
          {saving ? (
            <input
              type="text"
              className="search"
              autoFocus
              placeholder="Preset name…"
              value={saveName}
              onChange={(e) => setSaveName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") commitSave();
                if (e.key === "Escape") {
                  setSaving(false);
                  setSaveName("");
                }
              }}
              onBlur={() => setSaving(false)}
              aria-label="New preset name"
            />
          ) : (
            <button
              type="button"
              className="browser-save-btn"
              onClick={() => setSaving(true)}
            >
              ＋ Save preset
            </button>
          )}
        </div>
      )}

      {status && <div className="browser-note">{status}</div>}
    </aside>
  );
}
