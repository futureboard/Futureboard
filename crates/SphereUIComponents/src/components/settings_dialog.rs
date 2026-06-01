use std::sync::Arc;

use gpui::{
    div, px, size, svg, App, AppContext, Bounds, Context, Entity, FocusHandle, InteractiveElement,
    IntoElement, KeyDownEvent, MouseButton, ParentElement, Point, Render,
    StatefulInteractiveElement, Styled, Window, WindowBackgroundAppearance, WindowBounds,
    WindowHandle, WindowKind,
};

use crate::assets;
use crate::components::combo_box::{combo_box_string_menu, combo_box_trigger};
use crate::components::controls::{
    fb_button, fb_segmented_button, fb_stepper_button, FbButtonKind,
};
use crate::components::settings_layout::{
    settings_daw_row, settings_nav_group_header, settings_nav_item, settings_page_header,
    settings_section_card, settings_section_hint, settings_section_title, settings_status_badge,
    settings_value_readout, SETTINGS_CONTENT_PAD, SETTINGS_SIDEBAR_WIDTH, SETTINGS_WINDOW_HEIGHT,
    SETTINGS_WINDOW_WIDTH,
};
use crate::components::slider::slider;
use crate::components::text_input::{
    text_field_with_callbacks, TextInputAction, TextInputCallbacks, TextInputState,
};
use crate::components::timeline::render::list_available_gpu_devices;
use crate::components::title_bar::external_window_titlebar;
use crate::i18n::{I18n, Locale};
use crate::overlay::{
    compute_overlay_position, form_combo_trigger_bounds, refresh_form_anchor, settings_form_column,
    OverlayAnchor, OverlayPlacement, OverlaySize, COMBO_TRIGGER_HEIGHT,
};
use crate::settings::{GpuDevicePreference, RenderMode, SettingsModel, SettingsSchema};
use crate::theme::{self, Colors};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    General,
    Audio,
    Midi,
    Recording,
    Playback,
    Editing,
    Appearance,
    Plugins,
    FilesMedia,
    Shortcuts,
    Performance,
    Advanced,
    About,
}

impl SettingsTab {
    pub fn label_key(self) -> &'static str {
        match self {
            Self::General => "settings.tab.general",
            Self::Audio => "settings.tab.audio",
            Self::Midi => "settings.tab.midi",
            Self::Recording => "settings.tab.recording",
            Self::Playback => "settings.tab.playback",
            Self::Editing => "settings.tab.editing",
            Self::Appearance => "settings.tab.appearance",
            Self::Plugins => "settings.tab.plugins",
            Self::FilesMedia => "settings.tab.files-media",
            Self::Shortcuts => "settings.tab.shortcuts",
            Self::Performance => "settings.tab.performance",
            Self::Advanced => "settings.tab.advanced",
            Self::About => "settings.tab.about",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::General => assets::ICON_FILE_PATH,
            Self::Audio => assets::ICON_MIC_PATH,
            Self::Midi => assets::ICON_LINK_PATH,
            Self::Recording => assets::ICON_CIRCLE_PATH,
            Self::Playback => assets::ICON_PLAY_PATH,
            Self::Editing => assets::ICON_PENCIL_PATH,
            Self::Appearance => assets::ICON_SLIDERS_HORIZONTAL_PATH,
            Self::Plugins => assets::ICON_CPU_PATH,
            Self::FilesMedia => assets::ICON_FOLDER_PATH,
            Self::Shortcuts => assets::ICON_LINK_PATH,
            Self::Performance => assets::ICON_CPU_PATH,
            Self::Advanced => assets::ICON_CLOCK_PATH,
            Self::About => assets::ICON_CIRCLE_DOT_PATH,
        }
    }

    pub fn page_description_key(self) -> &'static str {
        match self {
            Self::General => "settings.tab.general.description",
            Self::Audio => "settings.tab.audio.description",
            Self::Midi => "settings.tab.midi.description",
            Self::Recording => "settings.tab.recording.description",
            Self::Playback => "settings.tab.playback.description",
            Self::Editing => "settings.tab.editing.description",
            Self::Appearance => "settings.tab.appearance.description",
            Self::Plugins => "settings.tab.plugins.description",
            Self::FilesMedia => "settings.tab.files-media.description",
            Self::Shortcuts => "settings.tab.shortcuts.description",
            Self::Performance => "settings.tab.performance.description",
            Self::Advanced => "settings.tab.advanced.description",
            Self::About => "settings.tab.about.description",
        }
    }

    pub fn nav_groups() -> &'static [(&'static str, &'static [Self])] {
        &[
            ("settings.nav.general", &[Self::General]),
            (
                "settings.nav.studio",
                &[Self::Audio, Self::Midi, Self::Recording, Self::Playback],
            ),
            (
                "settings.nav.workflow",
                &[Self::Editing, Self::Plugins, Self::FilesMedia],
            ),
            (
                "settings.nav.interface",
                &[Self::Appearance, Self::Shortcuts],
            ),
            (
                "settings.nav.system",
                &[Self::Performance, Self::Advanced, Self::About],
            ),
        ]
    }

    pub fn all() -> &'static [Self] {
        &[
            Self::General,
            Self::Audio,
            Self::Midi,
            Self::Recording,
            Self::Playback,
            Self::Editing,
            Self::Appearance,
            Self::Plugins,
            Self::FilesMedia,
            Self::Shortcuts,
            Self::Performance,
            Self::Advanced,
            Self::About,
        ]
    }
}

#[derive(Debug, Clone)]
pub struct SettingsDialogState {
    pub is_open: bool,
    pub active_tab: SettingsTab,
    pub search_query: String,
}

impl SettingsDialogState {
    pub fn closed() -> Self {
        Self {
            is_open: false,
            active_tab: SettingsTab::General,
            search_query: String::new(),
        }
    }

    pub fn open() -> Self {
        Self {
            is_open: true,
            active_tab: SettingsTab::General,
            search_query: String::new(),
        }
    }
}

pub type UpdateSettingFn = Arc<dyn Fn(&mut SettingsSchema) + Send + Sync + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardwareCombo {
    AudioDriver,
    InputDevice,
    OutputDevice,
    ClockSource,
    Language,
    AutosaveInterval,
    AutosaveMaxBackups,
    SampleRate,
    BufferSize,
    Renderer,
    GpuDevice,
}

#[derive(Clone)]
pub struct SettingsDialogCallbacks {
    pub on_close: Arc<dyn Fn(&(), &mut Window, &mut App) + 'static>,
    pub on_select_tab: Arc<dyn Fn(&SettingsTab, &mut Window, &mut App) + 'static>,
    pub on_update_setting: Arc<dyn Fn(UpdateSettingFn, &mut Window, &mut App) + 'static>,
    pub open_hardware_combo: Option<HardwareCombo>,
    pub on_toggle_hardware_combo:
        Arc<dyn Fn(HardwareCombo, Option<OverlayAnchor>, &mut Window, &mut App) + 'static>,
}

fn icon(path: &'static str, size: f32, color: gpui::Rgba) -> impl IntoElement {
    svg().path(path).w(px(size)).h(px(size)).text_color(color)
}

fn reveal_path_os(path: &std::path::Path) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer")
            .arg(format!("\"{}\"", path.display()))
            .spawn();
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

fn settings_path_list(paths: &[String]) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .children(paths.iter().enumerate().map(|(idx, path)| {
            let path_string = path.clone();
            div()
                .id(("settings-path-row", idx))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .h(px(30.0))
                        .px(px(9.0))
                        .rounded_md()
                        .border(px(1.0))
                        .border_color(Colors::border_subtle())
                        .bg(Colors::surface_input())
                        .flex()
                        .items_center()
                        .truncate()
                        .text_size(px(10.5))
                        .text_color(Colors::text_secondary())
                        .child(path_string.clone()),
                )
                .child(fb_button(
                    ("settings-path-reveal", idx),
                    "Reveal",
                    FbButtonKind::Default,
                    true,
                    move |_, _w, _cx| reveal_path_os(std::path::Path::new(&path_string)),
                ))
        }))
}

fn hardware_select(
    combo: HardwareCombo,
    trigger_id: &'static str,
    selected: &str,
    open_combo: Option<HardwareCombo>,
    on_toggle: Arc<dyn Fn(HardwareCombo, Option<OverlayAnchor>, &mut Window, &mut App) + 'static>,
) -> impl IntoElement {
    let open = open_combo == Some(combo);
    let toggle = on_toggle.clone();
    div().w_full().child(combo_box_trigger(
        trigger_id,
        selected.to_string(),
        open,
        move |event, window, cx| {
            let layout = settings_form_column(window);
            let bounds = form_combo_trigger_bounds(layout, event, COMBO_TRIGGER_HEIGHT);
            let anchor = if open {
                None
            } else {
                Some(OverlayAnchor { bounds })
            };
            toggle(combo, anchor, window, cx);
        },
    ))
}

pub fn fb_checkbox(
    id: impl Into<gpui::ElementId>,
    checked: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .w(px(12.0))
        .h(px(12.0))
        .rounded_sm()
        .border(px(1.0))
        .border_color(Colors::border_default())
        .bg(if checked {
            Colors::accent_primary()
        } else {
            Colors::surface_input()
        })
        .cursor(gpui::CursorStyle::PointingHand)
        .on_click(on_click)
        .children(if checked {
            Some(
                svg()
                    .path(assets::ICON_CHECK_PATH)
                    .w(px(8.0))
                    .h(px(8.0))
                    .text_color(Colors::text_inverse()),
            )
        } else {
            None
        })
}

fn settings_header(title: &'static str, _icon_path: &'static str) -> impl IntoElement {
    settings_section_title(title)
}

fn settings_i18n_header(i18n: I18n, key: &str, _icon_path: &'static str) -> impl IntoElement {
    settings_section_title(i18n.tr(key))
}

fn locale_label(i18n: I18n, locale: Locale) -> String {
    i18n.tr(locale.language_key())
}

fn selected_locale_label(i18n: I18n, language_code: &str) -> String {
    locale_label(i18n, Locale::from_code(language_code))
}

