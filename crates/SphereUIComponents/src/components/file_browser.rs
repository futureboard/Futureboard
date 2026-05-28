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
use std::fs;
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
    /// Active project folder. Pinned to the top of the browser roots when set.
    pub project_folder: Option<PathBuf>,
    /// Current search filter query.
    pub filter: String,
    /// Cached flattened visible nodes representing the current state.
    pub visible_nodes: Vec<BrowserVisibleNode>,
}

impl Default for FileBrowserState {
    fn default() -> Self {
        let mut state = Self {
            selected: None,
            expanded_paths: HashSet::new(),
            root_drives: default_root_drives(),
            index: IndexCache::default(),
            project_folder: None,
            filter: String::new(),
            visible_nodes: Vec::new(),
        };
        state.update_visible_nodes();
        state
    }
}

impl FileBrowserState {
    pub fn select(&mut self, path: PathBuf) {
        self.selected = Some(path);
        self.update_visible_nodes();
    }

    /// Toggle expand state for a node. Path is the source of truth.
    /// Returns `true` if the path was just expanded (caller should ensure
    /// it is indexed); `false` if it was collapsed.
    pub fn toggle_node(&mut self, _node_id: &str, path: Option<&Path>) -> bool {
        let Some(path) = path else {
            return false;
        };
        let path = path.to_path_buf();
        let expanded = if self.expanded_paths.contains(&path) {
            self.expanded_paths.remove(&path);
            false
        } else {
            self.expanded_paths.insert(path);
            true
        };
        self.update_visible_nodes();
        expanded
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
        self.update_visible_nodes();
    }

    /// Apply a finished directory listing failure from the background indexer.
    pub fn apply_error(&mut self, path: PathBuf, error: String) {
        self.index.loading.remove(&path);
        self.index.loaded.remove(&path);
        self.index.errors.insert(path, error);
        self.update_visible_nodes();
    }

    /// Mark a path as having an in-flight load request.
    pub fn mark_loading(&mut self, path: PathBuf) {
        self.index.errors.remove(&path);
        self.index.loading.insert(path);
        self.update_visible_nodes();
    }

    /// Set active project folder and refresh roots.
    pub fn set_project_folder(&mut self, folder: Option<PathBuf>) {
        self.project_folder = folder;
        self.update_visible_nodes();
    }

