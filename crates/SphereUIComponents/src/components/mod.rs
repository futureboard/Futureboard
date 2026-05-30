pub mod add_track_dialog;
mod app_chrome;
mod bottom_panel;
pub mod combo_box;
pub mod context_menu;
pub mod controls;
pub mod fader;
pub mod file_browser;
mod icon;
mod icon_button;
pub mod knob;
pub mod menu_bar;
pub mod message_box_dialog;
pub mod menu_dropdown;
pub mod mixer_panel;
mod mixer_window;
pub(crate) use mixer_window::external_mixer_debug;
pub mod panel;
pub mod project_switcher;
pub mod project_wizard;
pub mod piano_roll;
pub mod plugin_editor_window;
pub mod plugin_manager;
pub mod plugin_picker;
pub mod settings_layout;
pub mod settings_dialog;
mod sidebar;
pub mod slider;
mod status_bar;
pub mod text_input;
pub mod timeline;
pub mod title_bar;


pub use add_track_dialog::{
    add_track_dialog, open_add_track_window, AddTrackDialogCallbacks, AddTrackDialogState,
    AddTrackKind, AddTrackWindow,
};
pub use app_chrome::{
    app_chrome, bpm_debug_enabled, bpm_drag_sensitivity, BpmChangeCb, BpmDragCb, BpmDragSample,
    PanelChromeState, ProjectChromeState, TransportChromeState, BPM_DRAG_DEADZONE_PX, BPM_MAX,
    BPM_MIN,
};
pub use bottom_panel::{bottom_panel, BottomPanelResizeDrag, BottomPanelState, BottomTab};
pub use combo_box::{combo_box_menu, combo_box_trigger, ComboBoxOption};
pub use controls::{
    fb_button, fb_field_label, fb_form_row, fb_section_label, fb_segmented_button,
    fb_stepper_button, FbButtonKind,
};
pub use fader::fader;
pub use icon::icon;
pub use icon_button::icon_button;
pub use knob::knob;
pub use menu_bar::{menu_bar, menu_label_button};
pub use message_box_dialog::{
    open_message_box_window, unsaved_changes_options, MessageBoxKind, MessageBoxOptions,
    MessageBoxResult, MessageBoxWindow, MESSAGE_BOX_WIDTH,
};
pub use mixer_panel::mixer_panel;
pub use piano_roll::PianoRoll;
pub use mixer_window::{open_mixer_window, MixerSnapshot, MixerWindow};
pub use panel::right_panel;
pub use plugin_manager::{
    open_plugin_manager_window, FilterCounts, PluginManagerDialogState, PluginManagerWindow,
    SidebarFilter, SortDir, SortKey,
};
pub use project_wizard::{
    open_project_wizard_window, ProjectCreateCallback, ProjectTemplate, ProjectWizardResult,
    ProjectWizardState, ProjectWizardWindow,
};
pub use sidebar::sidebar;
pub use slider::slider;
pub use status_bar::status_bar;
pub use text_input::{text_field, TextInputAction, TextInputState};
pub use settings_dialog::{
    settings_dialog, HardwareCombo, OnSettingUpdate, SettingsDialogCallbacks, SettingsDialogState,
    SettingsTab, SettingsWindow, open_settings_window,
};