/// Performance > Rendering section. Renderer and GPU Device choices are
/// "restart required" — applied at next launch by `WgpuTimelineRenderer`
/// construction. We deliberately don't hot-swap the renderer at runtime
/// to avoid mid-session GPU device churn.
fn performance_section(
    schema: &SettingsSchema,
    open_combo: Option<HardwareCombo>,
    on_toggle: Arc<dyn Fn(HardwareCombo, Option<OverlayAnchor>, &mut Window, &mut App) + 'static>,
) -> impl IntoElement {
    let render_mode = schema.performance.render_mode;
    let gpu_pref = schema.performance.gpu_device.clone();
    // Enumerate once for label/status; the dropdown re-enumerates on open
    // to stay current with hot-pluggable eGPUs / driver changes.
    let detected = list_available_gpu_devices();
    let detected_count = detected.len();
    let enumeration_failed_unexpectedly = false; // catch_unwind path inside list_available_gpu_devices already returns Vec::new on panic; treat empty as "no GPU" rather than failure.

    let renderer_label = render_mode.label().to_string();
    let renderer_row = hardware_select(
        HardwareCombo::Renderer,
        "settings-performance-renderer-trigger",
        &renderer_label,
        open_combo,
        on_toggle.clone(),
    );

    let gpu_device_label = match &gpu_pref {
        GpuDevicePreference::Auto => "Auto".to_string(),
        GpuDevicePreference::DeviceId(id) => detected
            .iter()
            .find(|d| &d.id == id)
            .map(|d| d.name.clone())
            .unwrap_or_else(|| "Auto".to_string()),
    };
    let gpu_device_row = hardware_select(
        HardwareCombo::GpuDevice,
        "settings-performance-gpu-device-trigger",
        &gpu_device_label,
        open_combo,
        on_toggle,
    );

    let (status_text, status_color) = match (render_mode, detected_count) {
        (RenderMode::CpuRender, _) => (
            "CPU Render active (GPUI paint fallback).".to_string(),
            Colors::text_secondary(),
        ),
        (RenderMode::GpuAcceleration, 0) => (
            "No GPU adapter detected. CPU Render fallback will be used.".to_string(),
            Colors::status_warning(),
        ),
        (RenderMode::GpuAcceleration, n) => (
            format!("GPU Acceleration ready — {n} adapter(s) detected."),
            Colors::status_success(),
        ),
    };

    let mut card = settings_section_card()
        .child(settings_section_title("Rendering"))
        .child(settings_section_hint(
            "Choose how the timeline is drawn. GPU Acceleration uses WGPU when available; CPU Render forces the GPUI paint fallback (best compatibility).",
        ))
        .child(settings_daw_row("Renderer *", renderer_row))
        .child(settings_daw_row("GPU Device *", gpu_device_row))
        .child(settings_daw_row(
            "Status",
            div()
                .text_size(px(10.5))
                .text_color(status_color)
                .child(status_text),
        ));

    if enumeration_failed_unexpectedly {
        card = card.child(
            div()
                .pt(px(4.0))
                .text_size(px(10.0))
                .text_color(Colors::status_warning())
                .child("GPU enumeration failed. CPU Render fallback is available."),
        );
    }

    card.child(
        div()
            .pt(px(8.0))
            .text_size(px(10.0))
            .text_color(Colors::text_faint())
            .child("* Restart Futureboard Studio to apply this change."),
    )
}

fn tab_matches_search(
    tab: SettingsTab,
    query: &str,
    is_match: &dyn Fn(&str, &[&str]) -> bool,
) -> bool {
    if query.is_empty() {
        return true;
    }
    match tab {
        SettingsTab::General => {
            is_match("Language", &["language"])
                || is_match("Start screen", &["wizard", "start"])
                || is_match("Autosave", &["autosave", "backup"])
                || is_match("Tempo", &["tempo", "bpm"])
                || is_match("Sample Rate", &["sample", "rate", "hz"])
                || is_match("Buffer", &["buffer", "latency"])
        }
        SettingsTab::Audio => {
            is_match("Audio Driver", &["driver", "wasapi", "backend"])
                || is_match("Input Device", &["input", "microphone"])
                || is_match("Output Device", &["output", "speakers"])
        }
        SettingsTab::Midi => {
            is_match("MIDI", &["midi", "port", "keyboard"])
                || is_match("Clock", &["clock", "sync", "ltc"])
        }
        SettingsTab::Appearance => {
            is_match("Theme", &["theme"])
                || is_match("UI Scale", &["scale"])
                || is_match("Grid", &["grid", "timeline"])
                || is_match("Mixer", &["mixer", "meter"])
        }
        SettingsTab::Editing => {
            is_match("Zoom", &["mouse", "zoom"])
                || is_match("Snap", &["snap", "grid"])
                || is_match("Undo", &["undo", "history"])
        }
        SettingsTab::Recording => {
            is_match("Recording", &["record", "wav", "bit"])
                || is_match("Metronome", &["metronome", "click"])
        }
        SettingsTab::Playback => is_match("Transport", &["transport", "play", "stop"]),
        SettingsTab::Plugins => {
            is_match("VST3", &["vst3", "plugin"])
                || is_match("CLAP", &["clap"])
                || is_match("Scan", &["scan"])
        }
        SettingsTab::FilesMedia => {
            is_match("Projects", &["project", "folder", "path"])
                || is_match("Samples", &["sample", "media"])
        }
        SettingsTab::Shortcuts => is_match("Shortcut", &["key", "command"]),
        SettingsTab::Performance => {
            is_match("Renderer", &["renderer", "gpu", "cpu", "wgpu"])
                || is_match("GPU Device", &["gpu", "device", "adapter"])
                || is_match("Performance", &["cpu", "engine"])
        }
        SettingsTab::Advanced => is_match("Advanced", &["experimental"]),
        SettingsTab::About => is_match("About", &["version"]),
    }
}