    /// Set search filter query.
    pub fn set_filter(&mut self, filter: &str) {
        self.filter = filter.to_string();
        self.update_visible_nodes();
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

    pub fn visible_node_count(&self) -> usize {
        self.visible_nodes.len()
    }

    /// Flatten the tree into one row per visible node, driven entirely by
    /// the in-memory cache. No filesystem access happens here.
    pub fn visible_nodes(&self) -> Vec<BrowserVisibleNode> {
        self.visible_nodes.clone()
    }

    /// Re-calculate the cached visible flattened nodes.
    pub fn update_visible_nodes(&mut self) {
        let mut nodes = Vec::new();
        let dirs = resolve_standard_dirs();

        // 1. Current Project Folder (pinned at top when open)
        if let Some(proj_folder) = &self.project_folder {
            let expanded = self.expanded_paths.contains(proj_folder);
            let selected = self.selected.as_ref() == Some(proj_folder);
            let label = proj_folder
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Current Project".to_string());
            nodes.push(BrowserVisibleNode {
                id: format!("root:project"),
                label: format!("PROJECT: {}", label),
                path: Some(proj_folder.clone()),
                kind: BrowserNodeKind::Folder,
                depth: 0,
                extension: String::new(),
                expandable: true,
                expanded,
                selected,
                error: None,
            });
            if expanded {
                self.append_cached_dir(proj_folder, 1, &mut nodes);
            }
        }

        // 2. Audio Files
        if let Some(audio_path) = dirs.get("audio_files") {
            let expanded = self.expanded_paths.contains(audio_path);
            let selected = self.selected.as_ref() == Some(audio_path);
            nodes.push(BrowserVisibleNode {
                id: "root:audio_files".to_string(),
                label: "Audio Files".to_string(),
                path: Some(audio_path.clone()),
                kind: BrowserNodeKind::Folder,
                depth: 0,
                extension: String::new(),
                expandable: true,
                expanded,
                selected,
                error: None,
            });
            if expanded {
                self.append_cached_dir(audio_path, 1, &mut nodes);
            }
        }

        // 3. Plug-ins
        if let Some(plugins_path) = dirs.get("plugins") {
            let expanded = self.expanded_paths.contains(plugins_path);
            let selected = self.selected.as_ref() == Some(plugins_path);
            nodes.push(BrowserVisibleNode {
                id: "root:plugins".to_string(),
                label: "Plug-ins".to_string(),
                path: Some(plugins_path.clone()),
                kind: BrowserNodeKind::Folder,
                depth: 0,
                extension: String::new(),
                expandable: true,
                expanded,
                selected,
                error: None,
            });
            if expanded {
                self.append_cached_dir(plugins_path, 1, &mut nodes);
            }
        }

        // 4. Instruments
        if let Some(plugins_path) = dirs.get("plugins") {
            let instruments_path = plugins_path.join("Instruments");
            let expanded = self.expanded_paths.contains(&instruments_path);
            let selected = self.selected.as_ref() == Some(&instruments_path);
            nodes.push(BrowserVisibleNode {
                id: "root:instruments".to_string(),
                label: "Instruments".to_string(),
                path: Some(instruments_path.clone()),
                kind: BrowserNodeKind::Folder,
                depth: 0,
                extension: String::new(),
                expandable: true,
                expanded,
                selected,
                error: None,
            });
            if expanded {
                self.append_cached_dir(&instruments_path, 1, &mut nodes);
            }
        }

        // 5. Projects
        if let Some(projects_path) = dirs.get("projects") {
            let expanded = self.expanded_paths.contains(projects_path);
            let selected = self.selected.as_ref() == Some(projects_path);
            nodes.push(BrowserVisibleNode {
                id: "root:projects".to_string(),
                label: "Projects".to_string(),
                path: Some(projects_path.clone()),
                kind: BrowserNodeKind::Folder,
                depth: 0,
                extension: String::new(),
                expandable: true,
                expanded,
                selected,
                error: None,
            });
            if expanded {
                self.append_cached_dir(projects_path, 1, &mut nodes);
            }
        }

        // 6. Samples
        if let Some(samples_path) = dirs.get("samples") {
            let expanded = self.expanded_paths.contains(samples_path);
            let selected = self.selected.as_ref() == Some(samples_path);
            nodes.push(BrowserVisibleNode {
                id: "root:samples".to_string(),
                label: "Samples".to_string(),
                path: Some(samples_path.clone()),
                kind: BrowserNodeKind::Folder,
                depth: 0,
                extension: String::new(),
                expandable: true,
                expanded,
                selected,
                error: None,
            });
            if expanded {
                self.append_cached_dir(samples_path, 1, &mut nodes);
            }
        }

        // 7. User Library
        if let Some(user_lib_path) = dirs.get("user_library") {
            let expanded = self.expanded_paths.contains(user_lib_path);
            let selected = self.selected.as_ref() == Some(user_lib_path);
            nodes.push(BrowserVisibleNode {
                id: "root:user_library".to_string(),
                label: "User Library".to_string(),
                path: Some(user_lib_path.clone()),
                kind: BrowserNodeKind::Folder,
                depth: 0,
                extension: String::new(),
                expandable: true,
                expanded,
                selected,
                error: None,
            });
            if expanded {
                self.append_cached_dir(user_lib_path, 1, &mut nodes);
            }
        }

        // 8. Logical drives (fallback at bottom only when no project is open)
        if self.project_folder.is_none() {
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
        }

        // 9. Apply Search Filter
        if !self.filter.is_empty() {
            nodes = self.filter_flattened_nodes(nodes);
        }

        self.visible_nodes = nodes;
    }

    fn append_cached_dir(&self, dir: &Path, depth: usize, nodes: &mut Vec<BrowserVisibleNode>) {
        if let Some(err) = self.index.errors.get(dir) {
            nodes.push(placeholder_row(dir, depth, err.clone(), true));
            return;
        }
        if self.index.loading.contains(dir) {
            nodes.push(placeholder_row(dir, depth, "Loading…".to_string(), false));
            return;
        }
        let Some(indexed) = self.index.loaded.get(dir) else {
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

    fn filter_flattened_nodes(&self, nodes: Vec<BrowserVisibleNode>) -> Vec<BrowserVisibleNode> {
        let query = self.filter.to_lowercase();
        let mut kept = vec![false; nodes.len()];

        for i in (0..nodes.len()).rev() {
            let node = &nodes[i];
            let matches_query = node.label.to_lowercase().contains(&query);
            if matches_query {
                kept[i] = true;
                continue;
            }

            if node.expandable {
                let mut has_kept_descendant = false;
                for j in (i + 1)..nodes.len() {
                    let child = &nodes[j];
                    if child.depth <= node.depth {
                        break;
                    }
                    if kept[j] {
                        has_kept_descendant = true;
                        break;
                    }
                }
                if has_kept_descendant {
                    kept[i] = true;
                }
            }
        }

        nodes
            .into_iter()
            .enumerate()
            .filter(|(i, _)| kept[*i])
            .map(|(_, n)| n)
            .collect()
    }

    // Keyboard Helpers

    pub fn select_next(&mut self) {
        if self.visible_nodes.is_empty() {
            return;
        }
        let next_path = if let Some(current_selected) = &self.selected {
            if let Some(idx) = self
                .visible_nodes
                .iter()
                .position(|n| n.path.as_ref() == Some(current_selected))
            {
                if idx + 1 < self.visible_nodes.len() {
                    self.visible_nodes[idx + 1].path.clone()
                } else {
                    self.visible_nodes[idx].path.clone()
                }
            } else {
                self.visible_nodes[0].path.clone()
            }
        } else {
            self.visible_nodes[0].path.clone()
        };
        self.selected = next_path;
        self.update_visible_nodes();
    }

    pub fn select_previous(&mut self) {
        if self.visible_nodes.is_empty() {
            return;
        }
        let prev_path = if let Some(current_selected) = &self.selected {
            if let Some(idx) = self
                .visible_nodes
                .iter()
                .position(|n| n.path.as_ref() == Some(current_selected))
            {
                if idx > 0 {
                    self.visible_nodes[idx - 1].path.clone()
                } else {
                    self.visible_nodes[0].path.clone()
                }
            } else {
                self.visible_nodes[0].path.clone()
            }
        } else {
            self.visible_nodes[0].path.clone()
        };
        self.selected = prev_path;
        self.update_visible_nodes();
    }

    pub fn expand_selected(&mut self) {
        if let Some(current_selected) = &self.selected {
            if self.expanded_paths.contains(current_selected) {
                if let Some(idx) = self
                    .visible_nodes
                    .iter()
                    .position(|n| n.path.as_ref() == Some(current_selected))
                {
                    if idx + 1 < self.visible_nodes.len()
                        && self.visible_nodes[idx + 1].depth > self.visible_nodes[idx].depth
                    {
                        self.selected = self.visible_nodes[idx + 1].path.clone();
                    }
                }
            } else {
                self.expanded_paths.insert(current_selected.clone());
                self.update_visible_nodes();
            }
        }
    }

    pub fn collapse_selected_or_parent(&mut self) {
        if let Some(current_selected) = &self.selected {
            if self.expanded_paths.contains(current_selected) {
                self.expanded_paths.remove(current_selected);
                self.update_visible_nodes();
            } else {
                if let Some(idx) = self
                    .visible_nodes
                    .iter()
                    .position(|n| n.path.as_ref() == Some(current_selected))
                {
                    let current_depth = self.visible_nodes[idx].depth;
                    if current_depth > 0 {
                        for i in (0..idx).rev() {
                            if self.visible_nodes[i].depth < current_depth {
                                self.selected = self.visible_nodes[i].path.clone();
                                break;
                            }
                        }
                    }
                }
                self.update_visible_nodes();
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

/// Resolve sensible starting directory: user Music dir, then home, then cwd.
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

/// Enumerate logical drive letters on Windows, `/` plus mounted volumes on unix.
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

fn drive_label(path: &Path) -> String {
    #[cfg(target_os = "windows")]
    {
        let s = path.to_string_lossy();
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

/// Resolves standard folder paths for the file browser sidebar.
///
/// Delegates to [`crate::paths::FutureboardPaths`] — the centralized path
/// system. Directory creation is handled once at app startup via
/// `FutureboardPaths::ensure_user_dirs()`, not on every browser update.
pub fn resolve_standard_dirs() -> HashMap<String, PathBuf> {
    crate::paths::FutureboardPaths::resolve().standard_dirs()
}

/// Read directory into sorted entry lists. Treat .vst3 folders as files/plugins.
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
            let ext = p
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_default();
            if ext == "vst3" {
                files.push(FileBrowserEntry {
                    name,
                    path: p,
                    kind: FileEntryKind::File,
                    extension: ext,
                });
            } else {
                folders.push(FileBrowserEntry {
                    name,
                    path: p,
                    kind: FileEntryKind::Folder,
                    extension: String::new(),
                });
            }
        } else if meta.is_file() {
            let ext = p
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_default();
            if ext == "pst" {
                let display = read_pst_plugin_name(&p).unwrap_or_else(|| {
                    p.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or(&name)
                        .to_string()
                });
                files.push(FileBrowserEntry {
                    name: display,
                    path: p,
                    kind: FileEntryKind::File,
                    extension: ext,
                });
                continue;
            }
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

fn read_pst_plugin_name(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    if bytes.len() < 24 {
        return None;
    }
    if &bytes[0..5] != b"FBPST" {
        return None;
    }
    let meta_len = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
    if bytes.len() < 24 + meta_len {
        return None;
    }
    let meta = &bytes[24..24 + meta_len];
    let value: serde_json::Value = serde_json::from_slice(meta).ok()?;
    value
        .get("pluginMetadata")
        .and_then(|m| m.get("name"))
        .and_then(|n| n.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
