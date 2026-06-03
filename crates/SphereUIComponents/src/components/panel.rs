//! Inspector panel — the main detail/edit surface for the current selection.
//!
//! The Inspector renders one of several "targets" derived fresh from the live
//! [`TimelineState`] each frame (see [`InspectorTarget`]). It never stores a
//! duplicate of track/clip state: the panel reads the same `TrackState` the
//! TrackHeader and Mixer read, so any edit made here is reflected everywhere.
//!
//! Phase A establishes the redesigned shell (section-based layout, richer
//! detail, project-summary empty state) and the target model. Editing controls
//! and their callbacks are layered on in later phases without changing this
//! file's read-render structure.

use std::sync::Arc;

use gpui::prelude::FluentBuilder;
use gpui::{
    div, px, App, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement,
    Styled, Window,
};

use crate::components::controls::{
    fb_button, fb_form_row, fb_section_header, fb_segmented_button, FbButtonKind,
};
use crate::components::slider::slider;
use crate::components::text_input::{text_field, TextInputState};
use crate::components::timeline::timeline_state::{
    volume, ClipType, InsertLoadStatus, InsertSlotState, TrackAudioFormat, TrackInputRouting,
    TrackMidiInputRouting, TrackOutputRouting, TrackState, TrackType,
};
use crate::theme::Colors;

type StrCb = Arc<dyn Fn(&String, &mut Window, &mut App) + 'static>;
type StrF32Cb = Arc<dyn Fn(&(String, f32), &mut Window, &mut App) + 'static>;
type ColorCb = Arc<dyn Fn(&(String, gpui::Rgba), &mut Window, &mut App) + 'static>;
type InputRoutingCb = Arc<dyn Fn(&(String, TrackInputRouting), &mut Window, &mut App) + 'static>;
type OutputRoutingCb = Arc<dyn Fn(&(String, TrackOutputRouting), &mut Window, &mut App) + 'static>;
type AudioFormatCb = Arc<dyn Fn(&(String, TrackAudioFormat), &mut Window, &mut App) + 'static>;
type MidiInputCb = Arc<dyn Fn(&(String, TrackMidiInputRouting), &mut Window, &mut App) + 'static>;
type MidiChannelCb = Arc<dyn Fn(&(String, Option<u8>), &mut Window, &mut App) + 'static>;
type InsertPairCb = Arc<dyn Fn(&(String, String), &mut Window, &mut App) + 'static>;
type InsertOpenCb = Arc<dyn Fn(&(String, usize, String), &mut Window, &mut App) + 'static>;
type InsertMoveCb = Arc<dyn Fn(&(String, String, bool), &mut Window, &mut App) + 'static>;
type InsertPickerCb = Arc<dyn Fn(&(String, usize, bool), &mut Window, &mut App) + 'static>;
type ClipF32Cb = Arc<dyn Fn(&(String, f32), &mut Window, &mut App) + 'static>;

/// Edit callbacks handed to the Inspector. Built by the layout
/// (`build_inspector_callbacks`) and dispatched to the shared `TimelineState`.
#[derive(Clone)]
pub struct InspectorCallbacks {
    pub on_volume: StrF32Cb,
    pub on_pan: StrF32Cb,
    pub on_toggle_mute: StrCb,
    pub on_toggle_solo: StrCb,
    pub on_toggle_arm: StrCb,
    pub on_toggle_input: StrCb,
    pub on_set_color: ColorCb,
    pub on_set_input_routing: InputRoutingCb,
    pub on_set_output_routing: OutputRoutingCb,
    pub on_set_audio_format: AudioFormatCb,
    pub on_set_midi_input: MidiInputCb,
    pub on_set_midi_channel: MidiChannelCb,
    pub on_open_insert_picker: InsertPickerCb,
    pub on_remove_insert: InsertPairCb,
    pub on_toggle_insert_bypass: InsertPairCb,
    pub on_toggle_insert_enabled: InsertPairCb,
    pub on_move_insert: InsertMoveCb,
    pub on_open_insert_editor: InsertOpenCb,
    pub on_set_clip_start: ClipF32Cb,
    pub on_set_clip_length: ClipF32Cb,
    pub on_open_clip_bottom_editor: StrCb,
    pub on_open_clip_external_midi_editor: StrCb,
}

/// Width of the docked Inspector column. Mirrors the constant in
/// `studio_render.rs` (`INSPECTOR_WIDTH`).
pub const INSPECTOR_WIDTH: f32 = 292.0;

/// `FUTUREBOARD_INSPECTOR_DEBUG=1` gates verbose Inspector edit logging.
pub fn inspector_debug_enabled() -> bool {
    static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FLAG.get_or_init(|| {
        std::env::var("FUTUREBOARD_INSPECTOR_DEBUG")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    })
}

