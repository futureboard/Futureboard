//! User manual location + open helpers.
//!
//! The manual lives as `.mdx` files under `docs/manual/` in the repository and
//! is shipped alongside the executable in installed builds. This module locates
//! the manual root at runtime and opens a section with the OS default handler.
//!
//! Wired to the **Help ▸ Documentation** (`help:documentation`) and
//! **Help ▸ Quick Start Guide** (`help:quick-start`) menu actions.
//!
//! Pure path resolution + a single `std::process::Command` spawn to hand the
//! file to the platform shell — control-path only, never touched from audio.

use std::path::{Path, PathBuf};

/// Repository-relative location of the manual, from the workspace root.
pub const REPO_RELATIVE_DIR: &str = "docs/manual";

/// Landing / table-of-contents page.
pub const INDEX_FILE: &str = "index.mdx";

/// Section opened by **Help ▸ Quick Start Guide**.
pub const QUICK_START_FILE: &str = "getting-started.mdx";

/// Returns the first existing manual root directory, searching installed and
/// development layouts in priority order. Pure filesystem probing, no spawn.
pub fn manual_root() -> Option<PathBuf> {
    candidate_roots()
        .into_iter()
        .find(|candidate| candidate.join(INDEX_FILE).is_file())
}

/// Resolves a specific manual section file (e.g. [`INDEX_FILE`]) if the manual
/// is present, falling back to the manual root directory when the named file is
/// missing.
pub fn manual_section(file: &str) -> Option<PathBuf> {
    let root = manual_root()?;
    let section = root.join(file);
    if section.is_file() {
        Some(section)
    } else {
        Some(root)
    }
}

/// Opens a manual section with the OS default handler. Returns `false` when the
/// manual could not be located so callers can surface an alternative (e.g. an
/// online docs link) instead of failing silently.
pub fn open_section(file: &str) -> bool {
    match manual_section(file) {
        Some(path) => {
            open_path(&path);
            true
        }
        None => false,
    }
}

/// Candidate manual roots, most-specific first.
fn candidate_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    // 1. Installed layout: `<exe dir>/docs/manual/`.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.join(REPO_RELATIVE_DIR));
            // Some installers place resources one level up from the binary.
            if let Some(parent) = dir.parent() {
                roots.push(parent.join(REPO_RELATIVE_DIR));
            }
        }
    }

    // 2. Development layout: workspace root relative to this crate.
    //    `<crate>/../../docs/manual/`.
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    if let Some(workspace) = crate_dir.parent().and_then(|p| p.parent()) {
        roots.push(workspace.join(REPO_RELATIVE_DIR));
    }

    roots
}

/// Hands a path to the platform shell to open with its default application.
fn open_path(path: &Path) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer").arg(path).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_ships_in_repo() {
        // The development workspace must always contain the manual index so the
        // Documentation menu item has something to open.
        let root = manual_root().expect("manual root should resolve in the repo");
        assert!(root.join(INDEX_FILE).is_file());
        assert!(root.join(QUICK_START_FILE).is_file());
    }
}
