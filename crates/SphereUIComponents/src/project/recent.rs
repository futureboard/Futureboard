use crate::paths::FutureboardPaths;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const MAX_RECENT: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentProject {
    pub name: String,
    /// Project root folder (parent of the `.fbproj` file).
    #[serde(default)]
    pub folder_path: Option<PathBuf>,
    /// Absolute path to the `.fbproj` file.
    pub path: PathBuf,
    pub last_opened_at: u64,
    /// Set to true when the file no longer exists at `path`.
    pub missing: bool,
}

/// Persistent list of recently opened projects, backed by a JSON config file.
/// Stored at `<AppData>/Futureboard Studio/recent.json`.
#[derive(Debug, Default)]
pub struct RecentProjectsStore {
    entries: Vec<RecentProject>,
    config_path: PathBuf,
}

impl RecentProjectsStore {
    fn config_path() -> PathBuf {
        FutureboardPaths::resolve().recent_file
    }

    /// Loads from disk, creating an empty store if the file doesn't exist.
    pub fn load() -> Self {
        let config_path = Self::config_path();
        let entries = if config_path.exists() {
            fs::read_to_string(&config_path)
                .ok()
                .and_then(|s| serde_json::from_str::<Vec<RecentProject>>(&s).ok())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        Self {
            entries,
            config_path,
        }
    }

    pub fn entries(&self) -> &[RecentProject] {
        &self.entries
    }

    /// Adds or updates an entry, then saves to disk.
    pub fn push(&mut self, name: impl Into<String>, path: PathBuf, last_opened_at: u64) {
        let path_clone = path.clone();
        let folder_path = path.parent().map(PathBuf::from);
        self.entries.retain(|e| e.path != path_clone);
        self.entries.insert(
            0,
            RecentProject {
                name: name.into(),
                folder_path,
                path,
                last_opened_at,
                missing: false,
            },
        );
        self.entries.truncate(MAX_RECENT);
        let _ = self.save();
    }

    /// Marks entries whose files no longer exist, then saves.
    pub fn refresh_missing(&mut self) {
        for entry in &mut self.entries {
            entry.missing = !entry.path.exists();
        }
        let _ = self.save();
    }

    /// Removes all entries and saves.
    pub fn clear(&mut self) {
        self.entries.clear();
        let _ = self.save();
    }

    /// Removes a single entry by path.
    pub fn remove(&mut self, path: &Path) {
        self.entries.retain(|e| e.path != path);
        let _ = self.save();
    }

    fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.entries)?;
        fs::write(&self.config_path, json)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_records_folder_and_project_file_paths() {
        let mut store = RecentProjectsStore {
            entries: Vec::new(),
            config_path: PathBuf::from("recent-test.json"),
        };
        let project_file = PathBuf::from("/tmp/Beat Demo/Beat Demo.fbproj");
        store.push("Beat Demo", project_file.clone(), 42);
        let entry = store.entries().first().expect("recent entry");
        assert_eq!(entry.name, "Beat Demo");
        assert_eq!(entry.path, project_file);
        assert_eq!(
            entry.folder_path.as_deref(),
            Some(Path::new("/tmp/Beat Demo"))
        );
        assert_eq!(entry.last_opened_at, 42);
    }
}