/// Emit an Inspector debug line when `FUTUREBOARD_INSPECTOR_DEBUG=1`.
pub fn inspector_debug(message: &str) {
    if inspector_debug_enabled() {
        eprintln!("[inspector] {message}");
    }
}

/// Lightweight projection of the currently selected clip, built by the layout
/// from `TimelineState`. The inspector only needs a read-only summary.
pub struct SelectedClipSummary<'a> {
    pub clip_id: &'a str,
    pub track_id: &'a str,
    pub name: &'a str,
    pub start_beat: f32,
    pub duration_beats: f32,
    pub muted: bool,
    pub gain: f32,
    pub source_duration_seconds: Option<f64>,
    pub source_path: Option<&'a str>,
    pub note_count: Option<usize>,
    pub kind: &'static str,
    pub track_name: &'a str,
}

/// What the Inspector is currently editing. Resolved fresh from the live
/// selection every render — the Inspector must never guess from stale state,
/// and if the referenced object has been deleted the resolver falls back to a
/// lower-priority target (ultimately [`InspectorTarget::None`]).
///
/// Selection priority (highest first):
/// 1. `MidiNotes` — resolved inside the Piano Roll window, not here.
/// 2. `AutomationPoints` — when points are selected on the focused lane.
/// 3. `Clip` — when a clip is selected.
/// 4. `PluginInsert` — when an insert slot is selected.
/// 5. `Track` — when only a track is selected.
/// 6. `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InspectorTarget {
    None,
    Track {
        track_id: String,
    },
    Clip {
        track_id: String,
        clip_id: String,
    },
    MidiNotes {
        track_id: String,
        clip_id: String,
        note_ids: Vec<u64>,
    },
    AutomationPoints {
        track_id: String,
        lane_id: String,
        point_ids: Vec<u64>,
    },
    PluginInsert {
        track_id: String,
        insert_id: String,
    },
}

/// Stable badge label for a resolved target, used in the Inspector header.
pub fn target_badge(target: &InspectorTarget, tracks: &[TrackState]) -> &'static str {
    match target {
        InspectorTarget::None => "",
        InspectorTarget::Track { track_id } => tracks
            .iter()
            .find(|t| &t.id == track_id)
            .map(|t| track_type_badge(t.track_type))
            .unwrap_or(""),
        InspectorTarget::Clip { .. } | InspectorTarget::MidiNotes { .. } => "Clip",
        InspectorTarget::AutomationPoints { .. } => "Automation",
        InspectorTarget::PluginInsert { .. } => "Plugin",
    }
}

fn track_type_badge(t: TrackType) -> &'static str {
    match t {
        TrackType::Audio => "Audio Track",
        TrackType::Midi => "MIDI Track",
        TrackType::Instrument => "Instrument Track",
        TrackType::Bus => "Bus",
        TrackType::Return => "Return",
        TrackType::Master => "Master",
    }
}

/// Legacy entry point — kept so any existing call sites still compile. Returns
/// an empty placeholder identical to the pre-state version.
pub fn right_panel() -> impl IntoElement {
    inspector_shell().child(no_selection(0))
}

/// Inspector driven by the live selection. Renders one of:
/// 1. Clip details when a clip is selected.
/// 2. Track details when only a track is selected.
/// 3. "No Selection" placeholder (with a small project summary) otherwise.
pub fn inspector_panel<'a>(
    tracks: &'a [TrackState],
    selected_track_id: Option<&str>,
    selected_clip_id: Option<&str>,
    clip_summary: Option<SelectedClipSummary<'a>>,
    name_input: &TextInputState,
    name_focused: bool,
    clip_name_input: &TextInputState,
    clip_name_focused: bool,
    callbacks: &InspectorCallbacks,
) -> impl IntoElement {
    let body: gpui::AnyElement = if let Some(clip) = clip_summary {
        clip_inspector(clip, clip_name_input, clip_name_focused, callbacks).into_any_element()
    } else if let Some(tid) = selected_track_id {
        match tracks.iter().find(|t| t.id == tid) {
            Some(t) => track_inspector(t, name_input, name_focused, callbacks).into_any_element(),
            None => no_selection(tracks.len()).into_any_element(),
        }
    } else {
        let _ = selected_clip_id; // currently only used via clip_summary
        no_selection(tracks.len()).into_any_element()
    };

    inspector_shell().child(body)
}