fn build_settings_content(
    state: &SettingsDialogState,
    schema: &SettingsSchema,
    callbacks: &SettingsDialogCallbacks,
    _available_inputs: &[String],
    _available_outputs: &[String],
    _available_backends: &[String],
) -> (Vec<gpui::AnyElement>, Vec<gpui::AnyElement>) {
    let i18n = I18n::new(&schema.general.language);
    let query = state.search_query.trim().to_lowercase();
    let is_match = |label: &str, keywords: &[&str]| {
        if query.is_empty() {
            return true;
        }
        let q = query.as_str();
        label.to_lowercase().contains(q) || keywords.iter().any(|k| k.to_lowercase().contains(q))
    };

    let mut sidebar_items: Vec<gpui::AnyElement> = Vec::new();
    let mut nav_index = 0usize;
    for (group_key, tabs) in SettingsTab::nav_groups() {
        let visible_tabs: Vec<SettingsTab> = tabs
            .iter()
            .copied()
            .filter(|tab| tab_matches_search(*tab, query.as_str(), &is_match))
            .collect();
        if visible_tabs.is_empty() {
            continue;
        }
        sidebar_items.push(settings_nav_group_header(i18n.tr(group_key)).into_any_element());
        for tab in visible_tabs {
            let active = state.active_tab == tab && query.is_empty();
            let search_hit = !query.is_empty();
            let cb = callbacks.on_select_tab.clone();
            let idx = nav_index;
            nav_index += 1;
            sidebar_items.push(
                settings_nav_item(
                    ("settings-tab", idx),
                    i18n.tr(tab.label_key()),
                    tab.icon(),
                    active,
                    search_hit,
                    move |window, cx| cb(&tab, window, cx),
                )
                .into_any_element(),
            );
        }
    }

    // Right Side Content Views Builder
    let mut sections = Vec::new();

    // General Panel
    if (state.active_tab == SettingsTab::General && query.is_empty())
        || (!query.is_empty()
            && (is_match("Language", &["language", "english"])
                || is_match("Show start screen", &["start", "screen", "wizard"])
                || is_match("Check updates", &["updates", "check"])))
    {
        let on_update = callbacks.on_update_setting.clone();
        sections.push(
            settings_section_card()
                .child(settings_i18n_header(
                    i18n,
                    "settings.section.application",
                    assets::ICON_FILE_PATH,
                ))
                .child(settings_daw_row(i18n.tr("settings.field.language"), {
                    let open_combo = callbacks.open_hardware_combo;
                    let on_toggle = callbacks.on_toggle_hardware_combo.clone();
                    let selected = selected_locale_label(i18n, &schema.general.language);
                    hardware_select(
                        HardwareCombo::Language,
                        "settings-general-language",
                        &selected,
                        open_combo,
                        on_toggle,
                    )
                }))
                .child(settings_daw_row(
                    i18n.tr("settings.field.start-wizard"),
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child({
                            let val = schema.general.show_start_screen;
                            let up = on_update.clone();
                            fb_checkbox("show-start-screen", val, move |_, w, cx| {
                                up(Arc::new(move |s| s.general.show_start_screen = !val), w, cx);
                            })
                        })
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(Colors::text_muted())
                                .child(i18n.tr("settings.show-start-screen")),
                        ),
                ))
                .child(settings_daw_row(
                    i18n.tr("settings.field.update-check"),
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child({
                            let val = schema.general.check_updates;
                            let up = on_update.clone();
                            fb_checkbox("check-updates", val, move |_, w, cx| {
                                up(Arc::new(move |s| s.general.check_updates = !val), w, cx);
                            })
                        })
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(Colors::text_muted())
                                .child(i18n.tr("settings.check-updates")),
                        ),
                ))
                .into_any_element(),
        );
    }

    // General Panel > Autosave & Notifications
    if (state.active_tab == SettingsTab::General && query.is_empty())
        || (!query.is_empty()
            && (is_match("Autosave", &["autosave", "backup", "minutes"])
                || is_match("Notifications", &["warnings", "alerts", "notifications"])))
    {
        let on_update = callbacks.on_update_setting.clone();
        sections.push(
            settings_section_card()
                .child(settings_i18n_header(
                    i18n,
                    "settings.section.autosave-backup",
                    assets::ICON_FILE_PATH,
                ))
                .child(settings_daw_row(
                    i18n.tr("settings.field.autosave"),
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child({
                            let val = schema.general.autosave.enabled;
                            let up = on_update.clone();
                            fb_checkbox("autosave-enabled", val, move |_, w, cx| {
                                up(Arc::new(move |s| s.general.autosave.enabled = !val), w, cx);
                            })
                        })
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(Colors::text_muted())
                                .child(i18n.tr("settings.autosave.enabled")),
                        ),
                ))
                .child(settings_daw_row(i18n.tr("settings.field.interval"), {
                    let open_combo = callbacks.open_hardware_combo;
                    let on_toggle = callbacks.on_toggle_hardware_combo.clone();
                    let interval = schema.general.autosave.interval_minutes;
                    hardware_select(
                        HardwareCombo::AutosaveInterval,
                        "settings-general-autosave-interval",
                        &i18n.tr_vars("settings.interval.minutes", &[("n", interval.to_string())]),
                        open_combo,
                        on_toggle,
                    )
                }))
                .child(settings_daw_row(i18n.tr("settings.field.max-backups"), {
                    let open_combo = callbacks.open_hardware_combo;
                    let on_toggle = callbacks.on_toggle_hardware_combo.clone();
                    hardware_select(
                        HardwareCombo::AutosaveMaxBackups,
                        "settings-general-autosave-backups",
                        &schema.general.autosave.max_backups.to_string(),
                        open_combo,
                        on_toggle,
                    )
                }))
                .into_any_element(),
        );

        sections.push(
            settings_section_card()
                .child(settings_i18n_header(
                    i18n,
                    "settings.section.notifications",
                    assets::ICON_FILE_PATH,
                ))
                .child(settings_daw_row(
                    i18n.tr("settings.field.warnings"),
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child({
                            let val = schema.general.notifications.enable_warnings;
                            let up = on_update.clone();
                            fb_checkbox("notif-warnings-enabled", val, move |_, w, cx| {
                                up(
                                    Arc::new(move |s| {
                                        s.general.notifications.enable_warnings = !val
                                    }),
                                    w,
                                    cx,
                                );
                            })
                        })
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(Colors::text_muted())
                                .child(i18n.tr("settings.notifications.warnings")),
                        ),
                ))
                .child(settings_daw_row(
                    i18n.tr("settings.field.system-notifications"),
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child({
                            let val = schema.general.notifications.enable_system_notifications;
                            let up = on_update.clone();
                            fb_checkbox("notif-system-enabled", val, move |_, w, cx| {
                                up(
                                    Arc::new(move |s| {
                                        s.general.notifications.enable_system_notifications = !val
                                    }),
                                    w,
                                    cx,
                                );
                            })
                        })
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(Colors::text_muted())
                                .child(i18n.tr("settings.notifications.system")),
                        ),
                ))
                .into_any_element(),
        );
    }

    if (state.active_tab == SettingsTab::General && query.is_empty())
        || (!query.is_empty() && (is_match("Tempo", &["tempo", "bpm"])))
    {
        let on_update = callbacks.on_update_setting.clone();
        sections.push(
            settings_section_card()
                .child(settings_i18n_header(
                    i18n,
                    "settings.section.project-defaults",
                    assets::ICON_FILE_PATH,
                ))
                .child(settings_section_hint(
                    i18n.tr("settings.project-defaults.hint"),
                ))
                .child(settings_daw_row(
                    i18n.tr("settings.field.default-tempo"),
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .child({
                            let up = on_update.clone();
                            let tempo = schema.general.project_defaults.tempo;
                            fb_stepper_button("tempo-dec", "-", move |_, w, cx| {
                                up(
                                    Arc::new(move |s| {
                                        s.general.project_defaults.tempo = (tempo - 1.0).max(20.0)
                                    }),
                                    w,
                                    cx,
                                );
                            })
                        })
                        .child(
                            div()
                                .w(px(52.0))
                                .h(px(28.0))
                                .rounded_md()
                                .border(px(1.0))
                                .border_color(Colors::border_subtle())
                                .bg(Colors::surface_input())
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(11.0))
                                .text_color(Colors::text_primary())
                                .child(format!("{:.0}", schema.general.project_defaults.tempo)),
                        )
                        .child({
                            let up = on_update.clone();
                            let tempo = schema.general.project_defaults.tempo;
                            fb_stepper_button("tempo-inc", "+", move |_, w, cx| {
                                up(
                                    Arc::new(move |s| {
                                        s.general.project_defaults.tempo = (tempo + 1.0).min(999.0)
                                    }),
                                    w,
                                    cx,
                                );
                            })
                        })
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(Colors::text_muted())
                                .child(i18n.tr("settings.bpm")),
                        ),
                ))
                .into_any_element(),
        );
    }

    // Audio panel
    if (state.active_tab == SettingsTab::Audio && query.is_empty())
        || (!query.is_empty()
            && (is_match("Audio Driver", &["driver", "backend", "wasapi"])
                || is_match("Input Device", &["input", "microphone"])
                || is_match("Output Device", &["output", "speakers"])
                || is_match("Sample Rate", &["sample", "rate", "hz"])
                || is_match("Buffer Size", &["buffer", "latency"])))
    {
        let open_combo = callbacks.open_hardware_combo;
        let on_toggle = callbacks.on_toggle_hardware_combo.clone();

        let driver_select = hardware_select(
            HardwareCombo::AudioDriver,
            "settings-audio-driver",
            &schema.hardware.audio.driver_type,
            open_combo,
            on_toggle.clone(),
        );

        let input_select = hardware_select(
            HardwareCombo::InputDevice,
            "settings-audio-input",
            &schema.hardware.audio.device_in,
            open_combo,
            on_toggle.clone(),
        );

        let output_select = hardware_select(
            HardwareCombo::OutputDevice,
            "settings-audio-output",
            &schema.hardware.audio.device_out,
            open_combo,
            on_toggle.clone(),
        );

        let buffer_ms = schema.general.project_defaults.buffer_size as f32
            / schema.general.project_defaults.sample_rate as f32
            * 1000.0;

        sections.push(
            settings_section_card()
                .child(settings_i18n_header(
                    i18n,
                    "settings.section.audio-engine",
                    assets::ICON_MIC_PATH,
                ))
                .child(settings_daw_row(
                    i18n.tr("settings.field.backend"),
                    driver_select,
                ))
                .child(settings_daw_row(
                    i18n.tr("settings.field.input-device"),
                    input_select,
                ))
                .child(settings_daw_row(
                    i18n.tr("settings.field.output-device"),
                    output_select,
                ))
                .child(settings_daw_row(
                    i18n.tr("settings.field.driver-status"),
                    settings_status_badge(i18n.tr("settings.driver-status.ready"), true),
                ))
                .into_any_element(),
        );

        sections.push(
            settings_section_card()
                .child(settings_i18n_header(
                    i18n,
                    "settings.section.sample-rate-buffer",
                    assets::ICON_MIC_PATH,
                ))
                .child(settings_daw_row(i18n.tr("settings.field.sample-rate"), {
                    let open_combo = callbacks.open_hardware_combo;
                    let on_toggle = callbacks.on_toggle_hardware_combo.clone();
                    let sr = schema.general.project_defaults.sample_rate;
                    hardware_select(
                        HardwareCombo::SampleRate,
                        "settings-audio-sample-rate",
                        &i18n.tr_vars("settings.sample-rate.hz", &[("rate", sr.to_string())]),
                        open_combo,
                        on_toggle,
                    )
                }))
                .child(settings_daw_row(i18n.tr("settings.field.buffer-size"), {
                    let open_combo = callbacks.open_hardware_combo;
                    let on_toggle = callbacks.on_toggle_hardware_combo.clone();
                    let buf = schema.general.project_defaults.buffer_size;
                    hardware_select(
                        HardwareCombo::BufferSize,
                        "settings-audio-buffer-size",
                        &format!("{buf}"),
                        open_combo,
                        on_toggle,
                    )
                }))
                .child(settings_daw_row(
                    i18n.tr("settings.field.round-trip-latency"),
                    settings_value_readout(i18n.tr_vars(
                        "settings.latency.approx",
                        &[("ms", format!("{buffer_ms:.1}"))],
                    )),
                ))
                .child(settings_section_hint(i18n.tr("settings.buffer.hint")))
                .into_any_element(),
        );
    }

    // MIDI panel
    if (state.active_tab == SettingsTab::Midi && query.is_empty())
        || (!query.is_empty()
            && (is_match(
                "MIDI Enabled Inputs",
                &["midi", "inputs", "outputs", "port", "keyboard"],
            ) || is_match("Sync Clock", &["sync", "clock", "source", "ltc"])))
    {
        let on_update = callbacks.on_update_setting.clone();
        let up = on_update.clone();
        sections.push(
            settings_section_card()
                .child(settings_i18n_header(
                    i18n,
                    "settings.section.midi-devices",
                    assets::ICON_LINK_PATH,
                ))
                .child(settings_daw_row(
                    i18n.tr("settings.field.midi-inputs"),
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.0))
                        .child({
                            let enabled = schema
                                .hardware
                                .midi
                                .enabled_inputs
                                .contains(&"Keyboard Controller".to_string());
                            let up_in = up.clone();
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(6.0))
                                .child(fb_checkbox(
                                    "midi-keyboard-ctrl",
                                    enabled,
                                    move |_, w, cx| {
                                        up_in(
                                            Arc::new(move |s| {
                                                let list = &mut s.hardware.midi.enabled_inputs;
                                                if enabled {
                                                    list.retain(|x| x != "Keyboard Controller");
                                                } else if !list
                                                    .contains(&"Keyboard Controller".to_string())
                                                {
                                                    list.push("Keyboard Controller".to_string());
                                                }
                                            }),
                                            w,
                                            cx,
                                        );
                                    },
                                ))
                                .child(
                                    div()
                                        .text_size(px(10.5))
                                        .text_color(Colors::text_primary())
                                        .child(i18n.tr("settings.midi.keyboard-controller")),
                                )
                        })
                        .child({
                            let enabled = schema
                                .hardware
                                .midi
                                .enabled_inputs
                                .contains(&"Midi Device 2".to_string());
                            let up_in = up.clone();
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(6.0))
                                .child(fb_checkbox("midi-device-2", enabled, move |_, w, cx| {
                                    up_in(
                                        Arc::new(move |s| {
                                            let list = &mut s.hardware.midi.enabled_inputs;
                                            if enabled {
                                                list.retain(|x| x != "Midi Device 2");
                                            } else if !list.contains(&"Midi Device 2".to_string()) {
                                                list.push("Midi Device 2".to_string());
                                            }
                                        }),
                                        w,
                                        cx,
                                    );
                                }))
                                .child(
                                    div()
                                        .text_size(px(10.5))
                                        .text_color(Colors::text_primary())
                                        .child(i18n.tr("settings.midi.device-2")),
                                )
                        }),
                ))
                .child(settings_daw_row(
                    i18n.tr("settings.field.midi-outputs"),
                    div().flex().flex_col().gap(px(6.0)).child({
                        let enabled = schema
                            .hardware
                            .midi
                            .enabled_outputs
                            .contains(&"Synth Out".to_string());
                        let up_out = up.clone();
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(6.0))
                            .child(fb_checkbox("midi-synth-out", enabled, move |_, w, cx| {
                                up_out(
                                    Arc::new(move |s| {
                                        let list = &mut s.hardware.midi.enabled_outputs;
                                        if enabled {
                                            list.retain(|x| x != "Synth Out");
                                        } else if !list.contains(&"Synth Out".to_string()) {
                                            list.push("Synth Out".to_string());
                                        }
                                    }),
                                    w,
                                    cx,
                                );
                            }))
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(Colors::text_primary())
                                    .child(i18n.tr("settings.midi.synth-out")),
                            )
                    }),
                ))
                .child(settings_daw_row(
                    i18n.tr("settings.field.midi-clock-sync"),
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child({
                            let val = schema.hardware.midi.clock_sync;
                            let up_sync = up.clone();
                            fb_checkbox("midi-clock-sync", val, move |_, w, cx| {
                                up_sync(
                                    Arc::new(move |s| s.hardware.midi.clock_sync = !val),
                                    w,
                                    cx,
                                );
                            })
                        })
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(Colors::text_muted())
                                .child(i18n.tr("settings.midi.clock-sync")),
                        ),
                ))
                .into_any_element(),
        );

        let clock_select = hardware_select(
            HardwareCombo::ClockSource,
            "settings-clock-source",
            &schema.hardware.sync.clock_source,
            callbacks.open_hardware_combo,
            callbacks.on_toggle_hardware_combo.clone(),
        );
        sections.push(
            settings_section_card()
                .child(settings_i18n_header(
                    i18n,
                    "settings.section.sync-external-clock",
                    assets::ICON_CLOCK_PATH,
                ))
                .child(settings_daw_row(
                    i18n.tr("settings.field.clock-source"),
                    clock_select,
                ))
                .child(settings_daw_row(
                    i18n.tr("settings.field.ltc-reader"),
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child({
                            let val = schema.hardware.sync.ltc_enabled;
                            let up_ltc = up.clone();
                            fb_checkbox("sync-ltc-enabled", val, move |_, w, cx| {
                                up_ltc(
                                    Arc::new(move |s| s.hardware.sync.ltc_enabled = !val),
                                    w,
                                    cx,
                                );
                            })
                        })
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(Colors::text_muted())
                                .child(i18n.tr("settings.ltc.enable")),
                        ),
                ))
                .into_any_element(),
        );
    }

    // Appearance Panel (Theme, sliders)
    if (state.active_tab == SettingsTab::Appearance && query.is_empty())
        || (!query.is_empty()
            && (is_match("Theme", &["theme", "fleet", "dark"])
                || is_match("UI Scale", &["scale", "size"])
                || is_match("Arrangement Grid", &["grid", "intensity", "opacity"])
                || is_match("Piano Roll Guides", &["piano", "roll", "guides", "keys"])
                || is_match("Mixer Meter", &["mixer", "decay", "peak", "hold"])))
    {
        let on_update = callbacks.on_update_setting.clone();
        sections.push(
            settings_section_card()
                .child(settings_header(
                    "Theme & Interface",
                    assets::ICON_SLIDERS_HORIZONTAL_PATH,
                ))
                .child(settings_daw_row(
                    "Theme Preset",
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(4.0))
                        .child({
                            let val = schema.appearance.theme.clone();
                            let up = on_update.clone();
                            fb_segmented_button(
                                "theme-fleet",
                                "Fleet Dark",
                                val == "Fleet Dark",
                                move |_, w, cx| {
                                    up(
                                        Arc::new(|s| s.appearance.theme = "Fleet Dark".to_string()),
                                        w,
                                        cx,
                                    );
                                },
                            )
                        })
                        .child({
                            let val = schema.appearance.theme.clone();
                            let up = on_update.clone();
                            fb_segmented_button(
                                "theme-ableton",
                                "Ableton Dark",
                                val == "Ableton Dark",
                                move |_, w, cx| {
                                    up(
                                        Arc::new(|s| {
                                            s.appearance.theme = "Ableton Dark".to_string()
                                        }),
                                        w,
                                        cx,
                                    );
                                },
                            )
                        }),
                ))
                .child(settings_daw_row(
                    "UI Scale",
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(slider(
                            "ui-scale-slider",
                            (schema.appearance.ui_scale - 0.5) / 2.0, // map [0.5, 2.5] to [0, 1]
                            Colors::accent_primary(),
                            {
                                let up = on_update.clone();
                                move |val, w, cx| {
                                    let actual_val = 0.5 + val * 2.0;
                                    up(
                                        Arc::new(move |s| s.appearance.ui_scale = actual_val),
                                        w,
                                        cx,
                                    );
                                }
                            },
                        ))
                        .child(
                            div()
                                .w(px(32.0))
                                .text_size(px(10.0))
                                .text_color(Colors::text_muted())
                                .child(format!("{:.1}x", schema.appearance.ui_scale)),
                        ),
                ))
                .into_any_element(),
        );

        sections.push(
            settings_section_card()
                .child(settings_header(
                    "Timeline",
                    assets::ICON_SLIDERS_HORIZONTAL_PATH,
                ))
                .child(settings_daw_row(
                    "Grid Intensity",
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(slider(
                            "grid-intensity-slider",
                            schema.appearance.arrangement.grid_line_intensity,
                            Colors::accent_primary(),
                            {
                                let up = on_update.clone();
                                move |val, w, cx| {
                                    let intensity = *val;
                                    up(
                                        Arc::new(move |s| {
                                            s.appearance.arrangement.grid_line_intensity = intensity
                                        }),
                                        w,
                                        cx,
                                    );
                                }
                            },
                        ))
                        .child(
                            div()
                                .w(px(32.0))
                                .text_size(px(10.0))
                                .text_color(Colors::text_muted())
                                .child(format!(
                                    "{:.0}%",
                                    schema.appearance.arrangement.grid_line_intensity * 100.0
                                )),
                        ),
                ))
                .into_any_element(),
        );

        let up = on_update.clone();
        sections.push(
            settings_section_card()
                .child(settings_header("Piano Roll", assets::ICON_PENCIL_PATH))
                .child(settings_daw_row(
                    "Key Guides",
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child({
                            let val = schema.appearance.piano_roll.show_key_guides;
                            let up_guides = up.clone();
                            fb_checkbox("appearance-key-guides", val, move |_, w, cx| {
                                up_guides(
                                    Arc::new(move |s| {
                                        s.appearance.piano_roll.show_key_guides = !val
                                    }),
                                    w,
                                    cx,
                                );
                            })
                        })
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(Colors::text_muted())
                                .child("Show piano key guides in background"),
                        ),
                ))
                .into_any_element(),
        );

        let up = on_update.clone();
        sections.push(
            settings_section_card()
                .child(settings_header(
                    "Mixer & Metering",
                    assets::ICON_SLIDERS_HORIZONTAL_PATH,
                ))
                .child(settings_daw_row(
                    "Meter Decay",
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(slider(
                            "mixer-decay-slider",
                            (schema.appearance.mixer.meter_decay_db_per_sec - 12.0) / 36.0, // map [12, 48] to [0, 1]
                            Colors::accent_primary(),
                            {
                                let up_decay = up.clone();
                                move |val, w, cx| {
                                    let actual_val = 12.0 + val * 36.0;
                                    up_decay(
                                        Arc::new(move |s| {
                                            s.appearance.mixer.meter_decay_db_per_sec = actual_val
                                        }),
                                        w,
                                        cx,
                                    );
                                }
                            },
                        ))
                        .child(
                            div()
                                .w(px(52.0))
                                .text_size(px(10.0))
                                .text_color(Colors::text_muted())
                                .child(format!(
                                    "{:.1} dB/s",
                                    schema.appearance.mixer.meter_decay_db_per_sec
                                )),
                        ),
                ))
                .child(settings_daw_row(
                    "Peak Hold",
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(slider(
                            "mixer-peak-slider",
                            (schema.appearance.mixer.peak_hold_seconds - 0.5) / 4.5, // map [0.5, 5.0] to [0, 1]
                            Colors::accent_primary(),
                            {
                                let up_peak = up.clone();
                                move |val, w, cx| {
                                    let actual_val = 0.5 + val * 4.5;
                                    up_peak(
                                        Arc::new(move |s| {
                                            s.appearance.mixer.peak_hold_seconds = actual_val
                                        }),
                                        w,
                                        cx,
                                    );
                                }
                            },
                        ))
                        .child(
                            div()
                                .w(px(52.0))
                                .text_size(px(10.0))
                                .text_color(Colors::text_muted())
                                .child(format!(
                                    "{:.1} s",
                                    schema.appearance.mixer.peak_hold_seconds
                                )),
                        ),
                ))
                .into_any_element(),
        );
    }

    // Editing Panel (Mouse, snap, undo history)
    if (state.active_tab == SettingsTab::Editing && query.is_empty())
        || (!query.is_empty()
            && (is_match("Mouse Zoom", &["mouse", "zoom", "sensitivity", "natural"])
                || is_match("Snap to Grid", &["snap", "grid", "default"])
                || is_match("Undo History", &["undo", "redo", "history", "max"])))
    {
        let on_update = callbacks.on_update_setting.clone();

        sections.push(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(settings_header(
                    "Editing > Mouse & Navigation",
                    assets::ICON_PENCIL_PATH,
                ))
                .child(settings_daw_row(
                    "Zoom Sensitivity",
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(slider(
                            "zoom-sensitivity-slider",
                            (schema.editing.mouse.zoom_sensitivity - 0.2) / 1.8, // map [0.2, 2.0] to [0, 1]
                            Colors::accent_primary(),
                            {
                                let up = on_update.clone();
                                move |val, w, cx| {
                                    let actual_val = 0.2 + val * 1.8;
                                    up(
                                        Arc::new(move |s| {
                                            s.editing.mouse.zoom_sensitivity = actual_val
                                        }),
                                        w,
                                        cx,
                                    );
                                }
                            },
                        ))
                        .child(
                            div()
                                .w(px(32.0))
                                .text_size(px(10.0))
                                .text_color(Colors::text_muted())
                                .child(format!("{:.1}x", schema.editing.mouse.zoom_sensitivity)),
                        ),
                ))
                .child(settings_daw_row(
                    "Natural Scroll",
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child({
                            let val = schema.editing.mouse.natural_scroll;
                            let up = on_update.clone();
                            fb_checkbox("editing-natural-scroll", val, move |_, w, cx| {
                                up(
                                    Arc::new(move |s| s.editing.mouse.natural_scroll = !val),
                                    w,
                                    cx,
                                );
                            })
                        })
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(Colors::text_muted())
                                .child("Invert trackpad/mousewheel scroll direction"),
                        ),
                ))
                .into_any_element(),
        );

        let up = on_update.clone();
        sections.push(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .mt(px(12.0))
                .child(settings_header(
                    "Editing > Grid & Snap",
                    assets::ICON_SLIDERS_HORIZONTAL_PATH,
                ))
                .child(settings_daw_row(
                    "Snap to Grid",
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child({
                            let val = schema.editing.snap.snap_to_grid;
                            let up_snap = up.clone();
                            fb_checkbox("editing-snap-grid", val, move |_, w, cx| {
                                up_snap(
                                    Arc::new(move |s| s.editing.snap.snap_to_grid = !val),
                                    w,
                                    cx,
                                );
                            })
                        })
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(Colors::text_muted())
                                .child("Snap clips/notes to current grid lines"),
                        ),
                ))
                .child(settings_daw_row(
                    "Default Snap",
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(4.0))
                        .child({
                            let val = schema.editing.snap.default_snap_value.clone();
                            let up_val = up.clone();
                            fb_segmented_button("snap-1-4", "1/4", val == "1/4", move |_, w, cx| {
                                up_val(
                                    Arc::new(|s| {
                                        s.editing.snap.default_snap_value = "1/4".to_string()
                                    }),
                                    w,
                                    cx,
                                );
                            })
                        })
                        .child({
                            let val = schema.editing.snap.default_snap_value.clone();
                            let up_val = up.clone();
                            fb_segmented_button("snap-1-8", "1/8", val == "1/8", move |_, w, cx| {
                                up_val(
                                    Arc::new(|s| {
                                        s.editing.snap.default_snap_value = "1/8".to_string()
                                    }),
                                    w,
                                    cx,
                                );
                            })
                        })
                        .child({
                            let val = schema.editing.snap.default_snap_value.clone();
                            let up_val = up.clone();
                            fb_segmented_button(
                                "snap-1-16",
                                "1/16",
                                val == "1/16",
                                move |_, w, cx| {
                                    up_val(
                                        Arc::new(|s| {
                                            s.editing.snap.default_snap_value = "1/16".to_string()
                                        }),
                                        w,
                                        cx,
                                    );
                                },
                            )
                        })
                        .child({
                            let val = schema.editing.snap.default_snap_value.clone();
                            let up_val = up.clone();
                            fb_segmented_button(
                                "snap-1-32",
                                "1/32",
                                val == "1/32",
                                move |_, w, cx| {
                                    up_val(
                                        Arc::new(|s| {
                                            s.editing.snap.default_snap_value = "1/32".to_string()
                                        }),
                                        w,
                                        cx,
                                    );
                                },
                            )
                        }),
                ))
                .into_any_element(),
        );

        let up = on_update.clone();
        sections.push(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .mt(px(12.0))
                .child(settings_header(
                    "Editing > History",
                    assets::ICON_CLOCK_PATH,
                ))
                .child(settings_daw_row(
                    "Max Undo Steps",
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .child({
                            let val = schema.editing.history.max_undo_steps;
                            let up_steps = up.clone();
                            fb_stepper_button("undo-steps-dec", "-", move |_, w, cx| {
                                up_steps(
                                    Arc::new(move |s| {
                                        s.editing.history.max_undo_steps =
                                            val.saturating_sub(5).max(10)
                                    }),
                                    w,
                                    cx,
                                );
                            })
                        })
                        .child(
                            div()
                                .w(px(40.0))
                                .h(px(28.0))
                                .rounded_md()
                                .border(px(1.0))
                                .border_color(Colors::border_subtle())
                                .bg(Colors::surface_input())
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(11.0))
                                .text_color(Colors::text_primary())
                                .child(schema.editing.history.max_undo_steps.to_string()),
                        )
                        .child({
                            let val = schema.editing.history.max_undo_steps;
                            let up_steps = up.clone();
                            fb_stepper_button("undo-steps-inc", "+", move |_, w, cx| {
                                up_steps(
                                    Arc::new(move |s| {
                                        s.editing.history.max_undo_steps = (val + 5).min(500)
                                    }),
                                    w,
                                    cx,
                                );
                            })
                        }),
                ))
                .into_any_element(),
        );
    }

    // Recording Panel (Audio recording format, Metronome)
    if (state.active_tab == SettingsTab::Recording && query.is_empty())
        || (!query.is_empty()
            && (is_match("Audio Recording Format", &["format", "bit", "depth", "wav"])
                || is_match(
                    "Metronome Click",
                    &["metronome", "click", "sound", "volume"],
                )))
    {
        let on_update = callbacks.on_update_setting.clone();

        sections.push(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(settings_header(
                    "Recording > Audio Format",
                    assets::ICON_CIRCLE_PATH,
                ))
                .child(settings_daw_row(
                    "Format Type",
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(4.0))
                        .child({
                            let val = schema.recording.audio.format.clone();
                            let up = on_update.clone();
                            fb_segmented_button(
                                "rec-format-wav",
                                "WAV",
                                val == "wav",
                                move |_, w, cx| {
                                    up(
                                        Arc::new(|s| s.recording.audio.format = "wav".to_string()),
                                        w,
                                        cx,
                                    );
                                },
                            )
                        })
                        .child({
                            let val = schema.recording.audio.format.clone();
                            let up = on_update.clone();
                            fb_segmented_button(
                                "rec-format-aiff",
                                "AIFF",
                                val == "aiff",
                                move |_, w, cx| {
                                    up(
                                        Arc::new(|s| s.recording.audio.format = "aiff".to_string()),
                                        w,
                                        cx,
                                    );
                                },
                            )
                        })
                        .child({
                            let val = schema.recording.audio.format.clone();
                            let up = on_update.clone();
                            fb_segmented_button(
                                "rec-format-flac",
                                "FLAC",
                                val == "flac",
                                move |_, w, cx| {
                                    up(
                                        Arc::new(|s| s.recording.audio.format = "flac".to_string()),
                                        w,
                                        cx,
                                    );
                                },
                            )
                        }),
                ))
                .child(settings_daw_row(
                    "Bit Depth",
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(4.0))
                        .child({
                            let val = schema.recording.audio.bit_depth;
                            let up = on_update.clone();
                            fb_segmented_button(
                                "rec-depth-16",
                                "16-bit",
                                val == 16,
                                move |_, w, cx| {
                                    up(Arc::new(|s| s.recording.audio.bit_depth = 16), w, cx);
                                },
                            )
                        })
                        .child({
                            let val = schema.recording.audio.bit_depth;
                            let up = on_update.clone();
                            fb_segmented_button(
                                "rec-depth-24",
                                "24-bit",
                                val == 24,
                                move |_, w, cx| {
                                    up(Arc::new(|s| s.recording.audio.bit_depth = 24), w, cx);
                                },
                            )
                        })
                        .child({
                            let val = schema.recording.audio.bit_depth;
                            let up = on_update.clone();
                            fb_segmented_button(
                                "rec-depth-32",
                                "32-bit float",
                                val == 32,
                                move |_, w, cx| {
                                    up(Arc::new(|s| s.recording.audio.bit_depth = 32), w, cx);
                                },
                            )
                        }),
                ))
                .into_any_element(),
        );

        let up = on_update.clone();
        sections.push(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .mt(px(12.0))
                .child(settings_header(
                    "Recording > Metronome Click",
                    assets::ICON_CIRCLE_PATH,
                ))
                .child(settings_daw_row(
                    "Enable Click",
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child({
                            let val = schema.recording.metronome.enabled;
                            let up_met = up.clone();
                            fb_checkbox("rec-metronome-enabled", val, move |_, w, cx| {
                                up_met(
                                    Arc::new(move |s| s.recording.metronome.enabled = !val),
                                    w,
                                    cx,
                                );
                            })
                        })
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(Colors::text_muted())
                                .child("Hear metronome click during recording & playback"),
                        ),
                ))
                .child(settings_daw_row(
                    "Click Volume",
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(slider(
                            "metronome-volume-slider",
                            schema.recording.metronome.volume,
                            Colors::accent_primary(),
                            {
                                let up_vol = up.clone();
                                move |val, w, cx| {
                                    let volume = *val;
                                    up_vol(
                                        Arc::new(move |s| s.recording.metronome.volume = volume),
                                        w,
                                        cx,
                                    );
                                }
                            },
                        ))
                        .child(
                            div()
                                .w(px(32.0))
                                .text_size(px(10.0))
                                .text_color(Colors::text_muted())
                                .child(format!(
                                    "{:.0}%",
                                    schema.recording.metronome.volume * 100.0
                                )),
                        ),
                ))
                .child(settings_daw_row(
                    "Click Sound",
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(4.0))
                        .child({
                            let val = schema.recording.metronome.sound_type.clone();
                            let up_snd = up.clone();
                            fb_segmented_button(
                                "met-sound-wood",
                                "Woodblock",
                                val == "Woodblock",
                                move |_, w, cx| {
                                    up_snd(
                                        Arc::new(|s| {
                                            s.recording.metronome.sound_type =
                                                "Woodblock".to_string()
                                        }),
                                        w,
                                        cx,
                                    );
                                },
                            )
                        })
                        .child({
                            let val = schema.recording.metronome.sound_type.clone();
                            let up_snd = up.clone();
                            fb_segmented_button(
                                "met-sound-beep",
                                "Beep",
                                val == "Beep",
                                move |_, w, cx| {
                                    up_snd(
                                        Arc::new(|s| {
                                            s.recording.metronome.sound_type = "Beep".to_string()
                                        }),
                                        w,
                                        cx,
                                    );
                                },
                            )
                        }),
                ))
                .child(settings_daw_row(
                    "Count-in Bars",
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .child({
                            let val = schema.recording.metronome.count_in_bars;
                            let up_cnt = up.clone();
                            fb_stepper_button("met-count-dec", "-", move |_, w, cx| {
                                up_cnt(
                                    Arc::new(move |s| {
                                        s.recording.metronome.count_in_bars =
                                            val.saturating_sub(1).max(0)
                                    }),
                                    w,
                                    cx,
                                );
                            })
                        })
                        .child(
                            div()
                                .w(px(40.0))
                                .h(px(28.0))
                                .rounded_md()
                                .border(px(1.0))
                                .border_color(Colors::border_subtle())
                                .bg(Colors::surface_input())
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(11.0))
                                .text_color(Colors::text_primary())
                                .child(schema.recording.metronome.count_in_bars.to_string()),
                        )
                        .child({
                            let val = schema.recording.metronome.count_in_bars;
                            let up_cnt = up.clone();
                            fb_stepper_button("met-count-inc", "+", move |_, w, cx| {
                                up_cnt(
                                    Arc::new(move |s| {
                                        s.recording.metronome.count_in_bars = (val + 1).min(4)
                                    }),
                                    w,
                                    cx,
                                );
                            })
                        })
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(Colors::text_muted())
                                .child("bars"),
                        ),
                ))
                .into_any_element(),
        );
    }

    // Playback Panel (Transport options)
    if (state.active_tab == SettingsTab::Playback && query.is_empty())
        || (!query.is_empty()
            && (is_match(
                "Transport Playback",
                &["spacebar", "transport", "stop", "start"],
            )))
    {
        sections.push(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(settings_header(
                    "Playback > Transport",
                    assets::ICON_PLAY_PATH,
                ))
                .child(settings_daw_row(
                    "Spacebar Action",
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(4.0))
                        .child(fb_segmented_button(
                            "space-play-pause",
                            "Play / Pause",
                            true,
                            |_e, _w, _cx| {},
                        ))
                        .child(fb_segmented_button(
                            "space-play-stop",
                            "Play / Stop (Soon)",
                            false,
                            |_e, _w, _cx| {},
                        )),
                ))
                .child(settings_daw_row(
                    "Return to Start",
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(fb_checkbox("return-on-stop", true, |_e, _w, _cx| {}))
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(Colors::text_muted())
                                .child("Return playhead to start position on Stop"),
                        ),
                ))
                .into_any_element(),
        );
    }

    // Performance Panel — Renderer + GPU Device selection.
    if (state.active_tab == SettingsTab::Performance && query.is_empty())
        || (!query.is_empty()
            && (is_match("Renderer", &["renderer", "gpu", "cpu", "wgpu"])
                || is_match("GPU Device", &["gpu", "device", "adapter"])))
    {
        sections.push(
            performance_section(
                schema,
                callbacks.open_hardware_combo,
                callbacks.on_toggle_hardware_combo.clone(),
            )
            .into_any_element(),
        );
    }

    // Plugins Panel (vst directories list etc.)
    if (state.active_tab == SettingsTab::Plugins && query.is_empty())
        || (!query.is_empty()
            && (is_match("VST3 CLAP Formats", &["vst3", "clap", "plugins"])
                || is_match("Paths Directories", &["paths", "directories", "folders"])))
    {
        let on_update = callbacks.on_update_setting.clone();
        sections.push(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(settings_header(
                    "Plugins > Formats & Folders",
                    assets::ICON_CPU_PATH,
                ))
                .child(settings_daw_row(
                    "Formats",
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(16.0))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(6.0))
                                .child({
                                    let val = schema.plugins.vst3.enabled;
                                    let up = on_update.clone();
                                    fb_checkbox("vst3-enabled", val, move |_, w, cx| {
                                        up(Arc::new(move |s| s.plugins.vst3.enabled = !val), w, cx);
                                    })
                                })
                                .child(
                                    div()
                                        .text_size(px(10.5))
                                        .text_color(Colors::text_primary())
                                        .child("Enable VST3"),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(6.0))
                                .child({
                                    let val = schema.plugins.clap.enabled;
                                    let up = on_update.clone();
                                    fb_checkbox("clap-enabled", val, move |_, w, cx| {
                                        up(Arc::new(move |s| s.plugins.clap.enabled = !val), w, cx);
                                    })
                                })
                                .child(
                                    div()
                                        .text_size(px(10.5))
                                        .text_color(Colors::text_primary())
                                        .child("Enable CLAP"),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(6.0))
                                .child({
                                    let val = schema.plugins.scan.background_scan;
                                    let up = on_update.clone();
                                    fb_checkbox("scan-background-scan", val, move |_, w, cx| {
                                        up(
                                            Arc::new(move |s| {
                                                s.plugins.scan.background_scan = !val
                                            }),
                                            w,
                                            cx,
                                        );
                                    })
                                })
                                .child(
                                    div()
                                        .text_size(px(10.5))
                                        .text_color(Colors::text_primary())
                                        .child("Background Scan"),
                                ),
                        ),
                ))
                .child(settings_daw_row(
                    "VST3 Folders",
                    settings_path_list(&schema.plugins.vst3.paths),
                ))
                .child(settings_daw_row(
                    "CLAP Folders",
                    settings_path_list(&schema.plugins.clap.paths),
                ))
                .child(settings_daw_row(
                    "Actions",
                    fb_button(
                        "trigger-plugins-scan",
                        "Scan Plugins Now",
                        FbButtonKind::Primary,
                        true,
                        |_e, _w, _cx| {
                            eprintln!("[plugins] manual scan triggered from settings dialog");
                        },
                    ),
                ))
                .into_any_element(),
        );
    }

    // About Panel
    if (state.active_tab == SettingsTab::About && query.is_empty())
        || (!query.is_empty() && (is_match("Version About", &["version", "credits", "about"])))
    {
        sections.push(
            settings_section_card()
                .child(settings_header(
                    "Futureboard Studio",
                    assets::ICON_CIRCLE_DOT_PATH,
                ))
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(Colors::text_primary())
                        .child("Futureboard Studio / Mochi DAW v0.1.0"),
                )
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(Colors::text_muted())
                        .child("Built with GPUI, Rust, and C++ VST3 SDK."),
                )
                .child(
                    div()
                        .text_size(px(9.5))
                        .text_color(Colors::text_faint())
                        .child("© 2026 Futureboard Studio team. All rights reserved."),
                )
                .into_any_element(),
        );
    }

    // Placeholder panels for categories not yet fully wired
    if sections.is_empty() && query.is_empty() {
        let hint = match state.active_tab {
            SettingsTab::FilesMedia => {
                "Project folders, sample libraries, recording paths, and media cache settings."
            }
            SettingsTab::Shortcuts => {
                "Search, edit, and reset keyboard commands grouped by workflow area."
            }
            SettingsTab::Advanced => {
                "Experimental features, developer tools, and low-level engine options."
            }
            _ => "",
        };
        if !hint.is_empty() {
            sections.push(
                settings_section_card()
                    .child(settings_section_title(
                        i18n.tr(state.active_tab.label_key()),
                    ))
                    .child(settings_section_hint(hint))
                    .child(
                        div()
                            .pt(px(6.0))
                            .text_size(px(10.0))
                            .text_color(Colors::text_muted())
                            .child("This section is scaffolded for future settings."),
                    )
                    .into_any_element(),
            );
        }
    }

    if sections.is_empty() {
        sections.push(
            div()
                .px(px(12.0))
                .py(px(24.0))
                .text_align(gpui::TextAlign::Center)
                .text_size(px(11.0))
                .text_color(Colors::text_faint())
                .child(if query.is_empty() {
                    format!(
                        "The {} panel is not fully wired in Native yet.",
                        i18n.tr(state.active_tab.label_key())
                    )
                } else {
                    format!("No settings match \"{}\"", query)
                })
                .into_any_element(),
        );
    }
    (sidebar_items, sections)
}

