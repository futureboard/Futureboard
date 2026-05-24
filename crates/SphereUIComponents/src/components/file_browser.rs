//! File browser state + lazy directory index for the left sidebar.
//!
//! Mirrors the Electron browser's IPC model: a long-lived process owns the
//! filesystem access and the UI reads from a cache. Here:
//!
//! * `FileBrowserState` holds **state only** — expand/select sets, drive
//!   roots, and an [`IndexCache`] of previously-loaded directory listings.
//! * `visible_nodes()` never touches the filesystem. It walks the cache
//!   and emits "Loading…" / "Error" placeholder rows for paths the
//!   indexer has not finished (or failed to) load.
//! * The actual `std::fs::read_dir` work runs on `gpui::BackgroundExecutor`
//!   from [`crate::layout`] — the UI thread is never blocked.
//!
//! Realtime / audio rules:
//! * filesystem reads never happen in render/layout.
//! * audio paths must never touch this module.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileEntryKind {
    Folder,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileBrowserEntry {
    pub name: String,
    pub path: PathBuf,
    pub kind: FileEntryKind,
    /// Lowercased extension (without the dot), or empty for folders.
    pub extension: String,
}

impl FileBrowserEntry {
    pub fn is_audio(&self) -> bool {
        matches!(
            self.extension.as_str(),
            "wav" | "mp3" | "flac" | "ogg" | "aiff" | "aif"
        )
    }

