use std::sync::Arc;

use gpui::prelude::FluentBuilder;
use gpui::{
    div, px, size, svg, App, AppContext, Bounds, Context, FocusHandle, InteractiveElement,
    IntoElement, KeyDownEvent, ParentElement, Point, Render, StatefulInteractiveElement, Styled,
    Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind,
};

use crate::assets;
use crate::components::controls::{fb_button, fb_form_row, fb_stepper_button, FbButtonKind};
use crate::components::form::{select, SelectOption};
use crate::components::text_input::{
    bind_mouse_selection, text_field_with_callbacks, TextInputCallbacks, TextInputState,
};
use crate::components::timeline::timeline_state::TrackType;
use crate::components::title_bar::external_window_titlebar_with_icon;
use crate::theme::{self, Colors};
use sphere_plugin_host::{PluginFormat, RegistryPlugin};

const MAX_TRACK_COUNT: u32 = 128;

type VoidCb = Arc<dyn Fn(&(), &mut Window, &mut App) + 'static>;
type KindCb = Arc<dyn Fn(&AddTrackKind, &mut Window, &mut App) + 'static>;
type U32Cb = Arc<dyn Fn(&u32, &mut Window, &mut App) + 'static>;
type BoolCb = Arc<dyn Fn(&bool, &mut Window, &mut App) + 'static>;
type StringCb = Arc<dyn Fn(&String, &mut Window, &mut App) + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddTrackKind {
    Audio,
    Instrument,
    Midi,
    Automation,
    Folder,
    /// Legacy / menu-only kinds (not shown in the primary tab row).
    Plugin,
    Bus,
    Return,
    Group,
    Master,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    Mono,
    Stereo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddTrackSelectId {
    AudioFormat,
    InstrumentPlugin,
    Input,
    Output,
    MidiChannel,
}

impl AddTrackKind {
    pub fn label(self) -> &'static str {
        self.default_name_stem()
    }

    pub fn tab_label(self) -> &'static str {
        match self {
            Self::Audio => "Audio",
            Self::Instrument => "Instrument",
            Self::Midi => "MIDI",
            Self::Automation => "Automation",
            Self::Folder => "Folder",
            Self::Plugin => "Plugin",
            Self::Bus => "Bus",
            Self::Return => "Return",
            Self::Group => "Group",
            Self::Master => "Master",
        }
    }

    pub fn default_name_stem(self) -> &'static str {
        match self {
            Self::Audio => "Audio Track",
            Self::Instrument => "Instrument Track",
            Self::Midi => "MIDI Track",
            Self::Automation => "Automation Track",
            Self::Folder => "Folder Track",
            Self::Plugin => "Plugin Track",
            Self::Bus => "Bus Track",
            Self::Return => "Return Track",
            Self::Group => "Group Track",
            Self::Master => "Master Track",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Audio => assets::ICON_MIC_PATH,
            Self::Instrument => assets::ICON_CPU_PATH,
            Self::Midi => assets::ICON_MUSIC_PATH,
            Self::Automation => assets::ICON_AUTOMATION_PATH,
            Self::Folder => assets::ICON_FOLDER_PATH,
            Self::Plugin => assets::ICON_PLUG_PATH,
            Self::Bus => assets::ICON_GIT_MERGE_PATH,
            Self::Return => assets::ICON_CORNER_DOWN_LEFT_PATH,
            Self::Group => assets::ICON_GIT_FORK_PATH,
            Self::Master => assets::ICON_VOLUME_2_PATH,
        }
    }

    pub fn native_track_type(self) -> Option<TrackType> {
        match self {
            Self::Audio => Some(TrackType::Audio),
            Self::Instrument => Some(TrackType::Instrument),
            Self::Midi => Some(TrackType::Midi),
            Self::Bus => Some(TrackType::Bus),
            Self::Return => Some(TrackType::Return),
            Self::Automation | Self::Folder | Self::Plugin | Self::Group | Self::Master => None,
        }
    }

    pub fn default_input(self) -> &'static str {
        match self {
            Self::Midi | Self::Instrument => "All MIDI Inputs",
            Self::Audio => "System Input (Stereo)",
            _ => "None",
        }
    }

    /// Primary DAW-style tabs shown at the top of the dialog.
    pub fn primary_tabs() -> &'static [Self] {
        &[
            Self::Audio,
            Self::Instrument,
            Self::Midi,
            Self::Automation,
            Self::Folder,
        ]
    }

    /// Tabs visible for the current selection (primary + menu-only kinds).
    pub fn visible_tabs(selected: Self) -> Vec<Self> {
        let mut tabs = Self::primary_tabs().to_vec();
        for extra in [
            Self::Bus,
            Self::Return,
            Self::Plugin,
            Self::Group,
            Self::Master,
        ] {
            if selected == extra && !tabs.contains(&extra) {
                tabs.push(extra);
            }
        }
        tabs
    }
}

