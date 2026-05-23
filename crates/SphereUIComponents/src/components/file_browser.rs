//! File browser state and filesystem walker for the left sidebar.
//!
//! Mirrors the Electron browser's behavior: a navigable directory listing
//! with audio-aware filtering. No globals — the layout owns one
//! `FileBrowserState` and passes it to the sidebar each render.
//!
//! Realtime / audio rules:
//! * filesystem scans are best-effort and run on the UI thread when the user
//!   navigates. They must not be triggered from audio paths.
//! * we never block the audio engine on a `read_dir` call — this module is
//!   pure UI state.

use std::path::{Path, PathBuf};

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
        matches!(self.extension.as_str(), "wav" | "mp3" | "flac" | "ogg" | "aiff" | "aif")
    }

    pub fn is_midi(&self) -> bool {
        matches!(self.extension.as_str(), "mid" | "midi")
    }
}

#[derive(Debug, Clone)]
pub struct FileBrowserState {
    pub current_dir: PathBuf,
    pub entries: Vec<FileBrowserEntry>,
    pub selected: Option<PathBuf>,
    pub error: Option<String>,
}

impl Default for FileBrowserState {
    fn default() -> Self {
        let dir = default_directory();
        let (entries, error) = read_directory(&dir);
        Self {
            current_dir: dir,
            entries,
            selected: None,
            error,
        }
    }
}

impl FileBrowserState {
    /// Navigate to `path` (must be an existing directory). Refreshes entries.
    pub fn navigate_to(&mut self, path: impl Into<PathBuf>) {
        let target = path.into();
        if !target.is_dir() {
            return;
        }
        let (entries, error) = read_directory(&target);
        self.current_dir = target;
        self.entries = entries;
        self.error = error;
        self.selected = None;
    }

    /// Move up one directory if a parent exists.
    pub fn navigate_up(&mut self) {
        if let Some(parent) = self.current_dir.parent().map(|p| p.to_path_buf()) {
            self.navigate_to(parent);
        }
    }

    pub fn refresh(&mut self) {
        let (entries, error) = read_directory(&self.current_dir);
        self.entries = entries;
        self.error = error;
    }

    pub fn select(&mut self, path: PathBuf) {
        self.selected = Some(path);
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

/// Read a directory into a sorted entry list. Folders first, then files,
/// each block alphabetical (case-insensitive). Hidden entries (`.foo`) are
/// skipped — they almost never matter inside a DAW browser.
fn read_directory(path: &Path) -> (Vec<FileBrowserEntry>, Option<String>) {
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