fn inspector_shell() -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .w(px(INSPECTOR_WIDTH))
        .h_full()
        .bg(Colors::surface_panel())
        .border_l(px(1.0))
        .border_color(Colors::border_subtle())
        .child(
            div()
                .flex_shrink_0()
                .px(px(10.0))
                .py(px(8.0))
                .border_b(px(1.0))
                .border_color(Colors::border_subtle())
                .child(
                    div()
                        .text_color(Colors::text_primary())
                        .text_xs()
                        .font_weight(gpui::FontWeight::BOLD)
                        .child("Inspector"),
                ),
        )
}

/// Scrollable body wrapper shared by every populated inspector view.
fn scroll_body() -> gpui::Stateful<gpui::Div> {
    div()
        .id("inspector-scroll")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .px(px(10.0))
        .py(px(10.0))
        .gap(px(12.0))
}

fn no_selection(track_count: usize) -> impl IntoElement {
    div()
        .flex_1()
        .flex()
        .flex_col()
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(4.0))
                .px(px(16.0))
                .child(
                    div()
                        .text_color(Colors::text_muted())
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child("No Selection"),
                )
                .child(
                    div()
                        .text_color(Colors::text_faint())
                        .text_size(px(10.5))
                        .text_center()
                        .child("Select a track, clip, note, or plugin to edit its details."),
                ),
        )
        .child(
            div()
                .flex_shrink_0()
                .px(px(10.0))
                .py(px(10.0))
                .border_t(px(1.0))
                .border_color(Colors::border_subtle())
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(fb_section_header("PROJECT"))
                .child(kv_row("Tracks", track_count.to_string())),
        )
}

fn kv_row(key: impl Into<String>, value: impl Into<String>) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .justify_between()
        .items_center()
        .gap(px(8.0))
        .py(px(3.0))
        .child(
            div()
                .flex_shrink_0()
                .text_size(px(10.5))
                .text_color(Colors::text_muted())
                .child(key.into()),
        )
        .child(
            div()
                .min_w(px(0.0))
                .truncate()
                .text_size(px(11.0))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(Colors::text_primary())
                .child(value.into()),
        )
}

/// Header strip shown at the top of every populated inspector: color chip,
/// title, and a type badge.
fn inspector_header(
    color: gpui::Rgba,
    title: impl Into<String>,
    badge: &'static str,
) -> impl IntoElement {
    let mut row = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(div().w(px(4.0)).h(px(22.0)).rounded_sm().bg(color))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .text_size(px(13.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(Colors::text_primary())
                .child(title.into()),
        );
    if !badge.is_empty() {
        row = row.child(
            div()
                .flex_shrink_0()
                .px(px(7.0))
                .py(px(2.0))
                .rounded_sm()
                .bg(Colors::with_alpha(Colors::accent_primary(), 0.16))
                .text_size(px(9.0))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(Colors::accent_primary())
                .child(badge),
        );
    }
    row
}

fn format_pan(pan: f32) -> String {
    if pan.abs() < 0.01 {
        "Center".to_string()
    } else if pan < 0.0 {
        format!("L {}", (pan * -100.0).round().clamp(1.0, 100.0) as i32)
    } else {
        format!("R {}", (pan * 100.0).round().clamp(1.0, 100.0) as i32)
    }
}

/// Clickable M/S/R/I-style state badge.
fn toggle_badge(
    id: impl Into<gpui::ElementId>,
    label: &'static str,
    active: bool,
    accent: gpui::Rgba,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let (bg, fg) = if active {
        (accent, Colors::on_accent())
    } else {
        (
            Colors::with_alpha(Colors::text_primary(), 0.05),
            Colors::text_secondary(),
        )
    };
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .min_w(px(28.0))
        .px(px(8.0))
        .py(px(4.0))
        .rounded_sm()
        .bg(bg)
        .text_color(fg)
        .text_size(px(9.5))
        .font_weight(gpui::FontWeight::BOLD)
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|s| s.opacity(0.85))
        .on_click(on_click)
        .child(label)
}

/// Clickable track-color palette. Highlights the active swatch.
fn color_palette(track_id: String, current: gpui::Rgba, on_set: ColorCb) -> impl IntoElement {
    let mut row = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap(px(5.0))
        .items_center();
    for i in 0..Colors::TRACK_COLORS.len() {
        let color = Colors::track_color_for_index(i);
        let active = color == current;
        let cb = on_set.clone();
        let tid = track_id.clone();
        row = row.child(
            div()
                .id(("inspector-color", i))
                .w(px(15.0))
                .h(px(15.0))
                .rounded_full()
                .border(px(2.0))
                .border_color(color)
                .bg(if active {
                    color
                } else {
                    gpui::transparent_black().into()
                })
                .opacity(if active { 1.0 } else { 0.6 })
                .cursor(gpui::CursorStyle::PointingHand)
                .on_click(move |_, w, cx| cb(&(tid.clone(), color), w, cx)),
        );
    }
    row
}