#[derive(Debug, Clone)]
pub struct AddTrackDialogState {
    pub is_open: bool,
    pub selected_kind: AddTrackKind,
    pub track_name: String,
    pub count: u32,
    pub auto_color: bool,
    pub color_index: usize,
    pub audio_format: AudioFormat,
    pub instrument_plugin_id: Option<String>,
    pub instrument_plugin_name: Option<String>,
    pub fx_chain: Option<String>,
    pub input_label: String,
    pub output_label: String,
    pub ascending_input: bool,
    pub ascending_output: bool,
    pub midi_channel_label: String,
    pub pack_folder: bool,
    pub channel_count: u32,
    pub arm_track: bool,
    pub monitor_mode: &'static str,
    pub next_number: usize,
    pub has_master_track: bool,
    pub base_track_count: usize,
}

pub(crate) fn add_track_debug_enabled() -> bool {
    static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FLAG.get_or_init(|| {
        std::env::var("FUTUREBOARD_ADD_TRACK_DEBUG")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    })
}

pub(crate) fn add_track_debug(message: &str) {
    if add_track_debug_enabled() {
        eprintln!("[Add Track] {message}");
    }
}

impl AddTrackDialogState {
    pub fn closed() -> Self {
        Self::open_for(0, false)
    }

    pub fn open_for(track_count: usize, has_master_track: bool) -> Self {
        let next_number = track_count.saturating_add(1);
        let kind = AddTrackKind::Audio;
        Self {
            is_open: true,
            selected_kind: kind,
            track_name: format!("{} {}", kind.default_name_stem(), next_number),
            count: 1,
            auto_color: true,
            color_index: track_count % Colors::TRACK_COLORS.len(),
            audio_format: AudioFormat::Stereo,
            instrument_plugin_id: None,
            instrument_plugin_name: None,
            fx_chain: None,
            input_label: kind.default_input().to_string(),
            output_label: "Main".to_string(),
            ascending_input: false,
            ascending_output: false,
            midi_channel_label: "All Channels".to_string(),
            pack_folder: false,
            channel_count: 2,
            arm_track: false,
            monitor_mode: "off",
            next_number,
            has_master_track,
            base_track_count: track_count,
        }
    }

    pub fn selected_color(&self) -> gpui::Rgba {
        track_color(self.color_index)
    }

    pub fn is_valid(&self) -> bool {
        let name_ok = !self.track_name.trim().is_empty();
        let count_ok = self.count >= 1 && self.count <= MAX_TRACK_COUNT;
        name_ok
            && count_ok
            && self.selected_kind.native_track_type().is_some()
            && !(self.selected_kind == AddTrackKind::Master && self.has_master_track)
    }

    pub fn sync_channel_count_from_format(&mut self) {
        self.channel_count = match self.audio_format {
            AudioFormat::Mono => 1,
            AudioFormat::Stereo => 2,
        };
    }
}

#[derive(Clone)]
pub struct AddTrackDialogCallbacks {
    pub on_close: VoidCb,
    pub on_confirm: VoidCb,
    pub on_select_kind: KindCb,
    pub on_count_delta: Arc<dyn Fn(&i32, &mut Window, &mut App) + 'static>,
    pub on_audio_format: Arc<dyn Fn(&AudioFormat, &mut Window, &mut App) + 'static>,
    pub on_color_index: U32Cb,
    pub on_auto_color: BoolCb,
    pub on_ascending_input: BoolCb,
    pub on_ascending_output: BoolCb,
    pub on_pack_folder: BoolCb,
    pub on_arm: BoolCb,
    pub on_monitor: Arc<dyn Fn(&String, &mut Window, &mut App) + 'static>,
    pub on_toggle_select: Arc<dyn Fn(&AddTrackSelectId, &mut Window, &mut App) + 'static>,
    pub on_instrument_plugin: StringCb,
    pub on_input_label: StringCb,
    pub on_output_label: StringCb,
    pub on_midi_channel_label: StringCb,
}

pub fn track_color(index: usize) -> gpui::Rgba {
    Colors::track_color_for_index(index)
}

fn kind_supported(kind: AddTrackKind, state: &AddTrackDialogState) -> bool {
    kind.native_track_type().is_some() && !(kind == AddTrackKind::Master && state.has_master_track)
}

fn icon(path: &'static str, size: f32, color: gpui::Rgba) -> impl IntoElement {
    svg().path(path).w(px(size)).h(px(size)).text_color(color)
}

fn select_box(text: impl Into<String>) -> impl IntoElement {
    let text = text.into();
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .w_full()
        .h(px(28.0))
        .rounded_md()
        .border(px(1.0))
        .border_color(Colors::border_subtle())
        .bg(Colors::surface_input())
        .px(px(8.0))
        .text_size(px(11.0))
        .text_color(Colors::text_secondary())
        .child(text)
        .child(icon(
            assets::ICON_CHEVRON_DOWN_PATH,
            10.0,
            Colors::text_faint(),
        ))
}

fn select_options(values: &[&'static str]) -> Vec<SelectOption> {
    values
        .iter()
        .map(|value| SelectOption::new(*value, *value))
        .collect()
}

fn plugin_format_label(format: PluginFormat) -> &'static str {
    match format {
        PluginFormat::Vst3 => "VST3",
        PluginFormat::Clap => "CLAP",
        PluginFormat::Au => "AU",
        PluginFormat::Lv2 => "LV2",
        PluginFormat::Unknown => "Unknown",
    }
}