    pub fn is_midi(&self) -> bool {
        matches!(self.extension.as_str(), "mid" | "midi")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserNodeKind {
    Section,
    Folder,
    File,
}

#[derive(Debug, Clone)]
pub struct BrowserRootSection {
    pub id: String,
    pub label: String,
    pub root_path: Option<PathBuf>,
    pub kind: BrowserNodeKind,
}

#[derive(Debug, Clone)]
pub struct BrowserVisibleNode {
    pub id: String,
    pub label: String,
    pub path: Option<PathBuf>,
    pub kind: BrowserNodeKind,
    pub depth: usize,
    pub extension: String,
    pub expandable: bool,
    pub expanded: bool,
    pub selected: bool,
    pub error: Option<String>,
}

impl BrowserVisibleNode {
    pub fn is_audio(&self) -> bool {
        matches!(
            self.extension.as_str(),
            "wav" | "mp3" | "flac" | "ogg" | "aiff" | "aif"
        )
    }

    pub fn is_midi(&self) -> bool {
        matches!(self.extension.as_str(), "mid" | "midi")
    }
}

/// Lazy directory cache populated by the background indexer.
///
/// Each known path is in exactly one of three states:
///   * `loaded` — entries cached, render shows children.
///   * `loading` — request in flight, render shows a Loading row.
///   * `errors` — last load failed, render shows an Error row.
/// Paths in none of these maps are treated as "never asked" and the
/// layout will dispatch a load when the user expands them.
#[derive(Debug, Clone, Default)]
pub struct IndexCache {
    pub loaded: HashMap<PathBuf, IndexedDir>,
    pub loading: HashSet<PathBuf>,
    pub errors: HashMap<PathBuf, String>,
}

#[derive(Debug, Clone)]
pub struct IndexedDir {
    pub entries: Vec<FileBrowserEntry>,
    pub loaded_at: Instant,
}

#[derive(Debug, Clone)]
pub struct FileBrowserState {
    pub selected: Option<PathBuf>,
    /// Expand state, keyed exclusively by path. Drive roots and folders
    /// alike live here so toggle / lookup never disagree.
    pub expanded_paths: HashSet<PathBuf>,
    /// Top-level filesystem roots — drive letters on Windows, `/` and
    /// per-volume mounts on Unix-like systems. Enumerated cheaply at
    /// startup (Win32 `GetLogicalDrives` bitmask on Windows).
    pub root_drives: Vec<BrowserRootSection>,
    /// Lazy index of expanded directories. Render reads from here; the
    /// layout owns the background loader that populates it.
    pub index: IndexCache,
}

impl Default for FileBrowserState {
    fn default() -> Self {
        // Startup must not touch the disk beyond enumerating mounted
        // drives. Everything else loads on demand when the user expands
        // a node.
        Self {
            selected: None,
            expanded_paths: HashSet::new(),
            root_drives: default_root_drives(),
            index: IndexCache::default(),
        }
    }
}

impl FileBrowserState {
    pub fn select(&mut self, path: PathBuf) {
        self.selected = Some(path);
    }

    /// Toggle expand state for a node. Path is the source of truth — the
    /// `node_id` argument is kept for callsite ergonomics but unused.
    /// Returns `true` if the path was just expanded (caller should ensure
    /// it is indexed); `false` if it was collapsed.
    pub fn toggle_node(&mut self, _node_id: &str, path: Option<&Path>) -> bool {
        let Some(path) = path else {
            return false;
        };
        let path = path.to_path_buf();
        if self.expanded_paths.contains(&path) {
            self.expanded_paths.remove(&path);
            false
        } else {
            self.expanded_paths.insert(path);
            true
        }
    }

    pub fn is_expanded_node(&self, _node_id: &str, path: Option<&Path>) -> bool {
        path.is_some_and(|p| self.expanded_paths.contains(p))
    }

    /// Apply a finished directory listing from the background indexer.
    pub fn apply_loaded(&mut self, path: PathBuf, entries: Vec<FileBrowserEntry>) {
        self.index.loading.remove(&path);
        self.index.errors.remove(&path);
        self.index.loaded.insert(
            path,
            IndexedDir {
                entries,
                loaded_at: Instant::now(),
            },
        );
    }

    /// Apply a finished directory listing failure from the background indexer.
    pub fn apply_error(&mut self, path: PathBuf, error: String) {
        self.index.loading.remove(&path);
        self.index.loaded.remove(&path);
        self.index.errors.insert(path, error);
    }

    /// Mark a path as having an in-flight load request.
    pub fn mark_loading(&mut self, path: PathBuf) {
        self.index.errors.remove(&path);
        self.index.loading.insert(path);
    }

    /// Returns the list of currently-expanded paths whose contents have
    /// neither been loaded nor are loading. The caller dispatches
    /// background loads for each.
    pub fn paths_needing_load(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for path in &self.expanded_paths {
            if self.index.loaded.contains_key(path) || self.index.loading.contains(path) {
                continue;
            }
            out.push(path.clone());
        }
        out
    }

    /// Flatten the tree into one row per visible node, driven entirely by
    /// the in-memory cache. No filesystem access happens here.
    pub fn visible_nodes(&self) -> Vec<BrowserVisibleNode> {
        let mut nodes = Vec::new();
        for drive in &self.root_drives {
            let drive_path = match drive.root_path.as_ref() {
                Some(p) => p,
                None => continue,
            };
            let expanded = self.expanded_paths.contains(drive_path);
            let selected = self.selected.as_deref() == Some(drive_path.as_path());
            nodes.push(BrowserVisibleNode {
                id: drive.id.clone(),
                label: drive.label.clone(),
                path: Some(drive_path.clone()),
                kind: BrowserNodeKind::Folder,
                depth: 0,
                extension: String::new(),
                expandable: true,
                expanded,
                selected,
                error: None,
            });

            if expanded {
                self.append_cached_dir(drive_path, 1, &mut nodes);
            }
        }
        nodes
    }

    fn append_cached_dir(&self, dir: &Path, depth: usize, nodes: &mut Vec<BrowserVisibleNode>) {
        // Cache state, in priority order: error > loading > loaded > unknown.
        if let Some(err) = self.index.errors.get(dir) {
            nodes.push(placeholder_row(dir, depth, err.clone(), true));
            return;
        }
        if self.index.loading.contains(dir) {
            nodes.push(placeholder_row(dir, depth, "Loading…".to_string(), false));
            return;
        }
        let Some(indexed) = self.index.loaded.get(dir) else {
            // Path is expanded but not yet asked for — the layout's
            // `paths_needing_load` sweep will pick it up next render.
            nodes.push(placeholder_row(dir, depth, "Loading…".to_string(), false));
            return;
        };

        for entry in &indexed.entries {
            let is_folder = entry.kind == FileEntryKind::Folder;
            let expanded = is_folder && self.expanded_paths.contains(&entry.path);
            let selected = self.selected.as_deref() == Some(entry.path.as_path());
            nodes.push(BrowserVisibleNode {
                id: entry.path.to_string_lossy().to_string(),
                label: entry.name.clone(),
                path: Some(entry.path.clone()),
                kind: if is_folder {
                    BrowserNodeKind::Folder
                } else {
                    BrowserNodeKind::File
                },
                depth,
                extension: entry.extension.clone(),
                expandable: is_folder,
                expanded,
                selected,
                error: None,
            });

            if expanded {
                self.append_cached_dir(&entry.path, depth + 1, nodes);
            }
        }
    }
}

fn placeholder_row(dir: &Path, depth: usize, label: String, is_error: bool) -> BrowserVisibleNode {
    BrowserVisibleNode {
        id: format!(
            "{}:{}",
            if is_error { "error" } else { "loading" },
            dir.display()
        ),
        label,
        path: None,
        kind: BrowserNodeKind::File,
        depth,
        extension: String::new(),
        expandable: false,
        expanded: false,
        selected: false,
        error: if is_error {
            Some("Cannot read folder".to_string())
        } else {
            None
        },
    }
}

/// Resolve a sensible starting directory: user Music dir, then home, then cwd.
pub fn default_directory() -> PathBuf {
    if let Some(p) = dirs::audio_dir() {
        if p.is_dir() {
            return p;
        }
    }
    if let Some(p) = dirs::home_dir() {
        if p.is_dir() {
            return p;
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Enumerate top-level filesystem roots — drive letters on Windows,
/// `/` plus mounted volumes on macOS / Linux. Each root maps to a single
/// `BrowserRootSection` whose `root_path` is a real directory.
fn default_root_drives() -> Vec<BrowserRootSection> {
    let mut out = Vec::new();
    for path in enumerate_filesystem_roots() {
        let label = drive_label(&path);
        let id = format!("root:{}", path.display());
        out.push(BrowserRootSection {
            id,
            label,
            root_path: Some(path),
            kind: BrowserNodeKind::Folder,
        });
    }
    out
}

#[cfg(target_os = "windows")]
fn enumerate_filesystem_roots() -> Vec<PathBuf> {
    // Use kernel32 `GetLogicalDrives` — returns a 26-bit mask of mounted
    // drive letters in microseconds. The previous probe loop called
    // `Path::is_dir("A:\\")` for every letter, which causes Windows to
    // spin up empty optical / floppy / disconnected removable drives and
    // hang the UI for tens of seconds at startup.
    extern "system" {
        fn GetLogicalDrives() -> u32;
    }
    let mask = unsafe { GetLogicalDrives() };
    let mut drives = Vec::new();
    for i in 0u32..26 {
        if mask & (1 << i) != 0 {
            let letter = (b'A' + i as u8) as char;
            drives.push(PathBuf::from(format!("{}:\\", letter)));
        }
    }
    if drives.is_empty() {
        if let Some(home) = dirs::home_dir() {
            drives.push(home);
        }
    }
    drives
}

#[cfg(target_os = "macos")]
fn enumerate_filesystem_roots() -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from("/")];
    let volumes = PathBuf::from("/Volumes");
    if let Ok(read) = std::fs::read_dir(&volumes) {
        for entry in read.flatten() {
            let p = entry.path();
            if p.is_dir() {
                roots.push(p);
            }
        }
    }
    if let Some(home) = dirs::home_dir() {
        roots.push(home);
    }
    roots
}

#[cfg(all(unix, not(target_os = "macos")))]
fn enumerate_filesystem_roots() -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from("/")];
    for parent in ["/media", "/mnt", "/run/media"] {
        if let Ok(read) = std::fs::read_dir(parent) {
            for entry in read.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    roots.push(p);
                }
            }
        }
    }
    if let Some(home) = dirs::home_dir() {
        roots.push(home);
    }
    roots
}