fn option_button(
    id: impl Into<gpui::ElementId>,
    label: impl Into<String>,
    active: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    fb_segmented_button(id, label, active, on_click)
}

fn format_selector(track: &TrackState, callbacks: &InspectorCallbacks) -> impl IntoElement {
    let tid_mono = track.id.clone();
    let tid_stereo = track.id.clone();
    let cb_mono = callbacks.on_set_audio_format.clone();
    let cb_stereo = callbacks.on_set_audio_format.clone();
    div()
        .flex()
        .flex_row()
        .gap(px(4.0))
        .child(option_button(
            "inspector-format-mono",
            TrackAudioFormat::Mono.label(),
            track.routing.audio_format == TrackAudioFormat::Mono,
            move |_, w, cx| cb_mono(&(tid_mono.clone(), TrackAudioFormat::Mono), w, cx),
        ))
        .child(option_button(
            "inspector-format-stereo",
            TrackAudioFormat::Stereo.label(),
            track.routing.audio_format == TrackAudioFormat::Stereo,
            move |_, w, cx| cb_stereo(&(tid_stereo.clone(), TrackAudioFormat::Stereo), w, cx),
        ))
}

fn audio_input_selector(track: &TrackState, callbacks: &InspectorCallbacks) -> impl IntoElement {
    let tid = track.id.clone();
    let cb = callbacks.on_set_input_routing.clone();
    // TODO(device-enumeration): populate real audio input device/channel options
    // once DAUx device discovery is available in TimelineState.
    div().flex().flex_row().gap(px(4.0)).child(option_button(
        "inspector-input-none",
        "None",
        track.routing.input == TrackInputRouting::None,
        move |_, w, cx| cb(&(tid.clone(), TrackInputRouting::None), w, cx),
    ))
}

fn output_selector(track: &TrackState, callbacks: &InspectorCallbacks) -> impl IntoElement {
    let tid_main = track.id.clone();
    let tid_none = track.id.clone();
    let cb_main = callbacks.on_set_output_routing.clone();
    let cb_none = callbacks.on_set_output_routing.clone();
    // TODO(device-enumeration): add real hardware outputs and bus targets when
    // the routing/device registry is exposed to the Inspector.
    div()
        .flex()
        .flex_row()
        .gap(px(4.0))
        .child(option_button(
            "inspector-output-main",
            "Main",
            track.routing.output == TrackOutputRouting::Main,
            move |_, w, cx| cb_main(&(tid_main.clone(), TrackOutputRouting::Main), w, cx),
        ))
        .child(option_button(
            "inspector-output-none",
            "None",
            track.routing.output == TrackOutputRouting::None,
            move |_, w, cx| cb_none(&(tid_none.clone(), TrackOutputRouting::None), w, cx),
        ))
}

fn midi_input_selector(track: &TrackState, callbacks: &InspectorCallbacks) -> impl IntoElement {
    let tid_all = track.id.clone();
    let tid_none = track.id.clone();
    let cb_all = callbacks.on_set_midi_input.clone();
    let cb_none = callbacks.on_set_midi_input.clone();
    // TODO(device-enumeration): populate real MIDI input devices when available.
    div()
        .flex()
        .flex_row()
        .gap(px(4.0))
        .child(option_button(
            "inspector-midi-input-all",
            "All",
            track.routing.midi_input == TrackMidiInputRouting::AllInputs,
            move |_, w, cx| cb_all(&(tid_all.clone(), TrackMidiInputRouting::AllInputs), w, cx),
        ))
        .child(option_button(
            "inspector-midi-input-none",
            "None",
            track.routing.midi_input == TrackMidiInputRouting::None,
            move |_, w, cx| cb_none(&(tid_none.clone(), TrackMidiInputRouting::None), w, cx),
        ))
}

fn midi_channel_selector(track: &TrackState, callbacks: &InspectorCallbacks) -> impl IntoElement {
    let mut row = div().flex().flex_row().flex_wrap().gap(px(4.0));
    let tid_all = track.id.clone();
    let cb_all = callbacks.on_set_midi_channel.clone();
    row = row.child(option_button(
        "inspector-midi-channel-all",
        "All",
        track.routing.midi_channel.is_none(),
        move |_, w, cx| cb_all(&(tid_all.clone(), None), w, cx),
    ));
    for channel in 1..=16u8 {
        let tid = track.id.clone();
        let cb = callbacks.on_set_midi_channel.clone();
        row = row.child(option_button(
            ("inspector-midi-channel", channel as usize),
            channel.to_string(),
            track.routing.midi_channel == Some(channel),
            move |_, w, cx| cb(&(tid.clone(), Some(channel)), w, cx),
        ));
    }
    row
}