fn instrument_plugin_options(plugins: &[RegistryPlugin]) -> Vec<SelectOption> {
    if plugins.is_empty() {
        return vec![SelectOption::new("", "No Instrument")
            .description("Open Plugin Manager to scan instruments")];
    }
    let mut options = vec![SelectOption::new("", "No Instrument")];
    options.extend(plugins.iter().map(|plugin| {
        let vendor = plugin.vendor.trim();
        let description = if vendor.is_empty() {
            plugin_format_label(plugin.format).to_string()
        } else {
            format!("{} / {}", vendor, plugin_format_label(plugin.format))
        };
        SelectOption::new(plugin.id.clone(), plugin.name.clone()).description(description)
    }));
    options
}

fn add_track_select_open(open_select: Option<AddTrackSelectId>, id: AddTrackSelectId) -> bool {
    open_select == Some(id)
}

fn check_row(
    id: &'static str,
    label: &'static str,
    checked: bool,
    enabled: bool,
    on_toggle: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let mut row = div()
        .id(id)
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(
            div()
                .w(px(12.0))
                .h(px(12.0))
                .rounded_sm()
                .border(px(1.0))
                .border_color(if checked {
                    Colors::accent_primary()
                } else {
                    Colors::border_subtle()
                })
                .bg(if checked {
                    Colors::accent_primary()
                } else {
                    Colors::surface_input()
                }),
        )
        .child(
            div()
                .text_size(px(11.0))
                .text_color(Colors::text_secondary())
                .child(label),
        );
    if enabled {
        row = row
            .cursor(gpui::CursorStyle::PointingHand)
            .hover(|s| s.bg(Colors::surface_hover()))
            .on_click(on_toggle);
    } else {
        row = row.opacity(0.45);
    }
    row
}

fn count_stepper(
    state: &AddTrackDialogState,
    callbacks: &AddTrackDialogCallbacks,
) -> impl IntoElement {
    let down = callbacks.on_count_delta.clone();
    let up = callbacks.on_count_delta.clone();
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .child(fb_stepper_button(
            "add-track-count-minus",
            "-",
            move |_, w, cx| down(&-1, w, cx),
        ))
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .min_w(px(48.0))
                .h(px(28.0))
                .px(px(10.0))
                .rounded_md()
                .border(px(1.0))
                .border_color(Colors::border_subtle())
                .bg(Colors::surface_input())
                .text_size(px(12.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(Colors::text_primary())
                .child(state.count.to_string()),
        )
        .child(fb_stepper_button(
            "add-track-count-plus",
            "+",
            move |_, w, cx| up(&1, w, cx),
        ))
}

fn type_tabs(state: &AddTrackDialogState, callbacks: &AddTrackDialogCallbacks) -> impl IntoElement {
    let tabs = AddTrackKind::visible_tabs(state.selected_kind);
    let mut row = div()
        .flex()
        .flex_row()
        .gap(px(4.0))
        .px(px(12.0))
        .py(px(8.0));
    for (i, kind) in tabs.iter().enumerate() {
        let active = state.selected_kind == *kind;
        let supported = kind_supported(*kind, state);
        let cb = callbacks.on_select_kind.clone();
        let k = *kind;
        let mut tab = div()
            .id(("add-track-tab", i))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(3.0))
            .min_w(px(72.0))
            .px(px(8.0))
            .py(px(6.0))
            .rounded_md()
            .border(px(1.0))
            .border_color(if active {
                Colors::border_accent()
            } else {
                Colors::border_subtle()
            })
            .bg(if active {
                Colors::accent_muted()
            } else {
                Colors::surface_input()
            })
            .opacity(if supported { 1.0 } else { 0.42 })
            .child(icon(
                kind.icon(),
                14.0,
                if active {
                    Colors::accent_primary()
                } else {
                    Colors::text_muted()
                },
            ))
            .child(
                div()
                    .text_size(px(10.0))
                    .font_weight(if active {
                        gpui::FontWeight::SEMIBOLD
                    } else {
                        gpui::FontWeight::NORMAL
                    })
                    .text_color(if active {
                        Colors::text_primary()
                    } else {
                        Colors::text_muted()
                    })
                    .child(kind.tab_label()),
            );
        if supported {
            tab = tab
                .cursor(gpui::CursorStyle::PointingHand)
                .hover(|s| s.bg(Colors::surface_hover()))
                .on_click(move |_, w, cx| cb(&k, w, cx));
        }
        row = row.child(tab);
    }
    row
}

