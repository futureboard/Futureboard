import { Check, Cloud, FolderOpen, Plus, Search, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { platform } from "../../platform";
import { useProjectStore } from "../../store/projectStore";
import { useRecentProjectsStore, isSavedRecentProject, type RecentProject } from "../../store/recentProjectsStore";
import { useUIStore } from "../../store/uiStore";
import { showToast } from "../ui/Toast";
import { guardUnsavedProject, loadOpenedProject, openProjectFromPath } from "../../utils/projectLifecycle";
import { openProjectWizardWindow } from "../../utils/dialogWindows";

function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <div className="px-2 pb-0.5 pt-1.5 text-[9px] font-semibold uppercase tracking-widest text-daw-faint">
      {children}
    </div>
  );
}

function ProjectRow({
  name,
  subtext,
  active = false,
  onClick,
  onRemove,
}: {
  name: string;
  subtext?: string;
  active?: boolean;
  onClick?: () => void;
  onRemove?: () => void;
}) {
  return (
    <div className="group relative flex items-center gap-1.5 rounded px-2 py-1 transition-colors hover:bg-daw-surface-high">
      <span className="flex w-4 shrink-0 items-center justify-center text-daw-accent">
        {active ? <Check size={11} /> : null}
      </span>
      <button
        type="button"
        onClick={onClick}
        disabled={active}
        className="flex min-w-0 flex-1 flex-col items-start gap-0 disabled:cursor-default"
      >
        <span className="min-w-0 max-w-full truncate text-left text-[11px] font-medium text-daw-text">{name}</span>
        {subtext && (
          <span className="min-w-0 max-w-full truncate text-left text-[9px] text-daw-faint">{subtext}</span>
        )}
      </button>
      {onRemove && (
        <button
          type="button"
          onClick={(e) => { e.stopPropagation(); onRemove(); }}
          className="hidden shrink-0 rounded p-0.5 text-daw-faint transition-colors hover:text-daw-text group-hover:flex"
          title="Remove from recent"
        >
          <X size={10} />
        </button>
      )}
    </div>
  );
}

function ActionRow({
  icon: Icon,
  label,
  shortcut,
  disabled = false,
  onClick,
}: {
  icon: React.ElementType;
  label: string;
  shortcut?: string;
  disabled?: boolean;
  onClick?: () => void;
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      className="flex h-7 w-full items-center gap-2 rounded px-2 text-left transition-colors hover:bg-daw-surface-high disabled:cursor-not-allowed disabled:opacity-35"
    >
      <span className="flex w-4 shrink-0 items-center justify-center text-daw-faint">
        <Icon size={12} />
      </span>
      <span className="min-w-0 flex-1 truncate text-[11px] text-daw-text">{label}</span>
      {shortcut && (
        <span className="shrink-0 text-right text-[10px] text-daw-faint">{shortcut}</span>
      )}
    </button>
  );
}