/// Friendly label for a drive root. On Windows that's `C:` etc.; on
/// Unix-likes the leaf folder name (or `/` for the root itself).
fn drive_label(path: &Path) -> String {
    #[cfg(target_os = "windows")]
    {
        let s = path.to_string_lossy();
        // `C:\` → `C:`. Trim trailing separator(s) for display.
        let trimmed = s.trim_end_matches(|c| c == '\\' || c == '/');
        if trimmed.is_empty() {
            return s.into_owned();
        }
        return trimmed.to_string();
    }
    #[cfg(not(target_os = "windows"))]
    {
        if path == Path::new("/") {
            return "/".to_string();
        }
        path.file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| path.to_string_lossy().into_owned())
    }
}

/// Read a directory into a sorted entry list. Folders first, then files,
/// each block alphabetical (case-insensitive). Hidden entries (`.foo`) are
/// skipped — they almost never matter inside a DAW browser.
pub fn read_directory(path: &Path) -> (Vec<FileBrowserEntry>, Option<String>) {
    let read = match std::fs::read_dir(path) {
        Ok(r) => r,
        Err(e) => return (Vec::new(), Some(e.to_string())),
    };

    let mut folders: Vec<FileBrowserEntry> = Vec::new();
    let mut files: Vec<FileBrowserEntry> = Vec::new();

    for ent in read.flatten() {
        let p = ent.path();
        let name = match p.file_name().and_then(|s| s.to_str()) {
            Some(n) if !n.starts_with('.') => n.to_string(),
            _ => continue,
        };
        let meta = match ent.file_type() {
            Ok(m) => m,
            Err(_) => continue,
        };

        if meta.is_dir() {
            folders.push(FileBrowserEntry {
                name,
                path: p,
                kind: FileEntryKind::Folder,
                extension: String::new(),
            });
        } else if meta.is_file() {
            let ext = p
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_default();
            files.push(FileBrowserEntry {
                name,
                path: p,
                kind: FileEntryKind::File,
                extension: ext,
            });
        }
    }

    folders.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    folders.extend(files);
    (folders, None)
}