fn color_row(state: &AddTrackDialogState, callbacks: &AddTrackDialogCallbacks) -> impl IntoElement {
    let auto_cb = callbacks.on_auto_color.clone();
    let auto_on = state.auto_color;
    let mut swatches = div().flex().flex_row().gap(px(5.0)).flex_wrap();
    for i in 0..Colors::TRACK_COLORS.len() {
        let cb = callbacks.on_color_index.clone();
        let active = !auto_on && i == state.color_index;
        let color = track_color(i);
        let mut sw = div()
            .id(("add-track-color", i))
            .w(px(18.0))
            .h(px(18.0))
            .rounded_full()
            .border(px(2.0))
            .border_color(color)
            .bg(if active {
                color
            } else {
                gpui::transparent_black().into()
            })
            .opacity(if auto_on {
                0.35
            } else if active {
                1.0
            } else {
                0.55
            });
        if !auto_on {
            sw = sw
                .cursor(gpui::CursorStyle::PointingHand)
                .on_click(move |_, w, cx| cb(&(i as u32), w, cx));
        }
        swatches = swatches.child(sw);
    }
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(fb_form_row("Color", swatches))
        .child(check_row(
            "add-track-auto-color",
            "Auto Color",
            auto_on,
            true,
            move |_, w, cx| auto_cb(&!auto_on, w, cx),
        ))
}