pub fn settings_dialog(
    state: &SettingsDialogState,
    schema: &SettingsSchema,
    search_input: &TextInputState,
    search_focused: bool,
    search_callbacks: TextInputCallbacks,
    callbacks: SettingsDialogCallbacks,
    available_inputs: &[String],
    available_outputs: &[String],
    available_backends: &[String],
) -> impl IntoElement {
    let i18n = I18n::new(&schema.general.language);
    let close_backdrop = callbacks.on_close.clone();
    let close_button = callbacks.on_close.clone();

    let (sidebar_items, sections) = build_settings_content(
        state,
        schema,
        &callbacks,
        available_inputs,
        available_outputs,
        available_backends,
    );

    // Overlay shell
    div()
        .absolute()
        .top_0()
        .bottom_0()
        .left_0()
        .right_0()
        .flex()
        .items_start()
        .justify_center()
        .pt(px(56.0))
        .px(px(18.0))
        .pb(px(32.0))
        .id("settings-modal-overlay")
        .bg(gpui::transparent_black())
        .occlude()
        .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
            close_backdrop(&(), window, cx);
        })
        .child(
            div()
                .flex()
                .flex_col()
                .w(px(640.0))
                .max_w(px(640.0))
                .h(px(520.0))
                .max_h(px(520.0))
                .overflow_hidden()
                .rounded_xl()
                .border(px(1.0))
                .border_color(Colors::border_default())
                .bg(Colors::surface_window())
                .shadow(vec![gpui::BoxShadow {
                    color: Colors::surface_overlay().into(),
                    offset: gpui::point(px(0.0), px(16.0)),
                    blur_radius: px(40.0),
                    spread_radius: px(0.0),
                }])
                .on_mouse_down(gpui::MouseButton::Left, |_, _window, cx| {
                    cx.stop_propagation();
                })
                // Title Bar — matches project wizard style
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .h(px(32.0))
                        .pl(px(12.0))
                        .border_b(px(1.0))
                        .border_color(Colors::border_subtle())
                        .bg(Colors::surface_titlebar())
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .h_full()
                                .text_size(px(11.5))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(Colors::text_primary())
                                .child(i18n.tr("settings.title")),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .w(px(32.0))
                                .h(px(32.0))
                                .id("settings-close")
                                .cursor(gpui::CursorStyle::PointingHand)
                                .hover(|s| s.bg(Colors::surface_control_hover()))
                                .on_click(move |_, window, cx| close_button(&(), window, cx))
                                .child(icon(assets::ICON_X_PATH, 12.0, Colors::text_faint())),
                        ),
                )
                // Two-column layout
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_1()
                        .min_h_0()
                        // Left Sidebar: Search + Tabs List
                        .child(
                            div()
                                .id("settings-sidebar-scroll")
                                .w(px(160.0))
                                .flex_shrink_0()
                                .border_r(px(1.0))
                                .border_color(Colors::divider())
                                .bg(Colors::surface_panel_alt())
                                .overflow_y_scroll()
                                .p(px(8.0))
                                .flex()
                                .flex_col()
                                .gap(px(8.0))
                                .child(div().pb(px(4.0)).child(text_field_with_callbacks(
                                    search_input,
                                    search_focused,
                                    search_callbacks,
                                )))
                                .children(sidebar_items),
                        )
                        // Right Content Panel
                        .child(
                            div()
                                .id("settings-content-scroll")
                                .flex_1()
                                .bg(Colors::surface_panel())
                                .overflow_y_scroll()
                                .p(px(16.0))
                                .flex()
                                .flex_col()
                                .gap(px(16.0))
                                .children(sections),
                        ),
                ),
        )
}

