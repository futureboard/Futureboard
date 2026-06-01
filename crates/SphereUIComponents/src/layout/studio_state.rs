use std::path::PathBuf;

use crate::overlay::OverlayAnchor;

/// Top-menu open state. `open_menu_id` is the manifest menu id currently
/// showing its dropdown; `anchor` is the label rect used to position the panel.
#[derive(Debug, Clone, Default)]
pub struct MenuBarUiState {
    pub open_menu_id: Option<String>,
    pub anchor: OverlayAnchor,
    /// Nested submenu ids open underneath the root dropdown. `path[0]` is
    /// the submenu open in the root panel, `path[1]` in *that* submenu's
    /// panel, etc.
    pub submenu_path: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum OpenPopover {
    Context {
        target: ContextTarget,
        x: f32,
        y: f32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TextMenuTarget {
    ProjectSwitcherSearch,
    BrowserSearch,
    PluginPickerSearch,
    /// The Inspector's track-name edit field.
    InspectorName,
    /// The Inspector's clip-name edit field.
    InspectorClipName,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct TextContextMenu {
    pub(super) target: TextMenuTarget,
    pub(super) x: f32,
    pub(super) y: f32,
}

#[derive(Debug, Clone)]
pub enum ContextTarget {
    TimelineEmpty,
    Track(String),
    Clip(String),
    Browser(Option<PathBuf>),
    Mixer(String),
}

/// Which docked studio panels are visible in the main window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StudioPanelVisibility {
    pub browser: bool,
    pub inspector: bool,
    pub mixer_docked: bool,
}

impl Default for StudioPanelVisibility {
    fn default() -> Self {
        Self {
            browser: true,
            inspector: true,
            mixer_docked: true,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum TransportCommand {
    PlayPause,
    Stop,
    ReturnToStart,
    ToggleLoop,
    ToggleMetronome,
    ToggleFollowPlayhead,
    Record,
}