fn routing_section(track: &TrackState, callbacks: &InspectorCallbacks) -> impl IntoElement {
    let mut section = div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(fb_section_header("ROUTING"));

    match track.track_type {
        TrackType::Audio => {
            section = section
                .child(fb_form_row("Format", format_selector(track, callbacks)))
                .child(fb_form_row("Input", audio_input_selector(track, callbacks)))
                .child(fb_form_row("Output", output_selector(track, callbacks)));
        }
        TrackType::Instrument => {
            section = section
                .child(fb_form_row(
                    "MIDI Input",
                    midi_input_selector(track, callbacks),
                ))
                .child(fb_form_row(
                    "MIDI Ch",
                    midi_channel_selector(track, callbacks),
                ))
                .child(fb_form_row("Output", output_selector(track, callbacks)));
        }
        TrackType::Midi => {
            section = section
                .child(fb_form_row(
                    "MIDI Input",
                    midi_input_selector(track, callbacks),
                ))
                .child(fb_form_row(
                    "MIDI Ch",
                    midi_channel_selector(track, callbacks),
                ))
                .child(fb_form_row("MIDI Out", output_selector(track, callbacks)));
        }
        TrackType::Bus | TrackType::Return | TrackType::Master => {
            section = section.child(fb_form_row("Output", output_selector(track, callbacks)));
        }
    }

    section
}

fn compact_action_button(
    id: impl Into<gpui::ElementId>,
    label: impl Into<String>,
    enabled: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    fb_button(id, label, FbButtonKind::Default, enabled, on_click)
}

fn plugin_format_label(slot: &InsertSlotState) -> &'static str {
    slot.plugin_format.map(|fmt| fmt.label()).unwrap_or("-")
}

fn plugin_slot_name(slot: Option<&InsertSlotState>, empty: &'static str) -> String {
    slot.map(|slot| slot.display_name.clone())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| empty.to_string())
}

fn plugin_state_label(slot: &InsertSlotState) -> String {
    match &slot.load_status {
        InsertLoadStatus::Empty => "Empty".to_string(),
        InsertLoadStatus::Loading => "Loading".to_string(),
        InsertLoadStatus::Ready if slot.bypassed => "Bypassed".to_string(),
        InsertLoadStatus::Ready if !slot.enabled => "Disabled".to_string(),
        InsertLoadStatus::Ready => "Ready".to_string(),
        InsertLoadStatus::Failed(message) => format!("Missing / Failed: {message}"),
        InsertLoadStatus::Disabled => "Disabled".to_string(),
    }
}

fn format_chip(label: &'static str) -> impl IntoElement {
    div()
        .flex_shrink_0()
        .px(px(6.0))
        .py(px(2.0))
        .rounded_sm()
        .border(px(1.0))
        .border_color(Colors::border_subtle())
        .bg(Colors::surface_input())
        .text_size(px(9.0))
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(Colors::text_secondary())
        .child(label)
}

fn insert_action_row(
    track_id: &str,
    slot: &InsertSlotState,
    slot_index: usize,
    callbacks: &InspectorCallbacks,
    can_move_up: bool,
    can_move_down: bool,
    is_instrument: bool,
) -> impl IntoElement {
    let track_open = track_id.to_string();
    let slot_open = slot.id.clone();
    let slot_open_index = slot_index;
    let open = callbacks.on_open_insert_editor.clone();
    let track_replace = track_id.to_string();
    let replace = callbacks.on_open_insert_picker.clone();
    let track_bypass = track_id.to_string();
    let slot_bypass = slot.id.clone();
    let bypass = callbacks.on_toggle_insert_bypass.clone();
    let track_enable = track_id.to_string();
    let slot_enable = slot.id.clone();
    let enable = callbacks.on_toggle_insert_enabled.clone();
    let track_remove = track_id.to_string();
    let slot_remove = slot.id.clone();
    let remove = callbacks.on_remove_insert.clone();
    let track_up = track_id.to_string();
    let slot_up = slot.id.clone();
    let move_up = callbacks.on_move_insert.clone();
    let track_down = track_id.to_string();
    let slot_down = slot.id.clone();
    let move_down = callbacks.on_move_insert.clone();

    div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap(px(4.0))
        .child(compact_action_button(
            "insert-open-editor",
            "Open",
            true,
            move |_, w, cx| {
                open(
                    &(track_open.clone(), slot_open_index, slot_open.clone()),
                    w,
                    cx,
                )
            },
        ))
        .child(compact_action_button(
            "insert-replace",
            "Replace",
            true,
            move |_, w, cx| replace(&(track_replace.clone(), slot_index, is_instrument), w, cx),
        ))
        .child(compact_action_button(
            "insert-bypass",
            if slot.bypassed { "Unbypass" } else { "Bypass" },
            true,
            move |_, w, cx| bypass(&(track_bypass.clone(), slot_bypass.clone()), w, cx),
        ))
        .child(compact_action_button(
            "insert-enable",
            if slot.enabled { "Disable" } else { "Enable" },
            true,
            move |_, w, cx| enable(&(track_enable.clone(), slot_enable.clone()), w, cx),
        ))
        .child(compact_action_button(
            "insert-remove",
            "Remove",
            true,
            move |_, w, cx| remove(&(track_remove.clone(), slot_remove.clone()), w, cx),
        ))
        .child(compact_action_button(
            "insert-move-up",
            "Up",
            can_move_up,
            move |_, w, cx| move_up(&(track_up.clone(), slot_up.clone(), true), w, cx),
        ))
        .child(compact_action_button(
            "insert-move-down",
            "Down",
            can_move_down,
            move |_, w, cx| move_down(&(track_down.clone(), slot_down.clone(), false), w, cx),
        ))
}