export function ProjectDropdown({ onClose }: { onClose: () => void }) {
  const { project } = useProjectStore();
  const { saveStatus } = useUIStore();
  const waveformStatus = useProjectStore((s) => s.waveformStatus);
  const { recentProjects, removeRecentProject, clearRecentProjects } = useRecentProjectsStore();
  const [search, setSearch] = useState("");
  const searchRef = useRef<HTMLInputElement>(null);
  const dropdownRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    searchRef.current?.focus();
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (dropdownRef.current && !dropdownRef.current.contains(e.target as Node)) {
        onClose();
      }
    };
    // Defer so the trigger button click doesn't immediately close
    const timer = setTimeout(() => window.addEventListener("mousedown", onDown), 0);
    return () => {
      clearTimeout(timer);
      window.removeEventListener("mousedown", onDown);
    };
  }, [onClose]);

  const missingAssets = project.files.filter((file) => waveformStatus.get(file.id) === "missing" || file.storageProvider === "missing").length;
  const projectRoot = platform.folderProject.getProjectRoot();
  const storageModeLabel = projectRoot
    ? "Folder project"
    : platform.kind === "electron"
      ? "Saved locally"
      : "Browser storage";
  const saveLabel =
    saveStatus === "unsaved" ? "Unsaved changes" :
    saveStatus === "saving" ? "Saving..." :
    saveStatus === "error" ? "Save error" :
    `${storageModeLabel}${missingAssets > 0 ? ` · ${missingAssets} missing asset${missingAssets === 1 ? "" : "s"}` : ""}`;

  const savedRecentProjects = recentProjects.filter((p) =>
    isSavedRecentProject(p) && (platform.kind !== "electron" || p.source === "local"),
  );
  const filteredRecent = savedRecentProjects.filter(
    (p) =>
      p.id !== project.id &&
      p.name.toLowerCase().includes(search.toLowerCase())
  );

  const handleOpenProjectFile = async () => {
    const canContinue = await guardUnsavedProject("open");
    if (!canContinue) return;
    onClose();
    if (platform.kind === "electron") {
      try {
        const opened = await platform.projectStorage.openProject();
        if (opened) {
          await loadOpenedProject(opened);
          showToast(`Opened: ${opened.name}`);
        }
      } catch {
        showToast("Failed to open project.", true);
      }
    } else {
      showToast(
        "Folder projects are available in Futureboard Lite / Electron.",
        true,
      );
    }
  };

  const handleSelectRecent = async (p: RecentProject) => {
    const canContinue = await guardUnsavedProject("switch");
    if (!canContinue) return;
    onClose();
    // Folder project: open via the single canonical open path.
    if (p.storageMode === "folder" && p.projectFilePath && platform.folderProject.isSupported) {
      try {
        const ok = await openProjectFromPath(p.projectFilePath);
        if (ok) {
          useRecentProjectsStore.getState().addRecentProject({
            ...p,
            lastOpenedAt: Date.now(),
          });
          showToast(`Opened: ${p.name}`);
        } else {
          showToast(`Could not open "${p.name}" — file may have moved.`, true);
        }
      } catch {
        showToast(`Failed to open "${p.name}".`, true);
      }
      return;
    }
    // Fallback: just switch name (legacy behaviour)
    useProjectStore.getState().setProjectName(p.name);
    useRecentProjectsStore.getState().addRecentProject({
      ...p,
      lastOpenedAt: Date.now(),
    });
    showToast(`Switched to: ${p.name}`);
  };

  const handleNewProject = () => {
    void guardUnsavedProject("new").then((ok) => {
      if (!ok) return;
      onClose();
      void openProjectWizardWindow();
    });
  };

  const handleClearRecent = () => {
    clearRecentProjects();
    onClose();
  };

  return (
    <div
      ref={dropdownRef}
      className="app-no-drag absolute left-0 top-[calc(100%+4px)] z-[150] w-72 rounded-md border border-daw-border bg-daw-surface shadow-[0_12px_40px_rgba(0,0,0,0.55)]"
    >
      {/* Search */}
      <div className="border-b border-daw-border px-2 py-1.5">
        <div className="flex items-center gap-1.5 rounded bg-daw-sunken px-2 py-1">
          <Search size={11} className="shrink-0 text-daw-faint" />
          <input
            ref={searchRef}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            onKeyDown={(e) => e.stopPropagation()}
            placeholder="Search projects..."
            className="min-w-0 flex-1 bg-transparent text-[11px] text-daw-text outline-none placeholder:text-daw-faint"
          />
          {search && (
            <button
              type="button"
              onClick={() => setSearch("")}
              className="text-daw-faint transition-colors hover:text-daw-text"
            >
              <X size={10} />
            </button>
          )}
        </div>
      </div>

      <div className="max-h-[400px] overflow-y-auto p-1">
        {/* This Window */}
        <SectionLabel>This Window</SectionLabel>
        <ProjectRow name={project.name} subtext={saveLabel} active />

        {/* Recent Projects */}
        {filteredRecent.length > 0 && (
          <>
            <div className="my-1 h-px bg-daw-border" />
            <SectionLabel>Recent Projects</SectionLabel>
            {filteredRecent.map((p) => (
              <ProjectRow
                key={p.id}
                name={p.name}
                subtext={p.projectRoot ?? p.path ?? p.source}
                onClick={() => { void handleSelectRecent(p); }}
                onRemove={() => removeRecentProject(p.id)}
              />
            ))}
          </>
        )}

        {search && filteredRecent.length === 0 && savedRecentProjects.length > 0 && (
          <div className="px-2 py-3 text-center text-[11px] text-daw-faint">
            No projects match &ldquo;{search}&rdquo;
          </div>
        )}

        {/* Actions */}
        <div className="my-1 h-px bg-daw-border" />
        <SectionLabel>Actions</SectionLabel>
        <ActionRow
          icon={FolderOpen}
          label="Open Project File..."
          shortcut="Ctrl+K O"
          onClick={handleOpenProjectFile}
        />
        <ActionRow icon={Plus} label="New Project..." onClick={handleNewProject} />
        <ActionRow icon={Cloud} label="Open Remote Folder" disabled />

        {/* Footer */}
        {savedRecentProjects.length > 0 && (
          <>
            <div className="my-1 h-px bg-daw-border" />
            <button
              type="button"
              onClick={handleClearRecent}
              className="w-full rounded px-3 py-1.5 text-left text-[10px] text-daw-faint transition-colors hover:bg-daw-surface-high hover:text-daw-text"
            >
              Clear Recent Projects
            </button>
          </>
        )}
      </div>
    </div>
  );
}
