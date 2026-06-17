//! Top-level application route and project-lifecycle state machine (Part G).
//!
//! Today the Welcome screen and the Studio workspace are separate native
//! windows, so [`StudioRoute`] names which surface the app is presenting rather
//! than gating a single window. [`AppMode`] is the authoritative app-level gate
//! that decides whether [`crate::layout::StudioLayout`] may exist. [`ProjectState`]
//! is the lifecycle state of the project loaded in a Studio workspace, and
//! drives the window title so it always reflects reality (no ambiguous
//! "Welcome but also loaded a project" state).

use std::path::PathBuf;

use gpui::Global;

/// App-level surface gate. [`StudioLayout`] must only be mounted when the mode
/// is [`AppMode::Studio`] and the session install status is [`SessionInstallStatus::Ready`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppMode {
    #[default]
    Welcome,
    /// Pre-studio project decode/install transaction in progress.
    LoadingSession,
    /// Studio workspace is (or should be) the active surface.
    Studio,
    /// Last project open failed; the app stays alive (Welcome or restored studio).
    LoadFailed,
}

impl AppMode {
    pub fn label(self) -> &'static str {
        match self {
            AppMode::Welcome => "Welcome",
            AppMode::LoadingSession => "LoadingSession",
            AppMode::Studio => "Studio",
            AppMode::LoadFailed => "LoadFailed",
        }
    }

    pub fn allows_studio_mount(self) -> bool {
        matches!(self, AppMode::Studio)
    }
}

/// Whether a Studio workspace session is safe for UI components to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SessionInstallStatus {
    #[default]
    None,
    Loading,
    Ready,
    Failed,
}

impl SessionInstallStatus {
    pub fn is_ready(self) -> bool {
        matches!(self, SessionInstallStatus::Ready)
    }

    pub fn is_failed(self) -> bool {
        matches!(self, SessionInstallStatus::Failed)
    }
}

/// Process-wide app session gate — updated by the native shell when switching
/// between Welcome, Loading Session, and Studio surfaces.
#[derive(Debug, Default)]
pub struct AppSessionGate {
    pub mode: AppMode,
}

impl Global for AppSessionGate {}

/// Which top-level surface the app is presenting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StudioRoute {
    /// The Welcome / start surface.
    Welcome,
    /// The Studio workspace (arrangement, mixer, editors).
    StudioWorkspace,
}

impl StudioRoute {
    /// Resolve the startup route from the "Show start screen on launch"
    /// preference. When disabled, boot straight into an (unsaved) workspace.
    pub fn from_show_welcome(show_welcome: bool) -> Self {
        if show_welcome {
            StudioRoute::Welcome
        } else {
            StudioRoute::StudioWorkspace
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            StudioRoute::Welcome => "Welcome",
            StudioRoute::StudioWorkspace => "StudioWorkspace",
        }
    }
}

/// Lifecycle state of the project in a Studio workspace.
///
/// The dirty bit is tracked separately (on the project switcher) so a
/// [`ProjectState::SavedProject`] can still be shown as "Unsaved" after edits;
/// pass it to [`ProjectState::status_label`] / [`ProjectState::window_title`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ProjectState {
    /// No project/workspace loaded (e.g. on the Welcome surface).
    #[default]
    NoProject,
    /// A blank in-memory workspace with no folder on disk yet.
    UnsavedWorkspace,
    /// A project backed by a file on disk.
    SavedProject { path: PathBuf },
    /// A project file is being decoded/applied.
    Loading,
    /// The last load/save failed; carries a human-readable message.
    Error(String),
}

impl ProjectState {
    pub fn is_saved(&self) -> bool {
        matches!(self, ProjectState::SavedProject { .. })
    }

    pub fn saved_path(&self) -> Option<&PathBuf> {
        match self {
            ProjectState::SavedProject { path } => Some(path),
            _ => None,
        }
    }

    /// Short status word for the in-app header chip.
    pub fn status_label(&self, dirty: bool) -> &'static str {
        match self {
            ProjectState::NoProject => "No project",
            ProjectState::UnsavedWorkspace => "Unsaved",
            ProjectState::SavedProject { .. } => {
                if dirty {
                    "Unsaved"
                } else {
                    "Saved"
                }
            }
            ProjectState::Loading => "Loading",
            ProjectState::Error(_) => "Error",
        }
    }

    /// OS window title for the current state, e.g. `"Untitled Project — Unsaved"`
    /// or `"My Song — Saved"`.
    pub fn window_title(&self, name: &str, dirty: bool) -> String {
        match self {
            ProjectState::NoProject => "Futureboard Studio".to_string(),
            ProjectState::Loading => format!("{name} — Loading…"),
            ProjectState::Error(msg) => format!("{name} — Error: {msg}"),
            ProjectState::UnsavedWorkspace => format!("{name} — Unsaved"),
            ProjectState::SavedProject { .. } => {
                if dirty {
                    format!("{name} — Unsaved")
                } else {
                    format!("{name} — Saved")
                }
            }
        }
    }
}