const SETTINGS_WIDTH: f32 = SETTINGS_WINDOW_WIDTH;
const SETTINGS_HEIGHT: f32 = SETTINGS_WINDOW_HEIGHT;
const COMBO_MENU_ESTIMATE_HEIGHT: f32 = 148.0;
const CLOCK_SOURCE_OPTIONS: &[&str] = &["Internal", "MIDI"];
const AUTOSAVE_INTERVAL_OPTIONS: &[u32] = &[1, 2, 3, 5, 10, 15, 30, 60];
const AUTOSAVE_MAX_BACKUPS_OPTIONS: &[u32] = &[1, 2, 3, 5, 10, 20, 50, 99];
const SAMPLE_RATE_OPTIONS: &[u32] = &[44100, 48000, 88200, 96000];
const BUFFER_SIZE_OPTIONS: &[u32] = &[64, 128, 256, 512, 1024];

fn combo_menu_position(anchor: OverlayAnchor, window: &Window) -> crate::overlay::OverlayPosition {
    let layout = settings_form_column(window);
    let refreshed = refresh_form_anchor(anchor, layout);
    compute_overlay_position(
        refreshed.bounds,
        OverlaySize {
            width: layout.value_width,
            height: COMBO_MENU_ESTIMATE_HEIGHT,
        },
        window.bounds(),
        OverlayPlacement::BottomStart,
        4.0,
    )
}