fn plugin_slot_row(
    track: &TrackState,
    slot: &InsertSlotState,
    slot_index: usize,
    display_index: usize,
    callbacks: &InspectorCallbacks,
    can_move_up: bool,
    can_move_down: bool,
    is_instrument: bool,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(5.0))
        .py(px(7.0))
        .border_t(px(1.0))
        .border_color(Colors::border_subtle())
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .child(
                    div()
                        .w(px(22.0))
                        .text_size(px(10.0))
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(Colors::text_faint())
                        .child(display_index.to_string()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .truncate()
                        .text_size(px(11.0))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(Colors::text_primary())
                        .child(plugin_slot_name(Some(slot), "Empty Slot")),
                )
                .child(format_chip(plugin_format_label(slot))),
        )
        .child(kv_row("State", plugin_state_label(slot)))
        .child(kv_row("Latency", "TODO"))
        .child(insert_action_row(
            &track.id,
            slot,
            slot_index,
            callbacks,
            can_move_up,
            can_move_down,
            is_instrument,
        ))
}

fn instrument_section(track: &TrackState, callbacks: &InspectorCallbacks) -> gpui::AnyElement {
    let slot = track.instrument_insert();
    let slot_name = plugin_slot_name(slot, "No Instrument");
    let mut section = div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(fb_section_header("INSTRUMENT"))
        .child(kv_row("Plugin", slot_name))
        .child(kv_row(
            "Format",
            slot.map(plugin_format_label).unwrap_or("-").to_string(),
        ))
        .child(kv_row(
            "State",
            slot.map(plugin_state_label)
                .unwrap_or_else(|| "Empty".to_string()),
        ))
        .child(kv_row("MIDI Input", track.routing.midi_input.label()))
        .child(kv_row(
            "MIDI Ch",
            track
                .routing
                .midi_channel
                .map(|ch| ch.to_string())
                .unwrap_or_else(|| "All".to_string()),
        ))
        .child(kv_row("Output", track.routing.output.label()));

    if let Some(slot) = slot {
        section = section.child(insert_action_row(
            &track.id,
            slot,
            0,
            callbacks,
            false,
            track.inserts.len() > 1,
            true,
        ));
    } else {
        let track_id = track.id.clone();
        let picker = callbacks.on_open_insert_picker.clone();
        section = section.child(compact_action_button(
            "instrument-add",
            "Add Instrument",
            true,
            move |_, w, cx| picker(&(track_id.clone(), 0, true), w, cx),
        ));
    }

    section.into_any_element()
}

fn insert_effects_section(track: &TrackState, callbacks: &InspectorCallbacks) -> impl IntoElement {
    let effect_start = if track.track_type == TrackType::Instrument {
        1
    } else {
        0
    };
    let mut section = div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(fb_section_header("INSERT EFFECTS"));

    let effects = track.effect_inserts();
    if effects.is_empty() {
        section = section.child(kv_row("Effects", "No Effects"));
    } else {
        for (offset, slot) in effects.iter().enumerate() {
            let slot_index = effect_start + offset;
            section = section.child(plugin_slot_row(
                track,
                slot,
                slot_index,
                offset + 1,
                callbacks,
                slot_index > effect_start,
                slot_index + 1 < track.inserts.len(),
                false,
            ));
        }
    }

    let track_id = track.id.clone();
    let next_slot = track.inserts.len().max(effect_start);
    let picker = callbacks.on_open_insert_picker.clone();
    section.child(compact_action_button(
        "effect-add",
        "Add Effect",
        true,
        move |_, w, cx| picker(&(track_id.clone(), next_slot, false), w, cx),
    ))
}

