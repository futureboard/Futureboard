mod app_chrome;
pub mod add_track_dialog;
mod bottom_panel;
pub mod context_menu;
pub mod fader;
pub mod file_browser;
mod icon;
mod icon_button;
pub mod knob;
pub mod menu_dropdown;
pub mod mixer_panel;
pub mod panel;
pub mod project_switcher;
pub mod project_wizard;
mod sidebar;
pub mod text_input;
pub mod slider;
mod status_bar;
pub mod timeline;

pub use app_chrome::{app_chrome, ProjectChromeState, TransportChromeState};
pub use add_track_dialog::{
    add_track_dialog, AddTrackDialogCallbacks, AddTrackDialogState, AddTrackKind,
};
pub use bottom_panel::{bottom_panel, BottomPanelResizeDrag, BottomPanelState, BottomTab};
pub use fader::fader;
pub use icon::icon;
pub use icon_button::icon_button;
pub use knob::knob;
pub use mixer_panel::mixer_panel;
pub use panel::right_panel;
pub use project_wizard::{
    project_wizard, ProjectTemplate, ProjectWizardCallbacks, ProjectWizardResult,
    ProjectWizardState,
};
pub use sidebar::sidebar;
pub use slider::slider;
pub use status_bar::status_bar;
pub use text_input::{text_field, TextInputAction, TextInputState};