fn type_fields(
    state: &AddTrackDialogState,
    callbacks: &AddTrackDialogCallbacks,
    open_select: Option<AddTrackSelectId>,
    instrument_plugins: &[RegistryPlugin],
) -> gpui::AnyElement {
    let show_asc = state.count > 1;
    match state.selected_kind {
        AddTrackKind::Audio => {
            let fmt_cb = callbacks.on_audio_format.clone();
            let toggle_format = callbacks.on_toggle_select.clone();
            let input_cb = callbacks.on_input_label.clone();
            let output_cb = callbacks.on_output_label.clone();
            let toggle_input = callbacks.on_toggle_select.clone();
            let toggle_output = callbacks.on_toggle_select.clone();
            let asc_in = callbacks.on_ascending_input.clone();
            let asc_out = callbacks.on_ascending_output.clone();
            let arm_cb = callbacks.on_arm.clone();
            let asc_in_on = state.ascending_input;
            let asc_out_on = state.ascending_output;
            let arm_on = state.arm_track;
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(fb_form_row(
                    "Format",
                    select(
                        "add-track-format-select",
                        Some(match state.audio_format {
                            AudioFormat::Mono => "mono",
                            AudioFormat::Stereo => "stereo",
                        }),
                        "Select format...",
                        vec![
                            SelectOption::new("mono", "Mono"),
                            SelectOption::new("stereo", "Stereo"),
                        ],
                        add_track_select_open(open_select, AddTrackSelectId::AudioFormat),
                        false,
                        Arc::new(move |_, w, cx| {
                            toggle_format(&AddTrackSelectId::AudioFormat, w, cx)
                        }),
                        Arc::new(move |value, w, cx| {
                            let format = if value == "mono" {
                                AudioFormat::Mono
                            } else {
                                AudioFormat::Stereo
                            };
                            fmt_cb(&format, w, cx);
                        }),
                    ),
                ))
                .child(fb_form_row(
                    "FX Chain",
                    select_box(
                        state
                            .fx_chain
                            .clone()
                            .unwrap_or_else(|| "No Preset".to_string()),
                    ),
                ))
                .child(fb_form_row(
                    "Input",
                    select(
                        "add-track-input-select",
                        Some(state.input_label.as_str()),
                        "Select input...",
                        select_options(&["System Input (Stereo)", "Input 1", "Input 2", "None"]),
                        add_track_select_open(open_select, AddTrackSelectId::Input),
                        false,
                        Arc::new(move |_, w, cx| toggle_input(&AddTrackSelectId::Input, w, cx)),
                        Arc::new(move |value, w, cx| input_cb(value, w, cx)),
                    ),
                ))
                .child(fb_form_row(
                    "Output",
                    select(
                        "add-track-output-select",
                        Some(state.output_label.as_str()),
                        "Select output...",
                        select_options(&["Main", "Bus A", "None"]),
                        add_track_select_open(open_select, AddTrackSelectId::Output),
                        false,
                        Arc::new(move |_, w, cx| toggle_output(&AddTrackSelectId::Output, w, cx)),
                        Arc::new(move |value, w, cx| output_cb(value, w, cx)),
                    ),
                ))
                .when(show_asc, |col| {
                    col.child(check_row(
                        "add-track-asc-in",
                        "Ascending Input",
                        asc_in_on,
                        true,
                        move |_, w, cx| asc_in(&!asc_in_on, w, cx),
                    ))
                    .child(check_row(
                        "add-track-asc-out",
                        "Ascending Output",
                        asc_out_on,
                        true,
                        move |_, w, cx| asc_out(&!asc_out_on, w, cx),
                    ))
                })
                .child(check_row(
                    "add-track-arm",
                    "Arm for recording",
                    arm_on,
                    true,
                    move |_, w, cx| arm_cb(&!arm_on, w, cx),
                ))
                .into_any_element()
        }
        AddTrackKind::Instrument => {
            let asc_in = callbacks.on_ascending_input.clone();
            let instrument_cb = callbacks.on_instrument_plugin.clone();
            let toggle_instrument = callbacks.on_toggle_select.clone();
            let input_cb = callbacks.on_input_label.clone();
            let output_cb = callbacks.on_output_label.clone();
            let toggle_input = callbacks.on_toggle_select.clone();
            let toggle_output = callbacks.on_toggle_select.clone();
            let asc_in_on = state.ascending_input;
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(fb_form_row(
                    "Instrument",
                    select(
                        "add-track-instrument-plugin-select",
                        state.instrument_plugin_id.as_deref().or(Some("")),
                        "Select instrument...",
                        instrument_plugin_options(instrument_plugins),
                        add_track_select_open(open_select, AddTrackSelectId::InstrumentPlugin),
                        false,
                        Arc::new(move |_, w, cx| {
                            toggle_instrument(&AddTrackSelectId::InstrumentPlugin, w, cx)
                        }),
                        Arc::new(move |value, w, cx| instrument_cb(value, w, cx)),
                    ),
                ))
                .child(fb_form_row(
                    "MIDI Input",
                    select(
                        "add-track-instrument-input-select",
                        Some(state.input_label.as_str()),
                        "Select MIDI input...",
                        select_options(&["All MIDI Inputs", "MIDI Input 1", "MIDI Input 2", "None"]),
                        add_track_select_open(open_select, AddTrackSelectId::Input),
                        false,
                        Arc::new(move |_, w, cx| toggle_input(&AddTrackSelectId::Input, w, cx)),
                        Arc::new(move |value, w, cx| input_cb(value, w, cx)),
                    ),
                ))
                .child(fb_form_row(
                    "Output",
                    select(
                        "add-track-instrument-output-select",
                        Some(state.output_label.as_str()),
                        "Select output...",
                        select_options(&["Main", "Bus A", "None"]),
                        add_track_select_open(open_select, AddTrackSelectId::Output),
                        false,
                        Arc::new(move |_, w, cx| toggle_output(&AddTrackSelectId::Output, w, cx)),
                        Arc::new(move |value, w, cx| output_cb(value, w, cx)),
                    ),
                ))
                .child(fb_form_row("FX Chain", select_box("No Preset".to_string())))
                .when(show_asc, |col| {
                    col.child(check_row(
                        "add-track-asc-in",
                        "Ascending MIDI Input",
                        asc_in_on,
                        true,
                        move |_, w, cx| asc_in(&!asc_in_on, w, cx),
                    ))
                })
                .into_any_element()
        }
        AddTrackKind::Midi => {
            let asc_in = callbacks.on_ascending_input.clone();
            let input_cb = callbacks.on_input_label.clone();
            let output_cb = callbacks.on_output_label.clone();
            let midi_channel_cb = callbacks.on_midi_channel_label.clone();
            let toggle_input = callbacks.on_toggle_select.clone();
            let toggle_output = callbacks.on_toggle_select.clone();
            let toggle_channel = callbacks.on_toggle_select.clone();
            let asc_in_on = state.ascending_input;
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(fb_form_row(
                    "MIDI Input",
                    select(
                        "add-track-midi-input-select",
                        Some(state.input_label.as_str()),
                        "Select MIDI input...",
                        select_options(&["All MIDI Inputs", "MIDI Input 1", "MIDI Input 2", "None"]),
                        add_track_select_open(open_select, AddTrackSelectId::Input),
                        false,
                        Arc::new(move |_, w, cx| toggle_input(&AddTrackSelectId::Input, w, cx)),
                        Arc::new(move |value, w, cx| input_cb(value, w, cx)),
                    ),
                ))
                .child(fb_form_row("MIDI Output", select_box("None".to_string())))
                .child(fb_form_row(
                    "Channel",
                    select(
                        "add-track-midi-channel-select",
                        Some(state.midi_channel_label.as_str()),
                        "Select channel...",
                        select_options(&[
                            "All Channels",
                            "Channel 1",
                            "Channel 2",
                            "Channel 3",
                            "Channel 4",
                            "Channel 5",
                            "Channel 6",
                            "Channel 7",
                            "Channel 8",
                            "Channel 9",
                            "Channel 10",
                            "Channel 11",
                            "Channel 12",
                            "Channel 13",
                            "Channel 14",
                            "Channel 15",
                            "Channel 16",
                        ]),
                        add_track_select_open(open_select, AddTrackSelectId::MidiChannel),
                        false,
                        Arc::new(move |_, w, cx| {
                            toggle_channel(&AddTrackSelectId::MidiChannel, w, cx)
                        }),
                        Arc::new(move |value, w, cx| midi_channel_cb(value, w, cx)),
                    ),
                ))
                .child(fb_form_row(
                    "Output",
                    select(
                        "add-track-midi-output-select",
                        Some(state.output_label.as_str()),
                        "Select output...",
                        select_options(&["Main", "Bus A", "None"]),
                        add_track_select_open(open_select, AddTrackSelectId::Output),
                        false,
                        Arc::new(move |_, w, cx| toggle_output(&AddTrackSelectId::Output, w, cx)),
                        Arc::new(move |value, w, cx| output_cb(value, w, cx)),
                    ),
                ))
                .when(show_asc, |col| {
                    col.child(check_row(
                        "add-track-asc-in",
                        "Ascending Input",
                        asc_in_on,
                        true,
                        move |_, w, cx| asc_in(&!asc_in_on, w, cx),
                    ))
                })
                .into_any_element()
        }
        AddTrackKind::Automation => div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(fb_form_row("Target", select_box("None".to_string())))
            .child(
                div()
                    .text_size(px(10.0))
                    .text_color(Colors::text_faint())
                    .child(
                        "Automation lanes on existing tracks — dedicated automation tracks coming soon.",
                    ),
            )
            .into_any_element(),
        AddTrackKind::Folder => {
            let pack = callbacks.on_pack_folder.clone();
            let pack_on = state.pack_folder;
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(check_row(
                    "add-track-pack-folder",
                    "Pack Folder",
                    pack_on,
                    true,
                    move |_, w, cx| pack(&!pack_on, w, cx),
                ))
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(Colors::text_faint())
                        .child("Folder tracks for grouping — creation coming soon."),
                )
                .into_any_element()
        }
        AddTrackKind::Bus | AddTrackKind::Return => div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(fb_form_row("Output", select_box("Main".to_string())))
            .into_any_element(),
        _ => div()
            .text_size(px(10.0))
            .text_color(Colors::text_faint())
            .child("This track type is not available in Native yet.")
            .into_any_element(),
    }
}