fn hardware_combo_overlay(
    open_combo: HardwareCombo,
    anchor: OverlayAnchor,
    window: &Window,
    schema: &SettingsSchema,
    available_inputs: &[String],
    available_outputs: &[String],
    available_backends: &[String],
    on_update: Arc<dyn Fn(UpdateSettingFn, &mut Window, &mut App) + 'static>,
    close_target: Entity<SettingsWindow>,
) -> impl IntoElement {
    let i18n = I18n::new(&schema.general.language);
    let position = combo_menu_position(anchor, window);
    let close_target = close_target.clone();
    let experimental_asio = std::env::var("FUTUREBOARD_EXPERIMENTAL_ASIO")
        .ok()
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    let menu = match open_combo {
        HardwareCombo::AudioDriver => {
            let selected = schema.hardware.audio.driver_type.clone();
            let up = on_update.clone();
            let filtered_backends: Vec<String> = if experimental_asio {
                available_backends.to_vec()
            } else {
                available_backends
                    .iter()
                    .filter(|b| !b.to_ascii_lowercase().contains("asio"))
                    .cloned()
                    .collect()
            };
            combo_box_string_menu(
                "settings-audio-driver-menu",
                position,
                &selected,
                &filtered_backends,
                Arc::new(move |value, window, cx| {
                    up(
                        Arc::new(move |s| s.hardware.audio.driver_type = value.clone()),
                        window,
                        cx,
                    );
                }),
            )
            .into_any_element()
        }
        HardwareCombo::InputDevice => {
            let selected = schema.hardware.audio.device_in.clone();
            let up = on_update.clone();
            combo_box_string_menu(
                "settings-audio-input-menu",
                position,
                &selected,
                available_inputs,
                Arc::new(move |value, window, cx| {
                    up(
                        Arc::new(move |s| s.hardware.audio.device_in = value.clone()),
                        window,
                        cx,
                    );
                }),
            )
            .into_any_element()
        }
        HardwareCombo::OutputDevice => {
            let selected = schema.hardware.audio.device_out.clone();
            let up = on_update.clone();
            combo_box_string_menu(
                "settings-audio-output-menu",
                position,
                &selected,
                available_outputs,
                Arc::new(move |value, window, cx| {
                    up(
                        Arc::new(move |s| s.hardware.audio.device_out = value.clone()),
                        window,
                        cx,
                    );
                }),
            )
            .into_any_element()
        }
        HardwareCombo::ClockSource => {
            let selected = schema.hardware.sync.clock_source.clone();
            let options: Vec<String> = CLOCK_SOURCE_OPTIONS.iter().map(|s| s.to_string()).collect();
            let up = on_update;
            combo_box_string_menu(
                "settings-clock-source-menu",
                position,
                &selected,
                &options,
                Arc::new(move |value, window, cx| {
                    up(
                        Arc::new(move |s| s.hardware.sync.clock_source = value.clone()),
                        window,
                        cx,
                    );
                }),
            )
            .into_any_element()
        }
        HardwareCombo::Language => {
            let selected = selected_locale_label(i18n, &schema.general.language);
            let options: Vec<String> = Locale::ALL
                .iter()
                .map(|locale| locale_label(i18n, *locale))
                .collect();
            let up = on_update;
            combo_box_string_menu(
                "settings-general-language-menu",
                position,
                &selected,
                &options,
                Arc::new(move |value, window, cx| {
                    let locale_code = Locale::ALL
                        .iter()
                        .find(|locale| locale_label(i18n, **locale) == value)
                        .copied()
                        .unwrap_or(Locale::EnUs)
                        .code()
                        .to_string();
                    up(
                        Arc::new(move |s| s.general.language = locale_code.clone()),
                        window,
                        cx,
                    );
                }),
            )
            .into_any_element()
        }
        HardwareCombo::AutosaveInterval => {
            let selected = format!("{} min", schema.general.autosave.interval_minutes);
            let options: Vec<String> = AUTOSAVE_INTERVAL_OPTIONS
                .iter()
                .map(|m| format!("{m} min"))
                .collect();
            let up = on_update;
            combo_box_string_menu(
                "settings-general-autosave-interval-menu",
                position,
                &selected,
                &options,
                Arc::new(move |value, window, cx| {
                    let minutes = value
                        .split_whitespace()
                        .next()
                        .and_then(|v| v.parse::<u32>().ok())
                        .unwrap_or(5);
                    up(
                        Arc::new(move |s| s.general.autosave.interval_minutes = minutes),
                        window,
                        cx,
                    );
                }),
            )
            .into_any_element()
        }
        HardwareCombo::AutosaveMaxBackups => {
            let selected = schema.general.autosave.max_backups.to_string();
            let options: Vec<String> = AUTOSAVE_MAX_BACKUPS_OPTIONS
                .iter()
                .map(|v| v.to_string())
                .collect();
            let up = on_update;
            combo_box_string_menu(
                "settings-general-autosave-backups-menu",
                position,
                &selected,
                &options,
                Arc::new(move |value, window, cx| {
                    let backups = value.parse::<u32>().unwrap_or(10);
                    up(
                        Arc::new(move |s| s.general.autosave.max_backups = backups),
                        window,
                        cx,
                    );
                }),
            )
            .into_any_element()
        }
        HardwareCombo::SampleRate => {
            let selected = format!("{} Hz", schema.general.project_defaults.sample_rate);
            let options: Vec<String> = SAMPLE_RATE_OPTIONS
                .iter()
                .map(|v| format!("{v} Hz"))
                .collect();
            let up = on_update;
            combo_box_string_menu(
                "settings-audio-sample-rate-menu",
                position,
                &selected,
                &options,
                Arc::new(move |value, window, cx| {
                    let sr = value
                        .split_whitespace()
                        .next()
                        .and_then(|v| v.parse::<u32>().ok())
                        .unwrap_or(48000);
                    up(
                        Arc::new(move |s| s.general.project_defaults.sample_rate = sr),
                        window,
                        cx,
                    );
                }),
            )
            .into_any_element()
        }
        HardwareCombo::BufferSize => {
            let selected = schema.general.project_defaults.buffer_size.to_string();
            let options: Vec<String> = BUFFER_SIZE_OPTIONS.iter().map(|v| v.to_string()).collect();
            let up = on_update;
            combo_box_string_menu(
                "settings-audio-buffer-size-menu",
                position,
                &selected,
                &options,
                Arc::new(move |value, window, cx| {
                    let buf = value.parse::<u32>().unwrap_or(256);
                    up(
                        Arc::new(move |s| s.general.project_defaults.buffer_size = buf),
                        window,
                        cx,
                    );
                }),
            )
            .into_any_element()
        }
        HardwareCombo::Renderer => {
            let selected = schema.performance.render_mode.label().to_string();
            let options: Vec<String> = vec![
                RenderMode::GpuAcceleration.label().to_string(),
                RenderMode::CpuRender.label().to_string(),
            ];
            let up = on_update;
            combo_box_string_menu(
                "settings-performance-renderer-menu",
                position,
                &selected,
                &options,
                Arc::new(move |value, window, cx| {
                    let mode = if value == RenderMode::CpuRender.label() {
                        RenderMode::CpuRender
                    } else {
                        RenderMode::GpuAcceleration
                    };
                    up(
                        Arc::new(move |s| s.performance.render_mode = mode),
                        window,
                        cx,
                    );
                }),
            )
            .into_any_element()
        }
        HardwareCombo::GpuDevice => {
            // Enumerate adapters on open. Cheap on Windows/macOS; the
            // dropdown shows the actual device names instead of a stale
            // cached list. Falls back to "Auto" only on enumeration failure.
            let detected = list_available_gpu_devices();
            let mut options: Vec<String> = Vec::with_capacity(detected.len() + 1);
            options.push("Auto".to_string());
            for device in &detected {
                options.push(device.name.clone());
            }
            if detected.is_empty() {
                options.push("No GPU device found".to_string());
            }
            let selected = match &schema.performance.gpu_device {
                GpuDevicePreference::Auto => "Auto".to_string(),
                GpuDevicePreference::DeviceId(id) => detected
                    .iter()
                    .find(|d| &d.id == id)
                    .map(|d| d.name.clone())
                    .unwrap_or_else(|| "Auto".to_string()),
            };
            // Build a stable label -> id map for commit time.
            let id_lookup: Vec<(String, String)> = detected
                .iter()
                .map(|d| (d.name.clone(), d.id.clone()))
                .collect();
            let up = on_update;
            combo_box_string_menu(
                "settings-performance-gpu-device-menu",
                position,
                &selected,
                &options,
                Arc::new(move |value, window, cx| {
                    if value == "No GPU device found" {
                        return;
                    }
                    let next = if value == "Auto" {
                        GpuDevicePreference::Auto
                    } else {
                        id_lookup
                            .iter()
                            .find(|(name, _)| name == &value)
                            .map(|(_, id)| GpuDevicePreference::DeviceId(id.clone()))
                            .unwrap_or(GpuDevicePreference::Auto)
                    };
                    up(
                        Arc::new(move |s| s.performance.gpu_device = next.clone()),
                        window,
                        cx,
                    );
                }),
            )
            .into_any_element()
        }
    };

    div()
        .absolute()
        .inset_0()
        .id("settings-hardware-combo-overlay")
        .on_mouse_down(MouseButton::Left, move |_, _window, cx| {
            let _ = close_target.update(cx, |this, cx| {
                this.open_hardware_combo = None;
                this.hardware_combo_anchor = None;
                cx.notify();
            });
        })
        .child(menu)
}

