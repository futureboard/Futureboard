use std::path::PathBuf;

use super::{new_id, now_secs};

/// Canonical in-memory model for the project currently loaded in a studio
/// workspace. All UI chrome, save/open commands, and engine sync should read
/// from this struct (via [`StudioLayout::sync_project_session_to_workspace`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSession {
    pub id: String,
    pub name: String,
    pub folder_path: Option<PathBuf>,
    pub project_file_path: Option<PathBuf>,
    pub is_untitled: bool,
    pub is_dirty: bool,
    pub created_at: u64,
    pub modified_at: u64,
}

impl Default for ProjectSession {
    fn default() -> Self {
        Self::untitled()
    }
}

impl ProjectSession {
    pub fn fresh_id() -> String {
        new_id()
    }

    pub fn untitled() -> Self {
        let now = now_secs();
        Self {
            id: new_id(),
            name: "Untitled Project".to_string(),
            folder_path: None,
            project_file_path: None,
            is_untitled: true,
            is_dirty: false,
            created_at: now,
            modified_at: now,
        }
    }

    pub fn bind_saved(
        &mut self,
        id: String,
        name: String,
        folder_path: Option<PathBuf>,
        project_file_path: PathBuf,
        created_at: u64,
        modified_at: u64,
    ) {
        self.id = id;
        self.name = name;
        self.folder_path = folder_path;
        self.project_file_path = Some(project_file_path);
        self.is_untitled = false;
        self.is_dirty = false;
        self.created_at = created_at;
        self.modified_at = modified_at;
    }

    pub fn bind_untitled(&mut self, name: impl Into<String>, dirty: bool) {
        let now = now_secs();
        self.id = new_id();
        self.name = name.into();
        self.folder_path = None;
        self.project_file_path = None;
        self.is_untitled = true;
        self.is_dirty = dirty;
        self.created_at = now;
        self.modified_at = now;
    }

    /// Titlebar / window chrome display name.
    pub fn display_name(&self) -> &str {
        if self.is_untitled {
            "Untitled Project"
        } else {
            &self.name
        }
    }

    pub fn needs_save_as(&self) -> bool {
        self.is_untitled || self.project_file_path.is_none()
    }

    pub fn mark_dirty(&mut self) {
        self.is_dirty = true;
        self.modified_at = now_secs();
    }

    pub fn mark_clean(&mut self, modified_at: Option<u64>) {
        self.is_dirty = false;
        if let Some(ts) = modified_at {
            self.modified_at = ts;
        }
    }

    pub fn subtitle(&self) -> &'static str {
        if self.is_dirty {
            "Unsaved changes"
        } else if self.is_untitled {
            "New project"
        } else {
            "Saved"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_name_untitled() {
        let session = ProjectSession::untitled();
        assert_eq!(session.display_name(), "Untitled Project");
        assert!(session.needs_save_as());
    }

    #[test]
    fn bind_saved_clears_untitled() {
        let mut session = ProjectSession::untitled();
        let path = PathBuf::from("/tmp/Test Song/Test Song.fbproj");
        session.bind_saved(
            "id-1".to_string(),
            "Test Song".to_string(),
            Some(PathBuf::from("/tmp/Test Song")),
            path.clone(),
            1,
            2,
        );
        assert_eq!(session.name, "Test Song");
        assert_eq!(session.project_file_path.as_ref(), Some(&path));
        assert!(!session.is_untitled);
        assert!(!session.is_dirty);
        assert!(!session.needs_save_as());
        assert_eq!(session.display_name(), "Test Song");
    }

    #[test]
    fn save_as_from_untitled_then_direct_save() {
        let mut session = ProjectSession::untitled();
        assert!(session.needs_save_as());
        session.bind_saved(
            "id-2".to_string(),
            "My Project".to_string(),
            Some(PathBuf::from("/tmp/My Project")),
            PathBuf::from("/tmp/My Project/My Project.fbproj"),
            10,
            10,
        );
        assert_eq!(session.name, "My Project");
        assert!(!session.is_untitled);
        assert!(!session.needs_save_as());
    }

    #[test]
    fn dirty_state_round_trip() {
        let mut session = ProjectSession::untitled();
        session.bind_saved(
            "id-3".to_string(),
            "Beat Demo".to_string(),
            Some(PathBuf::from("/tmp/Beat Demo")),
            PathBuf::from("/tmp/Beat Demo/Beat Demo.fbproj"),
            1,
            1,
        );
        session.mark_dirty();
        assert!(session.is_dirty);
        session.mark_clean(Some(99));
        assert!(!session.is_dirty);
        assert_eq!(session.modified_at, 99);
    }
}