/// Compact DAW-style Add Tracks form (external window body).
pub fn add_track_dialog_body(
    state: &AddTrackDialogState,
    track_name_input: &TextInputState,
    track_name_focused: bool,
    track_name_callbacks: TextInputCallbacks,
    open_select: Option<AddTrackSelectId>,
    instrument_plugins: &[RegistryPlugin],
    callbacks: AddTrackDialogCallbacks,
) -> gpui::Div {
    let confirm = callbacks.on_confirm.clone();
    let cancel = callbacks.on_close.clone();
    let valid = state.is_valid();
    let ok_label = if state.count == 1 {
        "OK".to_string()
    } else {
        format!("OK ×{}", state.count)
    };

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .child(type_tabs(state, &callbacks))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .px(px(12.0))
                .py(px(10.0))
                .gap(px(8.0))
                .child(fb_form_row(
                    "Name",
                    text_field_with_callbacks(
                        track_name_input,
                        track_name_focused,
                        track_name_callbacks,
                    ),
                ))
                .child(fb_form_row("Count", count_stepper(state, &callbacks)))
                .child(color_row(state, &callbacks))
                .child(type_fields(
                    state,
                    &callbacks,
                    open_select,
                    instrument_plugins,
                )),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .h(px(44.0))
                .px(px(12.0))
                .border_t(px(1.0))
                .border_color(Colors::border_subtle())
                .bg(Colors::surface_panel())
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(Colors::text_faint())
                        .child("Load Track Preset...")
                        .opacity(0.5),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(fb_button(
                            "add-track-cancel",
                            "Cancel",
                            FbButtonKind::Default,
                            true,
                            move |_, w, cx| cancel(&(), w, cx),
                        ))
                        .child(fb_button(
                            "add-track-ok",
                            ok_label,
                            FbButtonKind::Primary,
                            valid,
                            move |_, w, cx| {
                                if valid {
                                    confirm(&(), w, cx);
                                }
                            },
                        )),
                ),
        )
}
pub const ADD_TRACK_WINDOW_WIDTH: f32 = 520.0;
pub const ADD_TRACK_WINDOW_HEIGHT: f32 = 520.0;
pub const ADD_TRACK_WINDOW_MIN_WIDTH: f32 = 480.0;
pub const ADD_TRACK_WINDOW_MIN_HEIGHT: f32 = 440.0;

pub struct AddTrackWindow {
    pub state: AddTrackDialogState,
    track_name_input: TextInputState,
    open_select: Option<AddTrackSelectId>,
    instrument_plugins: Vec<RegistryPlugin>,
    focus_handle: FocusHandle,
    /// Called when the user confirms (creates tracks).
    on_confirm_request: Arc<dyn Fn(AddTrackDialogState, String, &mut App) + 'static>,
}

impl AddTrackWindow {
    pub fn new(
        initial_state: AddTrackDialogState,
        instrument_plugins: Vec<RegistryPlugin>,
        on_confirm_request: Arc<dyn Fn(AddTrackDialogState, String, &mut App) + 'static>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut track_name_input = TextInputState::new("add-track-window-name", cx.focus_handle());
        track_name_input.set_value(initial_state.track_name.clone());
        track_name_input.select_all();
        Self {
            state: initial_state,
            track_name_input,
            open_select: None,
            instrument_plugins,
            focus_handle: cx.focus_handle(),
            on_confirm_request,
        }
    }

    pub fn set_context(&mut self, kind: AddTrackKind, track_count: usize, has_master: bool) {
        let mut dialog = AddTrackDialogState::open_for(track_count, has_master);
        dialog.selected_kind = kind;
        dialog.input_label = kind.default_input().to_string();
        dialog.track_name = format!("{} {}", kind.default_name_stem(), dialog.next_number);
        dialog.instrument_plugin_id = None;
        dialog.instrument_plugin_name = None;
        add_track_debug(&format!(
            "dialog open kind={} count={}",
            kind.tab_label(),
            dialog.count
        ));
        self.track_name_input.set_value(dialog.track_name.clone());
        self.track_name_input.select_all();
        self.open_select = None;
        self.state = dialog;
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.state.is_valid() {
            return;
        }
        self.state.track_name = self.track_name_input.value.clone();
        add_track_debug(&format!(
            "confirm kind={} count={}",
            self.state.selected_kind.tab_label(),
            self.state.count
        ));
        let req = self.state.clone();
        let name = self.track_name_input.value.clone();
        let cb = self.on_confirm_request.clone();
        cb(req, name, cx);
        window.remove_window();
    }