pub type OnSettingUpdate = Arc<dyn Fn(UpdateSettingFn, &mut App) + 'static>;

pub struct SettingsWindow {
    settings: Entity<SettingsModel>,
    active_tab: SettingsTab,
    search_input: TextInputState,
    available_inputs: Vec<String>,
    available_outputs: Vec<String>,
    available_backends: Vec<String>,
    open_hardware_combo: Option<HardwareCombo>,
    hardware_combo_anchor: Option<OverlayAnchor>,
    on_update: OnSettingUpdate,
    focus_handle: FocusHandle,
}

impl SettingsWindow {
    pub fn new(
        settings: Entity<SettingsModel>,
        available_inputs: Vec<String>,
        available_outputs: Vec<String>,
        available_backends: Vec<String>,
        on_update: OnSettingUpdate,
        cx: &mut Context<Self>,
    ) -> Self {
        let search_input = TextInputState::new("settings-search", cx.focus_handle())
            .with_placeholder("Search settings...");
        Self {
            settings,
            active_tab: SettingsTab::General,
            search_input,
            available_inputs,
            available_outputs,
            available_backends,
            open_hardware_combo: None,
            hardware_combo_anchor: None,
            on_update,
            focus_handle: cx.focus_handle(),
        }
    }

    fn handle_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key.as_str() == "escape" && self.open_hardware_combo.take().is_some() {
            self.hardware_combo_anchor = None;
            cx.notify();
            return;
        }