fn track_inspector(
    track: &TrackState,
    name_input: &TextInputState,
    name_focused: bool,
    callbacks: &InspectorCallbacks,
) -> impl IntoElement {
    let automation_points: usize = track
        .automation_lanes
        .iter()
        .map(|lane| lane.points.len())
        .sum();
    let tid = track.id.clone();

    // ── Volume slider + dB readout ──────────────────────────────────────
    let volume_row = {
        let cb = callbacks.on_volume.clone();
        let tid_v = tid.clone();
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.0))
            .child(slider(
                "inspector-volume",
                track.volume,
                track.color,
                move |v, w, cx| cb(&(tid_v.clone(), *v), w, cx),
            ))
            .child(
                div()
                    .flex_shrink_0()
                    .min_w(px(40.0))
                    .text_size(px(10.0))
                    .text_color(Colors::text_secondary())
                    .child(format!("{} dB", volume::format_db(track.volume))),
            )
    };

    // ── Pan slider (mapped -1..1 ↔ 0..1) + readout ──────────────────────
    let pan_row = {
        let cb = callbacks.on_pan.clone();
        let tid_p = tid.clone();
        let pan_norm = (track.pan + 1.0) / 2.0;
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.0))
            .child(slider(
                "inspector-pan",
                pan_norm,
                track.color,
                move |v, w, cx| {
                    let pan = (*v * 2.0 - 1.0).clamp(-1.0, 1.0);
                    cb(&(tid_p.clone(), pan), w, cx);
                },
            ))
            .child(
                div()
                    .flex_shrink_0()
                    .min_w(px(40.0))
                    .text_size(px(10.0))
                    .text_color(Colors::text_secondary())
                    .child(format_pan(track.pan)),
            )
    };

    // ── M / S / R / I toggles ───────────────────────────────────────────
    let state_row = {
        let mute = callbacks.on_toggle_mute.clone();
        let solo = callbacks.on_toggle_solo.clone();
        let arm = callbacks.on_toggle_arm.clone();
        let input = callbacks.on_toggle_input.clone();
        let (t1, t2, t3, t4) = (tid.clone(), tid.clone(), tid.clone(), tid.clone());
        div()
            .flex()
            .flex_row()
            .gap(px(4.0))
            .child(toggle_badge(
                "inspector-mute",
                "M",
                track.muted,
                Colors::accent_warning(),
                move |_, w, cx| mute(&t1, w, cx),
            ))
            .child(toggle_badge(
                "inspector-solo",
                "S",
                track.solo,
                Colors::accent_success(),
                move |_, w, cx| solo(&t2, w, cx),
            ))
            .child(toggle_badge(
                "inspector-arm",
                "R",
                track.armed,
                Colors::accent_danger(),
                move |_, w, cx| arm(&t3, w, cx),
            ))
            .child(toggle_badge(
                "inspector-input",
                "I",
                track.input_monitor,
                Colors::accent_primary(),
                move |_, w, cx| input(&t4, w, cx),
            ))
    };

    scroll_body()
        .child(inspector_header(
            track.color,
            track.name.clone(),
            track_type_badge(track.track_type),
        ))
        // ── Basic ────────────────────────────────────────────────────────
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(fb_section_header("TRACK"))
                .child(kv_row("Type", track_type_badge(track.track_type)))
                .child(fb_form_row("Name", text_field(name_input, name_focused)))
                .child(fb_form_row("Volume", volume_row))
                .child(fb_form_row("Pan", pan_row))
                .child(fb_form_row(
                    "Color",
                    color_palette(tid.clone(), track.color, callbacks.on_set_color.clone()),
                ))
                .child(fb_form_row("State", state_row)),
        )
        .child(routing_section(track, callbacks))
        .when(track.track_type == TrackType::Instrument, |this| {
            this.child(instrument_section(track, callbacks))
        })
        .when(
            matches!(track.track_type, TrackType::Audio | TrackType::Instrument),
            |this| this.child(insert_effects_section(track, callbacks)),
        )
        // ── Contents counts ────────────────────────────────────────────────
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(fb_section_header("CONTENTS"))
                .child(kv_row("Clips", track.clips.len().to_string()))
                .child(kv_row("Inserts", track.inserts.len().to_string()))
                .child(kv_row("Sends", track.sends.len().to_string()))
                .child(kv_row(
                    "Automation Lanes",
                    track.automation_lanes.len().to_string(),
                ))
                .child(kv_row("Automation Points", automation_points.to_string())),
        )
}