    fn handle_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.track_name_input.is_focused(window) {
            let action = self
                .track_name_input
                .handle_key_with_clipboard(event, Some(cx));
            self.state.track_name = self.track_name_input.value.clone();
            match action {
                crate::components::text_input::TextInputAction::Submit => {
                    self.confirm(window, cx);
                }
                crate::components::text_input::TextInputAction::Cancel => window.remove_window(),
                crate::components::text_input::TextInputAction::Consumed
                | crate::components::text_input::TextInputAction::Pass => cx.notify(),
            }
            return;
        }

        match event.keystroke.key.as_str() {
            "escape" => {
                if self.open_select.take().is_some() {
                    cx.notify();
                } else {
                    window.remove_window();
                }
            }
            "enter" | "numpad_enter" => self.confirm(window, cx),
            _ => {}
        }
    }
}

impl Render for AddTrackWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let target = cx.entity().clone();
        let search_focused = self.track_name_input.is_focused(window);

        let callbacks = AddTrackDialogCallbacks {
            on_close: Arc::new(|_: &(), window: &mut Window, _cx: &mut App| window.remove_window()),
            on_confirm: Arc::new({
                let target = target.clone();
                move |_: &(), window, cx| {
                    let _ = target.update(cx, |this, cx| this.confirm(window, cx));
                }
            }),
            on_select_kind: Arc::new({
                let target = target.clone();
                move |kind: &AddTrackKind, _w, cx| {
                    let kind = *kind;
                    let _ = target.update(cx, |this, cx| {
                        this.state.selected_kind = kind;
                        this.state.input_label = kind.default_input().to_string();
                        this.state.track_name =
                            format!("{} {}", kind.default_name_stem(), this.state.next_number);
                        this.open_select = None;
                        this.track_name_input
                            .set_value(this.state.track_name.clone());
                        this.track_name_input.select_all();
                        add_track_debug(&format!("selected kind={}", kind.tab_label()));
                        cx.notify();
                    });
                }
            }),
            on_count_delta: Arc::new({
                let target = target.clone();
                move |delta: &i32, _w, cx| {
                    let delta = *delta;
                    let _ = target.update(cx, |this, cx| {
                        let current = this.state.count as i32;
                        this.state.count =
                            (current + delta).clamp(1, MAX_TRACK_COUNT as i32) as u32;
                        cx.notify();
                    });
                }
            }),
            on_audio_format: Arc::new({
                let target = target.clone();
                move |format: &AudioFormat, _w, cx| {
                    let format = *format;
                    let _ = target.update(cx, |this, cx| {
                        this.state.audio_format = format;
                        this.state.sync_channel_count_from_format();
                        this.open_select = None;
                        cx.notify();
                    });
                }
            }),
            on_color_index: Arc::new({
                let target = target.clone();
                move |index: &u32, _w, cx| {
                    let index = *index as usize;
                    let _ = target.update(cx, |this, cx| {
                        this.state.color_index = index;
                        this.state.auto_color = false;
                        cx.notify();
                    });
                }
            }),
            on_auto_color: Arc::new({
                let target = target.clone();
                move |on: &bool, _w, cx| {
                    let on = *on;
                    let _ = target.update(cx, |this, cx| {
                        this.state.auto_color = on;
                        cx.notify();
                    });
                }
            }),
            on_ascending_input: Arc::new({
                let target = target.clone();
                move |on: &bool, _w, cx| {
                    let on = *on;
                    let _ = target.update(cx, |this, cx| {
                        this.state.ascending_input = on;
                        cx.notify();
                    });
                }
            }),
            on_ascending_output: Arc::new({
                let target = target.clone();
                move |on: &bool, _w, cx| {
                    let on = *on;
                    let _ = target.update(cx, |this, cx| {
                        this.state.ascending_output = on;
                        cx.notify();
                    });
                }
            }),
            on_pack_folder: Arc::new({
                let target = target.clone();
                move |on: &bool, _w, cx| {
                    let on = *on;
                    let _ = target.update(cx, |this, cx| {
                        this.state.pack_folder = on;
                        cx.notify();
                    });
                }
            }),
            on_arm: Arc::new({
                let target = target.clone();
                move |armed: &bool, _w, cx| {
                    let armed = *armed;
                    let _ = target.update(cx, |this, cx| {
                        this.state.arm_track = armed;
                        cx.notify();
                    });
                }
            }),
            on_monitor: Arc::new({
                let target = target.clone();
                move |mode: &String, _w, cx| {
                    let mode = match mode.as_str() {
                        "auto" => "auto",
                        "in" => "in",
                        _ => "off",
                    };
                    let _ = target.update(cx, |this, cx| {
                        this.state.monitor_mode = mode;
                        cx.notify();
                    });
                }
            }),
            on_toggle_select: Arc::new({
                let target = target.clone();
                move |select_id: &AddTrackSelectId, _w, cx| {
                    let select_id = *select_id;
                    let _ = target.update(cx, |this, cx| {
                        this.open_select = if this.open_select == Some(select_id) {
                            None
                        } else {
                            Some(select_id)
                        };
                        if crate::ui_debug_enabled() {
                            eprintln!(
                                "[ui] select_toggle id={select_id:?} open={:?}",
                                this.open_select
                            );
                        }
                        cx.notify();
                    });
                }
            }),
            on_instrument_plugin: Arc::new({
                let target = target.clone();
                move |value: &String, _w, cx| {
                    let value = value.clone();
                    let _ = target.update(cx, |this, cx| {
                        if value.is_empty() {
                            this.state.instrument_plugin_id = None;
                            this.state.instrument_plugin_name = None;
                        } else {
                            this.state.instrument_plugin_name = this
                                .instrument_plugins
                                .iter()
                                .find(|plugin| plugin.id == value)
                                .map(|plugin| plugin.name.clone());
                            this.state.instrument_plugin_id = Some(value);
                        }
                        this.open_select = None;
                        cx.notify();
                    });
                }
            }),
            on_input_label: Arc::new({
                let target = target.clone();
                move |value: &String, _w, cx| {
                    let value = value.clone();
                    let _ = target.update(cx, |this, cx| {
                        this.state.input_label = value;
                        this.open_select = None;
                        cx.notify();
                    });
                }
            }),
            on_output_label: Arc::new({
                let target = target.clone();
                move |value: &String, _w, cx| {
                    let value = value.clone();
                    let _ = target.update(cx, |this, cx| {
                        this.state.output_label = value;
                        this.open_select = None;
                        cx.notify();
                    });
                }
            }),
            on_midi_channel_label: Arc::new({
                let target = target.clone();
                move |value: &String, _w, cx| {
                    let value = value.clone();
                    let _ = target.update(cx, |this, cx| {
                        this.state.midi_channel_label = value;
                        this.open_select = None;
                        cx.notify();
                    });
                }
            }),
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .font_family(theme::FONT_FAMILY)
            .bg(Colors::surface_base())
            .overflow_hidden()
            .capture_key_down({
                let target = target.clone();
                move |event, window, cx| {
                    let _ = target.update(cx, |this, cx| this.handle_key(event, window, cx));
                }
            })
            .child(div().w(px(0.0)).h(px(0.0)).track_focus(&self.focus_handle))
            .child(external_window_titlebar_with_icon(
                Some(assets::ICON_PLUS_PATH),
                "Add Tracks",
                "add-track-window-close",
                move |window, _cx| window.remove_window(),
            ))
            .child(add_track_dialog_body(
                &self.state,
                &self.track_name_input,
                search_focused,
                bind_mouse_selection(cx.entity().clone(), |this| &mut this.track_name_input),
                self.open_select,
                &self.instrument_plugins,
                callbacks,
            ))
    }
}