        let search_focused = self.search_input.is_focused(window);
        if search_focused {
            let action = self.search_input.handle_key_with_clipboard(event, Some(cx));
            match action {
                TextInputAction::Cancel => window.remove_window(),
                _ => {}
            }
            cx.notify();
            return;
        }
        let key = event.keystroke.key.as_str();
        let ctrl = event.keystroke.modifiers.control || event.keystroke.modifiers.platform;
        match key {
            "escape" => window.remove_window(),
            "f" if ctrl => {
                self.search_input.focus_handle.focus(window);
                cx.notify();
            }
            _ => {}
        }
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let schema = self.settings.read(cx).current.clone();
        let i18n = I18n::new(&schema.general.language);
        self.search_input.placeholder = Some(i18n.tr("search.settings.placeholder"));
        let target = cx.entity().clone();
        let on_update = self.on_update.clone();
        let search_focused = self.search_input.is_focused(window);

        let state = SettingsDialogState {
            is_open: true,
            active_tab: self.active_tab,
            search_query: self.search_input.value.clone(),
        };

        let callbacks = SettingsDialogCallbacks {
            on_close: Arc::new(|_: &(), window: &mut Window, _cx: &mut App| {
                window.remove_window();
            }),
            on_select_tab: Arc::new({
                let target = target.clone();
                move |tab: &SettingsTab, _w: &mut Window, cx: &mut App| {
                    let tab = *tab;
                    let _ = target.update(cx, |this, cx| {
                        this.active_tab = tab;
                        this.open_hardware_combo = None;
                        this.hardware_combo_anchor = None;
                        cx.notify();
                    });
                }
            }),
            on_update_setting: Arc::new({
                let on_update = on_update.clone();
                let target = target.clone();
                move |updater: UpdateSettingFn, _w: &mut Window, cx: &mut App| {
                    (on_update)(updater, cx);
                    let _ = target.update(cx, |this, cx| {
                        this.open_hardware_combo = None;
                        this.hardware_combo_anchor = None;
                        cx.notify();
                    });
                }
            }),
            open_hardware_combo: self.open_hardware_combo,
            on_toggle_hardware_combo: Arc::new({
                let target = target.clone();
                move |combo: HardwareCombo,
                      anchor: Option<OverlayAnchor>,
                      _w: &mut Window,
                      cx: &mut App| {
                    let _ = target.update(cx, |this, cx| {
                        if this.open_hardware_combo == Some(combo) {
                            this.open_hardware_combo = None;
                            this.hardware_combo_anchor = None;
                        } else {
                            this.open_hardware_combo = Some(combo);
                            this.hardware_combo_anchor = anchor;
                        }
                        cx.notify();
                    });
                }
            }),
        };

        let search_callbacks = TextInputCallbacks {
            on_context_menu: None,
            on_mouse: None,
        };

        let (sidebar_items, sections) = build_settings_content(
            &state,
            &schema,
            &callbacks,
            &self.available_inputs,
            &self.available_outputs,
            &self.available_backends,
        );

        let sw_target = target.clone();

        let combo_overlay = if let (Some(open_combo), Some(anchor)) =
            (self.open_hardware_combo, self.hardware_combo_anchor)
        {
            let close_target = sw_target.clone();
            let overlay_update = Arc::new({
                let on_update = on_update.clone();
                let target = sw_target.clone();
                move |updater: UpdateSettingFn, _w: &mut Window, cx: &mut App| {
                    (on_update)(updater, cx);
                    let _ = target.update(cx, |this, cx| {
                        this.open_hardware_combo = None;
                        this.hardware_combo_anchor = None;
                        cx.notify();
                    });
                }
            });
            Some(hardware_combo_overlay(
                open_combo,
                anchor,
                window,
                &schema,
                &self.available_inputs,
                &self.available_outputs,
                &self.available_backends,
                overlay_update,
                close_target,
            ))
        } else {
            None
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .relative()
            .font(theme::ui_font())
            .bg(Colors::surface_window())
            .overflow_hidden()
            .capture_key_down({
                let target = sw_target.clone();
                move |event, window, cx| {
                    let _ = target.update(cx, |this, cx| this.handle_key(event, window, cx));
                }
            })
            .child(div().w(px(0.0)).h(px(0.0)).track_focus(&self.focus_handle))
            .child(external_window_titlebar(
                i18n.tr("settings.title"),
                "settings-window-close",
                {
                    let target = sw_target.clone();
                    move |window, cx| {
                        let _ = target.update(cx, |this, cx| {
                            this.open_hardware_combo = None;
                            this.hardware_combo_anchor = None;
                            cx.notify();
                        });
                        window.remove_window();
                    }
                },
            ))
            // Two-column body — DAW studio control center layout
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id("settings-sidebar")
                            .w(px(SETTINGS_SIDEBAR_WIDTH))
                            .flex_shrink_0()
                            .border_r(px(1.0))
                            .border_color(Colors::divider())
                            .bg(Colors::surface_panel_alt())
                            .overflow_y_scroll()
                            .py(px(6.0))
                            .flex()
                            .flex_col()
                            .children(sidebar_items),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .bg(Colors::surface_panel())
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .px(px(SETTINGS_CONTENT_PAD))
                                    .pt(px(10.0))
                                    .pb(px(8.0))
                                    .border_b(px(1.0))
                                    .border_color(Colors::divider())
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .items_start()
                                            .justify_between()
                                            .gap(px(12.0))
                                            .child(settings_page_header(
                                                i18n.tr(self.active_tab.label_key()),
                                                i18n.tr(self.active_tab.page_description_key()),
                                            ))
                                            .child(div().w(px(208.0)).flex_shrink_0().child(
                                                text_field_with_callbacks(
                                                    &self.search_input,
                                                    search_focused,
                                                    search_callbacks,
                                                ),
                                            )),
                                    ),
                            )
                            .child(
                                div()
                                    .id("settings-content-scroll")
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_y_scroll()
                                    .p(px(SETTINGS_CONTENT_PAD))
                                    .flex()
                                    .flex_col()
                                    .gap(px(10.0))
                                    .children(sections),
                            ),
                    ),
            )
            .children(combo_overlay)
    }
}

pub fn open_settings_window(
    owner_bounds: Bounds<gpui::Pixels>,
    settings: Entity<SettingsModel>,
    available_inputs: Vec<String>,
    available_outputs: Vec<String>,
    available_backends: Vec<String>,
    on_update: OnSettingUpdate,
    cx: &mut App,
) -> Result<WindowHandle<SettingsWindow>, String> {
    let parent_x: f32 = owner_bounds.origin.x.into();
    let parent_y: f32 = owner_bounds.origin.y.into();
    let parent_w: f32 = owner_bounds.size.width.into();
    let parent_h: f32 = owner_bounds.size.height.into();
    let origin = Point {
        x: px(parent_x + ((parent_w - SETTINGS_WIDTH) / 2.0).max(24.0)),
        y: px(parent_y + ((parent_h - SETTINGS_HEIGHT) / 2.0).max(24.0)),
    };

    let mut options = crate::platform_chrome::external_dialog_window_options_partial();
    options.window_bounds = Some(WindowBounds::Windowed(Bounds {
        origin,
        size: size(px(SETTINGS_WIDTH), px(SETTINGS_HEIGHT)),
    }));
    options.kind = WindowKind::Floating;
    options.is_resizable = true;
    options.is_minimizable = false;
    options.window_background = WindowBackgroundAppearance::Transparent;
    options.window_min_size = Some(size(px(SETTINGS_WIDTH), px(SETTINGS_HEIGHT)));

    cx.open_window(options, move |_window, cx| {
        cx.new(|cx| {
            SettingsWindow::new(
                settings,
                available_inputs,
                available_outputs,
                available_backends,
                on_update,
                cx,
            )
        })
    })
    .map_err(|error| error.to_string())
}