fn beat_stepper(
    id: &'static str,
    clip_id: &str,
    value: f32,
    callback: ClipF32Cb,
    min_value: f32,
) -> impl IntoElement {
    let down_id = clip_id.to_string();
    let up_id = clip_id.to_string();
    let down = callback.clone();
    let up = callback;
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(4.0))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_size(px(11.0))
                .text_color(Colors::text_primary())
                .child(format!("{value:.2} bt")),
        )
        .child(compact_action_button(
            (id, 0usize),
            "-",
            value > min_value + 0.0001,
            move |_, w, cx| down(&(down_id.clone(), (value - 0.25).max(min_value)), w, cx),
        ))
        .child(compact_action_button(
            (id, 1usize),
            "+",
            true,
            move |_, w, cx| up(&(up_id.clone(), value + 0.25), w, cx),
        ))
}

fn file_name_from_path(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_string()
}

fn clip_inspector(
    clip: SelectedClipSummary<'_>,
    clip_name_input: &TextInputState,
    clip_name_focused: bool,
    callbacks: &InspectorCallbacks,
) -> impl IntoElement {
    let clip_id = clip.clip_id.to_string();
    let open_bottom = callbacks.on_open_clip_bottom_editor.clone();
    let open_external = callbacks.on_open_clip_external_midi_editor.clone();
    let mut body = scroll_body()
        .child(inspector_header(
            Colors::accent_primary(),
            clip.name.to_string(),
            "Clip",
        ))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(fb_section_header("CLIP"))
                .child(kv_row("Type", clip.kind.to_string()))
                .child(kv_row("Track", clip.track_name.to_string()))
                .child(kv_row("Track ID", clip.track_id.to_string()))
                .child(fb_form_row(
                    "Name",
                    text_field(clip_name_input, clip_name_focused),
                ))
                .child(fb_form_row(
                    "Start",
                    beat_stepper(
                        "clip-start",
                        clip.clip_id,
                        clip.start_beat,
                        callbacks.on_set_clip_start.clone(),
                        0.0,
                    ),
                ))
                .child(fb_form_row(
                    "Length",
                    beat_stepper(
                        "clip-length",
                        clip.clip_id,
                        clip.duration_beats,
                        callbacks.on_set_clip_length.clone(),
                        0.25,
                    ),
                ))
                .child(kv_row(
                    "End",
                    format!("{:.2} bt", clip.start_beat + clip.duration_beats),
                ))
                .child(kv_row(
                    "Muted",
                    if clip.muted { "Yes" } else { "No" }.to_string(),
                )),
        );

    if clip.kind == "MIDI" {
        let bottom_id = clip_id.clone();
        body = body.child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(fb_section_header("MIDI CLIP"))
                .child(kv_row(
                    "Notes",
                    clip.note_count.unwrap_or_default().to_string(),
                ))
                .child(kv_row(
                    "Local Length",
                    format!("{:.2} bt", clip.duration_beats),
                ))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .gap(px(4.0))
                        .child(compact_action_button(
                            "clip-open-bottom-midi",
                            "Bottom Editor",
                            true,
                            move |_, w, cx| open_bottom(&bottom_id, w, cx),
                        ))
                        .child(compact_action_button(
                            "clip-open-floating-midi",
                            "MIDI Window",
                            true,
                            move |_, w, cx| open_external(&clip_id, w, cx),
                        )),
                ),
        );
    } else {
        body = body.child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(fb_section_header("AUDIO CLIP"))
                .child(kv_row(
                    "File",
                    clip.source_path
                        .map(file_name_from_path)
                        .unwrap_or_else(|| "Missing source".to_string()),
                ))
                .child(kv_row(
                    "Path",
                    clip.source_path
                        .map(|path| path.to_string())
                        .unwrap_or_else(|| "-".to_string()),
                ))
                .child(kv_row(
                    "Source Duration",
                    clip.source_duration_seconds
                        .map(|seconds| format!("{seconds:.2} s"))
                        .unwrap_or_else(|| "Pending".to_string()),
                ))
                .child(kv_row("Gain", format!("{:.2}", clip.gain))),
        );
    }

    body
}

/// Helper retained for later phases: classify a clip's type label from its
/// stored `ClipType`. Kept here so the clip inspector and summary share one
/// source of truth.
pub fn clip_type_label(clip_type: &ClipType) -> &'static str {
    match clip_type {
        ClipType::Audio { .. } => "Audio",
        ClipType::Midi { .. } => "MIDI",
    }
}