pub fn open_add_track_window(
    owner_bounds: Bounds<gpui::Pixels>,
    kind: AddTrackKind,
    track_count: usize,
    has_master_track: bool,
    instrument_plugins: Vec<RegistryPlugin>,
    on_confirm_request: Arc<dyn Fn(AddTrackDialogState, String, &mut App) + 'static>,
    cx: &mut App,
) -> Result<WindowHandle<AddTrackWindow>, String> {
    let parent_x: f32 = owner_bounds.origin.x.into();
    let parent_y: f32 = owner_bounds.origin.y.into();
    let parent_w: f32 = owner_bounds.size.width.into();
    let parent_h: f32 = owner_bounds.size.height.into();
    let origin = Point {
        x: px(parent_x + ((parent_w - ADD_TRACK_WINDOW_WIDTH) / 2.0).max(24.0)),
        y: px(parent_y + ((parent_h - ADD_TRACK_WINDOW_HEIGHT) / 2.0).max(24.0)),
    };

    let mut state = AddTrackDialogState::open_for(track_count, has_master_track);
    state.selected_kind = kind;
    state.input_label = kind.default_input().to_string();
    state.track_name = format!("{} {}", kind.default_name_stem(), state.next_number);
    add_track_debug(&format!(
        "open window kind={} track_count={}",
        kind.tab_label(),
        track_count
    ));

    let mut options = crate::platform_chrome::external_dialog_window_options_partial();
    options.window_bounds = Some(WindowBounds::Windowed(Bounds {
        origin,
        size: size(px(ADD_TRACK_WINDOW_WIDTH), px(ADD_TRACK_WINDOW_HEIGHT)),
    }));
    options.kind = WindowKind::Floating;
    options.is_resizable = true;
    options.is_minimizable = false;
    options.window_background = WindowBackgroundAppearance::Transparent;
    options.window_min_size = Some(size(
        px(ADD_TRACK_WINDOW_MIN_WIDTH),
        px(ADD_TRACK_WINDOW_MIN_HEIGHT),
    ));

    cx.open_window(options, |_window, cx| {
        cx.new(|cx| AddTrackWindow::new(state, instrument_plugins, on_confirm_request, cx))
    })
    .map_err(|e| e.to_string())
}
