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
    div, px, App, AppContext, InteractiveElement, IntoElement, MouseButton, ParentElement,
    StatefulInteractiveElement, Styled, Window,
};

use crate::components::combo_box::{combo_box_string_menu, combo_box_trigger};
use crate::components::controls::{
    fb_button, fb_checkbox, fb_form_row, fb_section_header, FbButtonKind,
};
use crate::components::inspector::{
    inspector_checkbox as shared_inspector_checkbox, inspector_hint_text, inspector_mini_button,
    inspector_numeric_stepper, inspector_row as shared_inspector_row,
    inspector_section as shared_inspector_section, inspector_select, inspector_value,
    InspectorSelectOption,
};
use crate::components::reorder::{drag_handle, drop_over_highlight};
use crate::components::slider::slider;
use crate::components::text_input::{text_field, TextInputState};
use crate::components::timeline::timeline_state::{
    volume, AudioClipStretchState, ClipType, InsertLoadStatus, InsertSlotState, StretchAlgorithm,
    StretchMode, TrackAudioFormat, TrackInputRouting, TrackMidiInputRouting, TrackOutputRouting,
    TrackState, TrackType,
};
use crate::overlay::{inspector_combo_menu_position, OverlayAnchor};
use crate::theme::Colors;

type RoutingComboToggleCb =
    Arc<dyn Fn(InspectorRoutingCombo, Option<OverlayAnchor>, &mut Window, &mut App) + 'static>;

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
type InsertOutputChannelCb =
    Arc<dyn Fn(&(String, String, u8, bool), &mut Window, &mut App) + 'static>;
type ClipF32Cb = Arc<dyn Fn(&(String, f32), &mut Window, &mut App) + 'static>;
type ClipBoolCb = Arc<dyn Fn(&(String, bool), &mut Window, &mut App) + 'static>;
/// Apply a full replacement of a clip's stretch/pitch state. One callback drives
/// every stretch control; the inspector builds the mutated state and the layout
/// records it as a single undo entry (see `set_clip_stretch_cb`).
type ClipStretchCb = Arc<dyn Fn(&(String, AudioClipStretchState), &mut Window, &mut App) + 'static>;

/// Transient UI state for async clip tempo analysis (not persisted).
#[derive(Debug, Clone, Default)]
pub struct StretchTempoUiSnapshot {
    pub finding: bool,
    pub error: Option<String>,
    pub alternatives: Vec<f32>,
    pub confidence: Option<f32>,
    pub low_confidence: bool,
    pub suggested_bpm: Option<f32>,
}
/// Reorder an FX/insert slot via drag. `(track_id, dragged_insert_id,
/// insertion_index)` where `insertion_index` is the gap (0..=len) the dragged
/// slot should move into. Identity is the stable `plugin_instance_id`, never
/// the visual index. One completed drag = one undo entry (see
/// `reorder_insert_cb`).
type InsertReorderCb = Arc<dyn Fn(&(String, String, usize), &mut Window, &mut App) + 'static>;

/// Drag payload for FX/insert reorder. Carries the stable instance identity
/// plus a label rendered in the drag preview. Cloned into the GPUI drag view.
#[derive(Clone)]
pub struct FxSlotDrag {
    pub track_id: String,
    pub insert_id: String,
    pub display_name: String,
}

impl gpui::Render for FxSlotDrag {
    fn render(&mut self, _window: &mut Window, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
            .px(px(8.0))
            .py(px(3.0))
            .rounded_sm()
            .bg(Colors::surface_raised())
            .border(px(1.0))
            .border_color(Colors::accent_primary())
            .text_size(px(11.0))
            .font_weight(gpui::FontWeight::MEDIUM)
            .text_color(Colors::text_primary())
            .child(self.display_name.clone())
    }
}

/// Open routing ComboBox in the Inspector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectorRoutingCombo {
    AudioFormat,
    AudioInput,
    AudioOutput,
    VstiOutputs,
    MidiInput,
    MidiChannel,
    MidiOut,
}

const ROUTING_COMBO_MENU_HEIGHT: f32 = 220.0;

/// Edit callbacks handed to the Inspector. Built by the layout
/// (`build_inspector_callbacks`) and dispatched to the shared `TimelineState`.
#[derive(Clone)]
pub struct InspectorCallbacks {
    pub on_volume: StrF32Cb,
    /// Toggle whether Track Volume automation drives this track's effective
    /// volume (the `[A]` button beside the volume readout).
    pub on_toggle_volume_automation_read: StrCb,
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
    pub on_toggle_insert_output_channel: InsertOutputChannelCb,
    pub on_move_insert: InsertMoveCb,
    /// Drag-reorder commit for an FX/insert slot (one undo entry per drag).
    pub on_reorder_insert: InsertReorderCb,
    pub on_open_insert_editor: InsertOpenCb,
    pub on_set_clip_start: ClipF32Cb,
    pub on_set_clip_length: ClipF32Cb,
    pub on_set_clip_gain: ClipF32Cb,
    pub on_set_clip_muted: ClipBoolCb,
    /// Apply a new stretch/pitch state to an audio clip (one undo entry).
    pub on_set_clip_stretch: ClipStretchCb,
    /// Analyze source audio and set `bpm_source` asynchronously.
    pub on_clip_stretch_auto_find_bpm: StrCb,
    /// Fit clip tempo to project BPM (auto-finds source BPM first if needed).
    pub on_clip_stretch_fit_project: StrCb,
    /// Append a warp marker at the current playhead on the given clip.
    pub on_clip_warp_add_at_playhead: StrCb,
    /// Remove all warp markers from the given clip.
    pub on_clip_warp_clear: StrCb,
    pub on_open_clip_bottom_editor: StrCb,
    pub on_open_clip_external_midi_editor: StrCb,
    pub open_routing_combo: Option<InspectorRoutingCombo>,
    pub on_toggle_routing_combo: RoutingComboToggleCb,
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
    /// Live stretch/pitch state of the selected clip (for the audio Inspector).
    pub stretch: &'a AudioClipStretchState,
    /// Current project tempo, shown as the read-only Tempo-Sync target.
    pub project_bpm: f64,
    /// Active arrangement time-selection duration in beats, when available.
    pub selection_duration_beats: Option<f32>,
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
    stretch_tempo: Option<StretchTempoUiSnapshot>,
    name_input: &TextInputState,
    name_focused: bool,
    clip_name_input: &TextInputState,
    clip_name_focused: bool,
    callbacks: &InspectorCallbacks,
) -> impl IntoElement {
    let body: gpui::AnyElement = if let Some(clip) = clip_summary {
        let tempo = stretch_tempo.unwrap_or_default();
        clip_inspector(clip, clip_name_input, clip_name_focused, tempo, callbacks)
            .into_any_element()
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

fn format_selector(track: &TrackState, callbacks: &InspectorCallbacks) -> impl IntoElement {
    routing_combo_trigger(
        "inspector-format-combo",
        track.routing.audio_format.label().to_string(),
        InspectorRoutingCombo::AudioFormat,
        callbacks.open_routing_combo,
        callbacks.on_toggle_routing_combo.clone(),
    )
}

fn audio_input_selector(track: &TrackState, callbacks: &InspectorCallbacks) -> impl IntoElement {
    routing_combo_trigger(
        "inspector-input-combo",
        audio_input_combo_label(&track.routing.input),
        InspectorRoutingCombo::AudioInput,
        callbacks.open_routing_combo,
        callbacks.on_toggle_routing_combo.clone(),
    )
}

fn output_selector(track: &TrackState, callbacks: &InspectorCallbacks) -> impl IntoElement {
    routing_combo_trigger(
        "inspector-output-combo",
        audio_output_combo_label(&track.routing.output, track.routing.audio_format),
        InspectorRoutingCombo::AudioOutput,
        callbacks.open_routing_combo,
        callbacks.on_toggle_routing_combo.clone(),
    )
}

fn normalized_vsti_output_channels(slot: &InsertSlotState) -> Vec<u8> {
    let mut channels = if slot.enabled_audio_output_channels.is_empty() {
        vec![1, 2]
    } else {
        slot.enabled_audio_output_channels.clone()
    };
    if !channels.contains(&1) {
        channels.push(1);
    }
    if !channels.contains(&2) {
        channels.push(2);
    }
    channels.retain(|channel| (1..=16).contains(channel));
    channels.sort_unstable();
    channels.dedup();
    channels
}

fn vsti_output_label(slot: &InsertSlotState) -> String {
    let channels = normalized_vsti_output_channels(slot);
    let extras: Vec<String> = channels
        .iter()
        .copied()
        .filter(|channel| *channel > 2)
        .map(|channel| format!("Ch {channel}"))
        .collect();
    if extras.is_empty() {
        "Main 1/2".to_string()
    } else {
        format!("Main 1/2 + {}", extras.join(", "))
    }
}

fn vsti_output_selector(slot: &InsertSlotState, callbacks: &InspectorCallbacks) -> impl IntoElement {
    routing_combo_trigger(
        "inspector-vsti-output-combo",
        vsti_output_label(slot),
        InspectorRoutingCombo::VstiOutputs,
        callbacks.open_routing_combo,
        callbacks.on_toggle_routing_combo.clone(),
    )
}

fn vsti_output_dropdown(
    track: &TrackState,
    slot: &InsertSlotState,
    callbacks: &InspectorCallbacks,
    position: crate::overlay::OverlayPosition,
) -> impl IntoElement {
    let selected = normalized_vsti_output_channels(slot);
    let left: f32 = position.x.into();
    let top: f32 = position.y.into();
    let width: f32 = position.width.map(|w| w.into()).unwrap_or(176.0);
    let max_h: f32 = position.max_height.map(|h| h.into()).unwrap_or(260.0);
    let mut menu = div()
        .id("inspector-vsti-output-dropdown")
        .absolute()
        .left(px(left))
        .top(px(top))
        .w(px(width))
        .max_h(px(max_h))
        .flex()
        .flex_col()
        .gap(px(2.0))
        .p(px(6.0))
        .rounded_md()
        .border(px(1.0))
        .border_color(Colors::border_subtle())
        .bg(Colors::surface_card())
        .shadow(vec![gpui::BoxShadow {
            color: Colors::surface_overlay().into(),
            offset: gpui::point(px(0.0), px(10.0)),
            blur_radius: px(28.0),
            spread_radius: px(0.0),
            inset: false,
        }])
        .overflow_y_scroll()
        .occlude()
        .on_mouse_down(MouseButton::Left, |_, _window, cx| cx.stop_propagation());

    menu = menu.child(fb_checkbox(
        "vsti-output-main",
        "Main 1/2",
        true,
        false,
        |_, _, _| {},
    ));

    for channel in 3u8..=16 {
        let track_id = track.id.clone();
        let insert_id = slot.id.clone();
        let checked = selected.contains(&channel);
        let toggle = callbacks.on_toggle_insert_output_channel.clone();
        menu = menu.child(fb_checkbox(
            ("vsti-output-channel", channel as u32),
            format!("Channel {channel}"),
            checked,
            true,
            move |_, window, cx| {
                toggle(
                    &(track_id.clone(), insert_id.clone(), channel, !checked),
                    window,
                    cx,
                )
            },
        ));
    }

    menu
}

fn audio_format_options() -> Vec<String> {
    vec![
        TrackAudioFormat::Mono.label().to_string(),
        TrackAudioFormat::Stereo.label().to_string(),
    ]
}

/// Build the Inspector audio-input options as `(label, routing)` pairs from the
/// selected input device's channel count. Mono routes map to
/// `AudioDeviceChannel`; stereo pairs and multi-channel routes map to
/// `AudioDeviceChannels`.
fn build_input_routing_options(
    track: &TrackState,
    device: Option<&(String, u32)>,
) -> Vec<(String, TrackInputRouting)> {
    let mut out = vec![("None".to_string(), TrackInputRouting::None)];
    if let Some((name, count)) = device {
        for opt in crate::audio_routing::build_input_channel_options(*count) {
            let compatible = match track.routing.audio_format {
                TrackAudioFormat::Mono => opt.channels.len() == 1,
                TrackAudioFormat::Stereo => opt.channels.len() == 2,
            };
            if !compatible {
                continue;
            }
            let routing = match opt.channels.as_slice() {
                [ch] => TrackInputRouting::AudioDeviceChannel {
                    device_id: name.clone(),
                    channel: *ch,
                },
                channels if !channels.is_empty() => TrackInputRouting::AudioDeviceChannels {
                    device_id: name.clone(),
                    channels: channels.to_vec(),
                },
                _ => continue,
            };
            out.push((audio_input_combo_label(&routing), routing));
        }
    }
    if !out.iter().any(|(_, r)| *r == track.routing.input) {
        out.push((
            format!(
                "Missing - {}",
                audio_input_combo_label(&track.routing.input)
            ),
            track.routing.input.clone(),
        ));
    }
    out
}

fn audio_input_combo_label(routing: &TrackInputRouting) -> String {
    match routing {
        TrackInputRouting::None => "None".to_string(),
        TrackInputRouting::AudioDeviceChannel { channel, .. } => {
            format!("Channel {}", channel + 1)
        }
        TrackInputRouting::AudioDeviceChannels { channels, .. } => match channels.as_slice() {
            [0, 1] => "Stereo".to_string(),
            [left, right] => format!("Stereo {}+{}", left + 1, right + 1),
            channels if !channels.is_empty() => channels
                .iter()
                .map(|channel| (channel + 1).to_string())
                .collect::<Vec<_>>()
                .join("+"),
            _ => "None".to_string(),
        },
        TrackInputRouting::AllInputs => "All Inputs".to_string(),
        TrackInputRouting::MidiDevice { .. } => "MIDI".to_string(),
    }
}

fn build_audio_output_options(
    track: &TrackState,
    bus_targets: &[(String, String)],
    _output_device: Option<&(String, u32)>,
) -> Vec<(String, TrackOutputRouting)> {
    let mut out = vec![
        (
            master_output_label(track.routing.audio_format),
            TrackOutputRouting::Main,
        ),
        ("None".to_string(), TrackOutputRouting::None),
    ];
    for (bus_id, name) in bus_targets {
        if *bus_id == track.id {
            continue;
        }
        out.push((
            format!("Bus - {name}"),
            TrackOutputRouting::Bus {
                bus_id: bus_id.clone(),
            },
        ));
    }
    if !out.iter().any(|(_, r)| *r == track.routing.output) {
        out.push((
            format!(
                "Missing - {}",
                audio_output_combo_label(&track.routing.output, track.routing.audio_format)
            ),
            track.routing.output.clone(),
        ));
    }
    out
}

fn audio_output_combo_label(
    routing: &TrackOutputRouting,
    audio_format: TrackAudioFormat,
) -> String {
    match routing {
        TrackOutputRouting::Main => master_output_label(audio_format),
        TrackOutputRouting::Bus { bus_id } => format!("Bus - {bus_id}"),
        TrackOutputRouting::HardwareOutput { channel, .. } => {
            format!("Channel {}", channel + 1)
        }
        TrackOutputRouting::None => "None".to_string(),
    }
}

fn master_output_label(audio_format: TrackAudioFormat) -> String {
    match audio_format {
        TrackAudioFormat::Mono => "Mono Master".to_string(),
        TrackAudioFormat::Stereo => "Stereo Master".to_string(),
    }
}

fn parse_audio_format_option(label: &str) -> TrackAudioFormat {
    match label {
        "Mono" => TrackAudioFormat::Mono,
        _ => TrackAudioFormat::Stereo,
    }
}

fn midi_input_combo_label(routing: &TrackMidiInputRouting) -> String {
    match routing {
        TrackMidiInputRouting::AllInputs => "All".to_string(),
        TrackMidiInputRouting::None => "None".to_string(),
        TrackMidiInputRouting::MidiDevice { device_id } => device_id.clone(),
    }
}

fn midi_channel_combo_label(channel: Option<u8>) -> String {
    channel
        .map(|ch| ch.to_string())
        .unwrap_or_else(|| "All".to_string())
}

fn midi_input_options(detected: &[String]) -> Vec<String> {
    let mut options = vec!["All".to_string(), "None".to_string()];
    options.extend(detected.iter().cloned());
    options
}

fn midi_channel_options() -> Vec<String> {
    std::iter::once("All".to_string())
        .chain((1..=16).map(|ch| ch.to_string()))
        .collect()
}

fn midi_output_options(detected: &[String]) -> Vec<String> {
    let mut options = vec!["Main".to_string(), "None".to_string()];
    options.extend(detected.iter().cloned());
    options
}

fn parse_midi_input_option(label: &str) -> TrackMidiInputRouting {
    match label {
        "All" => TrackMidiInputRouting::AllInputs,
        "None" => TrackMidiInputRouting::None,
        device => TrackMidiInputRouting::MidiDevice {
            device_id: device.to_string(),
        },
    }
}

fn parse_midi_channel_option(label: &str) -> Option<u8> {
    if label == "All" {
        None
    } else {
        label.parse::<u8>().ok().map(|ch| ch.clamp(1, 16))
    }
}

fn parse_midi_output_option(label: &str) -> TrackOutputRouting {
    match label {
        "Main" => TrackOutputRouting::Main,
        "None" => TrackOutputRouting::None,
        device => TrackOutputRouting::HardwareOutput {
            device_id: device.to_string(),
            channel: 0,
        },
    }
}

fn routing_combo_trigger(
    trigger_id: &'static str,
    label: String,
    combo: InspectorRoutingCombo,
    open_combo: Option<InspectorRoutingCombo>,
    on_toggle: RoutingComboToggleCb,
) -> impl IntoElement {
    let open = open_combo == Some(combo);
    let toggle = on_toggle.clone();
    div().w_full().child(combo_box_trigger(
        trigger_id,
        label,
        open,
        move |event, window, cx| {
            let anchor = if open {
                None
            } else {
                Some(OverlayAnchor {
                    bounds: crate::overlay::inspector_combo_trigger_bounds(
                        window,
                        INSPECTOR_WIDTH,
                        event,
                    ),
                })
            };
            toggle(combo, anchor, window, cx);
        },
    ))
}

fn midi_input_selector(track: &TrackState, callbacks: &InspectorCallbacks) -> impl IntoElement {
    routing_combo_trigger(
        "inspector-midi-input-combo",
        midi_input_combo_label(&track.routing.midi_input),
        InspectorRoutingCombo::MidiInput,
        callbacks.open_routing_combo,
        callbacks.on_toggle_routing_combo.clone(),
    )
}

fn midi_channel_selector(track: &TrackState, callbacks: &InspectorCallbacks) -> impl IntoElement {
    routing_combo_trigger(
        "inspector-midi-channel-combo",
        midi_channel_combo_label(track.routing.midi_channel),
        InspectorRoutingCombo::MidiChannel,
        callbacks.open_routing_combo,
        callbacks.on_toggle_routing_combo.clone(),
    )
}

fn midi_output_selector(track: &TrackState, callbacks: &InspectorCallbacks) -> impl IntoElement {
    routing_combo_trigger(
        "inspector-midi-output-combo",
        track.routing.output.label(),
        InspectorRoutingCombo::MidiOut,
        callbacks.open_routing_combo,
        callbacks.on_toggle_routing_combo.clone(),
    )
}

type CloseRoutingComboCb = Arc<dyn Fn(&mut App) + 'static>;

/// Dropdown overlay for Inspector MIDI routing ComboBoxes. Rendered above the
/// main chrome so menus stay anchored to their trigger, not the mount point.
pub fn inspector_routing_combo_overlay(
    track: &TrackState,
    open_combo: InspectorRoutingCombo,
    anchor: OverlayAnchor,
    window: &Window,
    callbacks: &InspectorCallbacks,
    on_close: CloseRoutingComboCb,
    // Selected input device `(name, channel_count)` for the audio-input combo.
    audio_input_device: Option<(String, u32)>,
    // Available Bus/Return output targets as `(track_id, display_name)`.
    audio_output_buses: Vec<(String, String)>,
    // Selected output device `(name, channel_count)` for hardware output routes.
    audio_output_device: Option<(String, u32)>,
) -> impl IntoElement {
    let position =
        inspector_combo_menu_position(anchor, INSPECTOR_WIDTH, ROUTING_COMBO_MENU_HEIGHT, window);
    // Placeholder until DAUx exposes MIDI device lists to the UI shell.
    let detected_midi_inputs: Vec<String> = Vec::new();
    let detected_midi_outputs: Vec<String> = Vec::new();

    let track_id = track.id.clone();
    let menu = match open_combo {
        InspectorRoutingCombo::AudioFormat => {
            let selected = track.routing.audio_format.label().to_string();
            let options = audio_format_options();
            let cb = callbacks.on_set_audio_format.clone();
            let close = on_close.clone();
            combo_box_string_menu(
                "inspector-audio-format-menu",
                position,
                &selected,
                &options,
                Arc::new(move |value, window, cx| {
                    let format = parse_audio_format_option(&value);
                    cb(&(track_id.clone(), format), window, cx);
                    close(cx);
                }),
            )
            .into_any_element()
        }
        InspectorRoutingCombo::AudioInput => {
            let routing_options = build_input_routing_options(track, audio_input_device.as_ref());
            let selected = routing_options
                .iter()
                .find(|(_, r)| *r == track.routing.input)
                .map(|(l, _)| l.clone())
                .unwrap_or_else(|| track.routing.input.label());
            let labels: Vec<String> = routing_options.iter().map(|(l, _)| l.clone()).collect();
            let cb = callbacks.on_set_input_routing.clone();
            let close = on_close.clone();
            combo_box_string_menu(
                "inspector-audio-input-menu",
                position,
                &selected,
                &labels,
                Arc::new(move |value, window, cx| {
                    let routing = routing_options
                        .iter()
                        .find(|(l, _)| *l == value)
                        .map(|(_, r)| r.clone())
                        .unwrap_or(TrackInputRouting::None);
                    cb(&(track_id.clone(), routing), window, cx);
                    close(cx);
                }),
            )
            .into_any_element()
        }
        InspectorRoutingCombo::AudioOutput => {
            let routing_options = build_audio_output_options(
                track,
                &audio_output_buses,
                audio_output_device.as_ref(),
            );
            let selected = routing_options
                .iter()
                .find(|(_, r)| *r == track.routing.output)
                .map(|(l, _)| l.clone())
                .unwrap_or_else(|| track.routing.output.label());
            let labels: Vec<String> = routing_options.iter().map(|(l, _)| l.clone()).collect();
            let cb = callbacks.on_set_output_routing.clone();
            let close = on_close.clone();
            combo_box_string_menu(
                "inspector-audio-output-menu",
                position,
                &selected,
                &labels,
                Arc::new(move |value, window, cx| {
                    let output = routing_options
                        .iter()
                        .find(|(l, _)| *l == value)
                        .map(|(_, r)| r.clone())
                        .unwrap_or(TrackOutputRouting::None);
                    cb(&(track_id.clone(), output), window, cx);
                    close(cx);
                }),
            )
            .into_any_element()
        }
        InspectorRoutingCombo::VstiOutputs => track
            .instrument_insert()
            .map(|slot| vsti_output_dropdown(track, slot, callbacks, position).into_any_element())
            .unwrap_or_else(|| div().into_any_element()),
        InspectorRoutingCombo::MidiInput => {
            let selected = midi_input_combo_label(&track.routing.midi_input);
            let options = midi_input_options(&detected_midi_inputs);
            let cb = callbacks.on_set_midi_input.clone();
            let close = on_close.clone();
            combo_box_string_menu(
                "inspector-midi-input-menu",
                position,
                &selected,
                &options,
                Arc::new(move |value, window, cx| {
                    let routing = parse_midi_input_option(&value);
                    cb(&(track_id.clone(), routing), window, cx);
                    close(cx);
                }),
            )
            .into_any_element()
        }
        InspectorRoutingCombo::MidiChannel => {
            let selected = midi_channel_combo_label(track.routing.midi_channel);
            let options = midi_channel_options();
            let cb = callbacks.on_set_midi_channel.clone();
            let close = on_close.clone();
            combo_box_string_menu(
                "inspector-midi-channel-menu",
                position,
                &selected,
                &options,
                Arc::new(move |value, window, cx| {
                    let channel = parse_midi_channel_option(&value);
                    cb(&(track_id.clone(), channel), window, cx);
                    close(cx);
                }),
            )
            .into_any_element()
        }
        InspectorRoutingCombo::MidiOut => {
            let selected = track.routing.output.label();
            let options = midi_output_options(&detected_midi_outputs);
            let cb = callbacks.on_set_output_routing.clone();
            let close = on_close.clone();
            combo_box_string_menu(
                "inspector-midi-output-menu",
                position,
                &selected,
                &options,
                Arc::new(move |value, window, cx| {
                    let output = parse_midi_output_option(&value);
                    cb(&(track_id.clone(), output), window, cx);
                    close(cx);
                }),
            )
            .into_any_element()
        }
    };

    div()
        .absolute()
        .inset_0()
        .id("inspector-routing-combo-overlay")
        .on_mouse_down(MouseButton::Left, move |_, _window, cx| {
            on_close(cx);
        })
        .child(menu)
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
                .child(fb_form_row(
                    "MIDI Out",
                    midi_output_selector(track, callbacks),
                ));
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
        InsertLoadStatus::Missing(message) => format!("Missing: {message}"),
        InsertLoadStatus::Failed(message) => format!("Failed: {message}"),
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
    // Drag source: the grip handle carries the stable plugin_instance_id, so
    // reorder identity follows the instance — never the visual index — and
    // bypass / preset / editor / automation state come along untouched (the
    // model only reorders existing slots, see `set_insert_order`).
    let drag_payload = FxSlotDrag {
        track_id: track.id.clone(),
        insert_id: slot.id.clone(),
        display_name: plugin_slot_name(Some(slot), "Empty Slot"),
    };
    let handle = drag_handle()
        .id(("fx-drag-handle", slot_index))
        .on_drag(drag_payload, |drag, _offset, _window, cx| {
            cx.new(|_| drag.clone())
        });

    // Drop target: dropping a compatible drag onto this row moves it into the
    // gap *above* this slot (`insertion_index == slot_index`). `can_drop`
    // restricts drops to the same track, and `drag_over` paints the accent
    // drop-position line. The row is NOT a drag source, so the action buttons
    // and right-click keep their own hit-testing (only the handle drags).
    let drop_track = track.id.clone();
    let can_drop_track = track.id.clone();
    let reorder = callbacks.on_reorder_insert.clone();
    let gap = slot_index;

    div()
        .id(("fx-drop-row", slot_index))
        .flex()
        .flex_col()
        .gap(px(5.0))
        .py(px(7.0))
        .border_t(px(1.0))
        .border_color(Colors::border_subtle())
        .can_drop(move |dragged, _window, _cx| {
            dragged
                .downcast_ref::<FxSlotDrag>()
                .is_some_and(|d| d.track_id == can_drop_track)
        })
        .drag_over::<FxSlotDrag>(|style, _drag, _window, _cx| drop_over_highlight(style))
        .on_drop::<FxSlotDrag>(move |drag, window, cx| {
            if drag.track_id == drop_track {
                reorder(
                    &(drop_track.clone(), drag.insert_id.clone(), gap),
                    window,
                    cx,
                );
            }
        })
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .child(handle)
                .child(
                    div()
                        .w(px(18.0))
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
        section = section
            .child(fb_form_row("VSTi Outputs", vsti_output_selector(slot, callbacks)))
            .child(insert_action_row(
                &track.id, slot, 0, callbacks, false, false, true,
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
        // Trailing drop zone so a dragged slot can land at the very end of the
        // chain (insertion gap == inserts.len()); rows above only cover the
        // gaps before each slot. Same-track guarded; shows the accent line.
        let end_track = track.id.clone();
        let end_can_track = track.id.clone();
        let end_reorder = callbacks.on_reorder_insert.clone();
        let end_gap = track.inserts.len();
        section = section.child(
            div()
                .id("fx-drop-end")
                .h(px(8.0))
                .can_drop(move |dragged, _window, _cx| {
                    dragged
                        .downcast_ref::<FxSlotDrag>()
                        .is_some_and(|d| d.track_id == end_can_track)
                })
                .drag_over::<FxSlotDrag>(|style, _drag, _window, _cx| drop_over_highlight(style))
                .on_drop::<FxSlotDrag>(move |drag, window, cx| {
                    if drag.track_id == end_track {
                        end_reorder(
                            &(end_track.clone(), drag.insert_id.clone(), end_gap),
                            window,
                            cx,
                        );
                    }
                }),
        );
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
    // When Track Volume automation is reading, the slider/readout follow the
    // effective (automation) value and an `[A]` marker plus a separate base
    // readout make the manual value clear. Otherwise it shows the base only.
    let volume_row = {
        let cb = callbacks.on_volume.clone();
        let tid_v = tid.clone();
        let has_volume_automation = track.has_active_volume_automation();
        let automation_active = track.volume_automation_read && has_volume_automation;
        let display_vol = track.display_volume();
        // `[A]` automation-read toggle — only meaningful when a volume lane
        // exists. Lit when reading, dim when bypassed.
        let auto_toggle = has_volume_automation.then(|| {
            let read_cb = callbacks.on_toggle_volume_automation_read.clone();
            let tid_a = tid.clone();
            let on = track.volume_automation_read;
            div()
                .id("inspector-vol-automation-read")
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .w(px(18.0))
                .h(px(16.0))
                .rounded_sm()
                .border(px(1.0))
                .border_color(if on {
                    Colors::accent_primary()
                } else {
                    Colors::border_default()
                })
                .bg(if on {
                    Colors::with_alpha(Colors::accent_primary(), 0.18)
                } else {
                    Colors::with_alpha(Colors::surface_canvas(), 0.3)
                })
                .text_size(px(9.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(if on {
                    Colors::accent_primary()
                } else {
                    Colors::text_muted()
                })
                .cursor(gpui::CursorStyle::PointingHand)
                .child("A")
                .on_mouse_down(gpui::MouseButton::Left, move |_ev, w, cx| {
                    read_cb(&tid_a, w, cx)
                })
        });
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.0))
            .child(slider(
                "inspector-volume",
                display_vol,
                track.color,
                move |v, w, cx| cb(&(tid_v.clone(), *v), w, cx),
            ))
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .items_end()
                    .min_w(px(48.0))
                    .text_size(px(10.0))
                    .text_color(Colors::text_secondary())
                    .child(format!(
                        "{} dB{}",
                        volume::format_db(display_vol),
                        if automation_active { " [A]" } else { "" }
                    ))
                    .when(has_volume_automation, |this| {
                        this.child(
                            div()
                                .text_size(px(8.0))
                                .text_color(Colors::text_faint())
                                .child(format!("Base {} dB", volume::format_db(track.volume))),
                        )
                    }),
            )
            .when_some(auto_toggle, |row, toggle| row.child(toggle))
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
                track.input_monitor.is_active(track.armed),
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
                .child(kv_row("Inserts", track.effect_inserts().len().to_string()))
                .child(kv_row("Sends", track.sends.len().to_string()))
                .child(kv_row(
                    "Automation Lanes",
                    track.automation_lanes.len().to_string(),
                ))
                .child(kv_row("Automation Points", automation_points.to_string())),
        )
}

fn inspector_section(label: impl Into<String>, child: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(5.0))
        .child(fb_section_header(label))
        .child(child)
}

fn compact_property_row(label: impl Into<String>, child: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .min_h(px(26.0))
        .child(
            div()
                .w(px(66.0))
                .flex_shrink_0()
                .text_size(px(10.5))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(Colors::text_muted())
                .child(label.into()),
        )
        .child(div().flex_1().min_w_0().child(child))
}

fn value_box(text: impl Into<String>, width: f32) -> impl IntoElement {
    div()
        .w(px(width))
        .h(px(26.0))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_end()
        .rounded_md()
        .border(px(1.0))
        .border_color(Colors::border_subtle())
        .bg(Colors::surface_input())
        .px(px(8.0))
        .text_size(px(11.0))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(Colors::text_primary())
        .child(text.into())
}

fn readonly_value(text: impl Into<String>) -> impl IntoElement {
    div()
        .h(px(26.0))
        .flex()
        .items_center()
        .justify_end()
        .pr(px(64.0))
        .text_size(px(11.0))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(Colors::text_secondary())
        .child(text.into())
}

fn small_step_button(
    id: impl Into<gpui::ElementId>,
    label: &'static str,
    enabled: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let mut button = div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .w(px(28.0))
        .min_w(px(28.0))
        .h(px(26.0))
        .rounded_md()
        .border(px(1.0))
        .border_color(Colors::border_subtle())
        .bg(Colors::surface_input())
        .text_size(px(12.0))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(Colors::text_secondary())
        .opacity(if enabled { 1.0 } else { 0.45 })
        .child(label);
    if enabled {
        button = button
            .cursor(gpui::CursorStyle::PointingHand)
            .hover(|s| {
                s.bg(Colors::surface_control_hover())
                    .border_color(Colors::border_strong())
            })
            .on_click(on_click);
    }
    button
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
        .child(value_box(format!("{value:.2} bt"), 92.0))
        .child(small_step_button(
            (id, 0usize),
            "-",
            value > min_value + 0.0001,
            move |_, w, cx| down(&(down_id.clone(), (value - 0.25).max(min_value)), w, cx),
        ))
        .child(small_step_button(
            (id, 1usize),
            "+",
            true,
            move |_, w, cx| up(&(up_id.clone(), value + 0.25), w, cx),
        ))
}

fn linear_gain_to_db(gain: f32) -> f32 {
    if gain <= 0.000_001 {
        -60.0
    } else {
        20.0 * gain.log10()
    }
}

fn db_to_linear_gain(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

fn gain_stepper(clip_id: &str, gain: f32, callback: ClipF32Cb) -> impl IntoElement {
    let db = linear_gain_to_db(gain);
    let down_id = clip_id.to_string();
    let up_id = clip_id.to_string();
    let down = callback.clone();
    let up = callback;
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(4.0))
        .child(value_box(format!("{db:.1} dB"), 92.0))
        .child(small_step_button(
            "clip-gain-down",
            "-",
            db > -59.9,
            move |_, w, cx| {
                down(
                    &(down_id.clone(), db_to_linear_gain((db - 1.0).max(-60.0))),
                    w,
                    cx,
                )
            },
        ))
        .child(small_step_button(
            "clip-gain-up",
            "+",
            db < 12.0,
            move |_, w, cx| {
                up(
                    &(up_id.clone(), db_to_linear_gain((db + 1.0).min(12.0))),
                    w,
                    cx,
                )
            },
        ))
}

fn truncate_value(text: impl Into<String>) -> impl IntoElement {
    div()
        .min_w(px(0.0))
        .truncate()
        .text_size(px(11.0))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(Colors::text_primary())
        .child(text.into())
}

// ── Audio-clip stretch inspector controls (Slice 2) ─────────────────────────
//
// Every control produces a fully-formed next `AudioClipStretchState` and routes
// it through the single `on_set_clip_stretch` callback, so each edit is one
// undo entry. The controls edit real persisted state; the audio engine wires it
// to playback/export in a later slice (the Stretch section says so honestly).

#[derive(Clone, Copy, PartialEq, Eq)]
enum StretchBasicMode {
    Off,
    RePitch,
    PreservePitch,
}

const STRETCH_ACTIVE_MODE_OPTIONS: &[InspectorSelectOption<StretchBasicMode>] = &[
    InspectorSelectOption {
        label: "RePitch",
        value: StretchBasicMode::RePitch,
    },
    InspectorSelectOption {
        label: "Preserve Pitch",
        value: StretchBasicMode::PreservePitch,
    },
];

fn mode_supports_preserve_pitch(mode: StretchMode) -> bool {
    matches!(
        mode,
        StretchMode::Manual | StretchMode::TempoSync | StretchMode::Warp
    )
}

fn with_mode(s: &AudioClipStretchState, mode: StretchMode) -> AudioClipStretchState {
    let mut n = s.clone();
    n.mode = mode;
    if mode == StretchMode::Off {
        n.preserve_pitch = false;
        n.algorithm = StretchAlgorithm::Auto;
    } else if !mode_supports_preserve_pitch(mode) {
        n.preserve_pitch = false;
        n.algorithm = StretchAlgorithm::ResampleOnly;
    } else if matches!(n.algorithm, StretchAlgorithm::Auto) {
        n.algorithm = StretchAlgorithm::ResampleOnly;
    }
    n.dirty = true;
    n
}

fn stretch_basic_mode(s: &AudioClipStretchState) -> StretchBasicMode {
    if s.mode == StretchMode::Off {
        StretchBasicMode::Off
    } else if s.preserve_pitch && !matches!(s.algorithm, StretchAlgorithm::ResampleOnly) {
        StretchBasicMode::PreservePitch
    } else {
        StretchBasicMode::RePitch
    }
}

fn with_basic_mode(s: &AudioClipStretchState, mode: StretchBasicMode) -> AudioClipStretchState {
    let mut next = s.clone();
    match mode {
        StretchBasicMode::Off => {
            next.mode = StretchMode::Off;
            next.algorithm = StretchAlgorithm::Auto;
            next.preserve_pitch = false;
        }
        StretchBasicMode::RePitch => {
            next.mode = StretchMode::Manual;
            next.algorithm = StretchAlgorithm::ResampleOnly;
            next.preserve_pitch = false;
        }
        StretchBasicMode::PreservePitch => {
            next.mode = StretchMode::Manual;
            next.algorithm = StretchAlgorithm::PhaseVocoder;
            next.preserve_pitch = true;
        }
    }
    next.clip_timeline_duration_beats = 0.0;
    next.dirty = true;
    next
}

#[cfg(test)]
mod stretch_inspector_tests {
    use super::*;

    #[test]
    fn preserve_pitch_availability_matches_mode() {
        assert!(!mode_supports_preserve_pitch(StretchMode::Resample));
        assert!(mode_supports_preserve_pitch(StretchMode::Manual));
        assert!(mode_supports_preserve_pitch(StretchMode::TempoSync));
        assert!(mode_supports_preserve_pitch(StretchMode::Warp));
    }

    #[test]
    fn switching_to_resample_clears_preserve_pitch() {
        let state = AudioClipStretchState {
            preserve_pitch: true,
            ..AudioClipStretchState::default()
        };
        let next = with_mode(&state, StretchMode::Resample);
        assert!(!next.preserve_pitch);
    }

    #[test]
    fn basic_mode_maps_to_real_stretch_params() {
        let state = AudioClipStretchState::default();
        let repitch = with_basic_mode(&state, StretchBasicMode::RePitch);
        assert_eq!(repitch.mode, StretchMode::Manual);
        assert_eq!(repitch.algorithm, StretchAlgorithm::ResampleOnly);
        assert!(!repitch.preserve_pitch);

        let preserve = with_basic_mode(&state, StretchBasicMode::PreservePitch);
        assert_eq!(preserve.mode, StretchMode::Manual);
        assert_eq!(preserve.algorithm, StretchAlgorithm::PhaseVocoder);
        assert!(preserve.preserve_pitch);
    }
}

fn stretch_backend_summary(s: &AudioClipStretchState) -> &'static str {
    match stretch_basic_mode(s) {
        StretchBasicMode::Off => "Off",
        StretchBasicMode::RePitch => "RePitch",
        StretchBasicMode::PreservePitch => "Signalsmith",
    }
}

fn seconds_to_time_label(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    let minutes = (seconds / 60.0).floor() as u64;
    let rem = seconds - minutes as f64 * 60.0;
    format!("{minutes:02}:{rem:06.3}")
}

fn stretch_length_summary(
    s: &AudioClipStretchState,
    project_bpm: f64,
    fallback_duration_seconds: Option<f64>,
) -> String {
    let sample_rate = s.project_sample_rate.max(s.original_sample_rate).max(1) as f64;
    let source_seconds = if s.source_len_samples() > 0 {
        s.source_len_samples() as f64 / sample_rate
    } else {
        fallback_duration_seconds.unwrap_or(0.0)
    };
    let new_seconds = source_seconds * s.effective_time_ratio(project_bpm).max(0.0);
    format!(
        "{} -> {}",
        seconds_to_time_label(source_seconds),
        seconds_to_time_label(new_seconds)
    )
}

fn stretch_field_block(label: impl Into<String>, control: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(3.0))
        .py(px(2.0))
        .child(
            div()
                .text_size(px(10.0))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(Colors::text_muted())
                .child(label.into()),
        )
        .child(control)
}

fn stretch_metric_row(label: impl Into<String>, value: impl Into<String>) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .min_w(px(0.0))
        .child(
            div()
                .text_size(px(10.0))
                .text_color(Colors::text_muted())
                .child(label.into()),
        )
        .child(
            div()
                .min_w(px(0.0))
                .truncate()
                .text_size(px(10.5))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(Colors::text_secondary())
                .child(value.into()),
        )
}

/// STRETCH section body — compact Basic mode with real state-backed actions.
fn stretch_section_body(
    clip: &SelectedClipSummary<'_>,
    s: &AudioClipStretchState,
    project_bpm: f64,
    tempo: &StretchTempoUiSnapshot,
    cb: &ClipStretchCb,
    callbacks: &InspectorCallbacks,
) -> impl IntoElement {
    let clip_id = clip.clip_id.to_string();
    let stretch_enabled = s.mode != StretchMode::Off;
    let preserve_mode = stretch_basic_mode(s) == StretchBasicMode::PreservePitch;
    let (semi, fine) = s.pitch_semi_and_cents();
    let cur_src_bpm = s
        .bpm_source
        .or(tempo.suggested_bpm.map(f64::from))
        .unwrap_or(project_bpm);
    let src_bpm_label = s
        .bpm_source
        .map(|b| format!("{b:.2}"))
        .or_else(|| tempo.suggested_bpm.map(|b| format!("{b:.2} ?")))
        .unwrap_or_else(|| "—".to_string());
    let target_display = if matches!(s.mode, StretchMode::TempoSync) || s.bpm_target.is_none() {
        format!("Project {project_bpm:.2}")
    } else {
        format!("Manual {:.2}", s.bpm_target.unwrap_or(project_bpm))
    };
    let ratio = s.effective_time_ratio(project_bpm);
    let length_summary = stretch_length_summary(s, project_bpm, clip.source_duration_seconds);
    let backend = stretch_backend_summary(s);
    let pitch_summary = format!("{:+.2} st / {:+.0} ct", semi, fine);
    let mut fit_selection = s.clone();
    let fit_selection_enabled = clip
        .selection_duration_beats
        .map(|beats| fit_selection.fit_to_timeline_beats(beats as f64, project_bpm))
        .unwrap_or(false);
    let mut fit_clip = s.clone();
    let fit_clip_enabled = fit_clip.fit_to_timeline_beats(clip.duration_beats as f64, project_bpm);
    let mut reset = s.clone();
    reset.reset_stretch_defaults();
    let auto_find = callbacks.on_clip_stretch_auto_find_bpm.clone();
    let fit_project = callbacks.on_clip_stretch_fit_project.clone();
    let auto_find_id = clip_id.clone();
    let fit_project_id = clip_id.clone();
    let finding = tempo.finding;
    let auto_find_label = if finding { "Finding..." } else { "Auto Find" };

    div()
        .flex()
        .flex_col()
        .gap(px(7.0))
        .child(stretch_field_block(
            "Enable Stretch",
            shared_inspector_checkbox(
                "clip-stretch-enabled",
                stretch_enabled,
                false,
                if stretch_enabled { "On" } else { "Off" },
                {
                    let s = s.clone();
                    let cb = cb.clone();
                    let clip_id = clip_id.clone();
                    move |checked, w, cx| {
                        let next = if checked {
                            with_basic_mode(&s, StretchBasicMode::RePitch)
                        } else {
                            with_basic_mode(&s, StretchBasicMode::Off)
                        };
                        cb(&(clip_id.clone(), next), w, cx);
                    }
                },
            ),
        ))
        .child(stretch_field_block(
            "Mode",
            inspector_select(
                "clip-stretch-mode",
                if stretch_enabled {
                    stretch_basic_mode(s)
                } else {
                    StretchBasicMode::RePitch
                },
                STRETCH_ACTIVE_MODE_OPTIONS,
                !stretch_enabled,
                {
                    let s = s.clone();
                    let cb = cb.clone();
                    let clip_id = clip_id.clone();
                    move |mode, w, cx| {
                        cb(&(clip_id.clone(), with_basic_mode(&s, mode)), w, cx);
                    }
                },
            ),
        ))
        .child(
            div()
                .text_size(px(10.0))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(Colors::text_muted())
                .child("Tempo"),
        )
        .child(stretch_field_block(
            "Source BPM",
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(4.0))
                        .child(inspector_mini_button(
                            "clip-stretch-auto-tempo",
                            auto_find_label,
                            !finding,
                            move |_, w, cx| auto_find(&auto_find_id, w, cx),
                        ))
                        .child(inspector_numeric_stepper(
                            "clip-stretch-srcbpm",
                            cur_src_bpm,
                            src_bpm_label,
                            1.0,
                            999.0,
                            1.0,
                            !stretch_enabled,
                            {
                                let s = s.clone();
                                let cb = cb.clone();
                                let clip_id = clip_id.clone();
                                move |bpm, w, cx| {
                                    let mut next = s.clone();
                                    next.bpm_source = Some(bpm);
                                    next.clip_timeline_duration_beats = 0.0;
                                    next.dirty = true;
                                    cb(&(clip_id.clone(), next), w, cx);
                                }
                            },
                        )),
                )
                .children(
                    tempo
                        .error
                        .as_ref()
                        .map(|error| inspector_hint_text(format!("{error}"))),
                )
                .children(tempo.confidence.map(|confidence| {
                    inspector_hint_text(format!(
                        "Detected confidence: {:.0}%{}",
                        confidence * 100.0,
                        if tempo.low_confidence { " (low)" } else { "" }
                    ))
                }))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .gap(px(4.0))
                        .child({
                            let s = s.clone();
                            let cb = cb.clone();
                            let clip_id = clip_id.clone();
                            inspector_mini_button(
                                "clip-stretch-bpm-half",
                                "x0.5",
                                s.bpm_source.is_some(),
                                move |_, w, cx| {
                                    let Some(bpm) = s.bpm_source else {
                                        return;
                                    };
                                    let mut next = s.clone();
                                    next.bpm_source = Some((bpm * 0.5).clamp(1.0, 999.0));
                                    next.clip_timeline_duration_beats = 0.0;
                                    next.dirty = true;
                                    cb(&(clip_id.clone(), next), w, cx);
                                },
                            )
                        })
                        .child({
                            let s = s.clone();
                            let cb = cb.clone();
                            let clip_id = clip_id.clone();
                            inspector_mini_button(
                                "clip-stretch-bpm-double",
                                "x2",
                                s.bpm_source.is_some(),
                                move |_, w, cx| {
                                    let Some(bpm) = s.bpm_source else {
                                        return;
                                    };
                                    let mut next = s.clone();
                                    next.bpm_source = Some((bpm * 2.0).clamp(1.0, 999.0));
                                    next.clip_timeline_duration_beats = 0.0;
                                    next.dirty = true;
                                    cb(&(clip_id.clone(), next), w, cx);
                                },
                            )
                        })
                        .child({
                            let s = s.clone();
                            let cb = cb.clone();
                            let clip_id = clip_id.clone();
                            inspector_mini_button(
                                "clip-stretch-bpm-match-project",
                                "Match Project",
                                true,
                                move |_, w, cx| {
                                    let mut next = s.clone();
                                    next.bpm_source = Some(project_bpm);
                                    next.clip_timeline_duration_beats = 0.0;
                                    next.dirty = true;
                                    cb(&(clip_id.clone(), next), w, cx);
                                },
                            )
                        }),
                )
                .children((!tempo.alternatives.is_empty()).then(|| {
                    let cb = cb.clone();
                    let s = s.clone();
                    let clip_id = clip_id.clone();
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(3.0))
                        .child(
                            div()
                                .text_size(px(9.5))
                                .text_color(Colors::text_faint())
                                .child("Alternatives"),
                        )
                        .child(div().flex().flex_row().flex_wrap().gap(px(4.0)).children(
                            tempo.alternatives.iter().map(|alt| {
                                let alt = *alt as f64;
                                let label = format!("{alt:.1}");
                                let cb = cb.clone();
                                let s = s.clone();
                                let clip_id = clip_id.clone();
                                inspector_mini_button(
                                    format!("clip-stretch-alt-{alt:.1}"),
                                    label,
                                    true,
                                    move |_, w, cx| {
                                        let mut next = s.clone();
                                        next.bpm_source = Some(alt);
                                        next.clip_timeline_duration_beats = 0.0;
                                        next.dirty = true;
                                        cb(&(clip_id.clone(), next), w, cx);
                                    },
                                )
                            }),
                        ))
                })),
        ))
        .child(stretch_field_block(
            "Target BPM",
            div()
                .h(px(24.0))
                .flex()
                .items_center()
                .px(px(7.0))
                .rounded_md()
                .border(px(1.0))
                .border_color(Colors::border_subtle())
                .bg(Colors::surface_input())
                .text_size(px(11.0))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(Colors::text_secondary())
                .child(target_display.clone()),
        ))
        .child(stretch_field_block(
            "Fit",
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap(px(4.0))
                .child(inspector_mini_button(
                    "clip-fit-project-tempo",
                    "Fit Project",
                    !finding,
                    move |_, w, cx| fit_project(&fit_project_id, w, cx),
                ))
                .child(inspector_mini_button(
                    "clip-fit-selection",
                    "Fit Selection",
                    fit_selection_enabled,
                    {
                        let cb = cb.clone();
                        let clip_id = clip_id.clone();
                        move |_, w, cx| {
                            cb(&(clip_id.clone(), fit_selection.clone()), w, cx);
                        }
                    },
                ))
                .child(inspector_mini_button(
                    "clip-fit-length",
                    "Fit Clip",
                    fit_clip_enabled,
                    {
                        let cb = cb.clone();
                        let clip_id = clip_id.clone();
                        move |_, w, cx| {
                            cb(&(clip_id.clone(), fit_clip.clone()), w, cx);
                        }
                    },
                )),
        ))
        .child(
            div()
                .text_size(px(10.0))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(Colors::text_muted())
                .child("Pitch"),
        )
        .children(
            (!preserve_mode && stretch_enabled)
                .then(|| inspector_hint_text("Pitch shift requires Preserve Pitch mode")),
        )
        .child(stretch_field_block(
            "Semi",
            inspector_numeric_stepper(
                "clip-pitch-semi",
                semi as f64,
                format!("{:+.2}", semi),
                -48.0,
                48.0,
                1.0,
                !stretch_enabled || !preserve_mode,
                {
                    let s = s.clone();
                    let cb = cb.clone();
                    let clip_id = clip_id.clone();
                    move |semi, w, cx| {
                        let (_, fine) = s.pitch_semi_and_cents();
                        let mut next = s.clone();
                        next.set_pitch_semi_and_cents(semi as f32, fine);
                        cb(&(clip_id.clone(), next), w, cx);
                    }
                },
            ),
        ))
        .child(stretch_field_block(
            "Fine",
            inspector_numeric_stepper(
                "clip-pitch-fine",
                fine as f64,
                format!("{fine:+.0} ct"),
                -99.0,
                99.0,
                50.0,
                !stretch_enabled || !preserve_mode,
                {
                    let s = s.clone();
                    let cb = cb.clone();
                    let clip_id = clip_id.clone();
                    move |fine, w, cx| {
                        let (semi, _) = s.pitch_semi_and_cents();
                        let mut next = s.clone();
                        next.set_pitch_semi_and_cents(semi, fine as f32);
                        cb(&(clip_id.clone(), next), w, cx);
                    }
                },
            ),
        ))
        .child(stretch_field_block(
            "",
            inspector_mini_button(
                "clip-reset-pitch",
                "Reset Pitch",
                stretch_enabled && preserve_mode,
                {
                    let s = s.clone();
                    let cb = cb.clone();
                    let clip_id = clip_id.clone();
                    move |_, w, cx| {
                        let mut next = s.clone();
                        next.reset_pitch();
                        cb(&(clip_id.clone(), next), w, cx);
                    }
                },
            ),
        ))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .pt(px(5.0))
                .border_t(px(1.0))
                .border_color(Colors::divider())
                .child(
                    div()
                        .text_size(px(10.0))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(Colors::text_muted())
                        .child("Result"),
                )
                .child(inspector_mini_button(
                    "clip-reset-stretch",
                    "Reset Stretch",
                    true,
                    {
                        let cb = cb.clone();
                        let clip_id = clip_id.clone();
                        move |_, w, cx| {
                            cb(&(clip_id.clone(), reset.clone()), w, cx);
                        }
                    },
                )),
        )
        .child(stretch_metric_row("Ratio", format!("{ratio:.3}x")))
        .children(
            s.bpm_source
                .map(|b| stretch_metric_row("Source", format!("{b:.2} BPM"))),
        )
        .child(stretch_metric_row("Target", target_display))
        .child(stretch_metric_row("Pitch", pitch_summary))
        .child(stretch_metric_row("Length", length_summary))
        .child(stretch_metric_row("Backend", backend))
        .children((stretch_enabled && !preserve_mode).then(|| {
            inspector_hint_text(format!(
                "Pitch follows speed. Extra pitch: {}",
                format!("{:+.2} st", semi)
            ))
        }))
        .children(
            (stretch_enabled && preserve_mode).then(|| {
                inspector_hint_text(format!("Pitch preserved. Pitch shift: {:+.2} st", semi))
            }),
        )
        .children(
            (!stretch_enabled)
                .then(|| inspector_hint_text("Stretch is off; playback uses default params.")),
        )
        .children((!fit_selection_enabled).then(|| {
            inspector_hint_text("Fit Selection enables when an arrangement time range is selected.")
        }))
        .child(stretch_field_block(
            "Advanced",
            inspector_hint_text(
                "Formant, transient, warp markers, and quality — not available yet.",
            ),
        ))
}

/// PITCH section body — semitone / fine-cents / formant.
fn pitch_section_body(
    clip_id: &str,
    s: &AudioClipStretchState,
    cb: &ClipStretchCb,
) -> impl IntoElement {
    let semi = s.pitch_shift_semitones.trunc() as f64;
    let fine = ((s.pitch_shift_semitones as f64 - semi) * 100.0).round();
    let pitch_note = (s.preserve_pitch && mode_supports_preserve_pitch(s.mode))
        .then_some("Independent pitch shift pending in preserve mode");

    div()
        .flex()
        .flex_col()
        .gap(px(3.0))
        .children(pitch_note.map(inspector_hint_text))
        .child(shared_inspector_row(
            "Semi",
            false,
            inspector_numeric_stepper(
                "clip-pitch-semi",
                semi,
                format!("{:+.2} st", s.pitch_shift_semitones),
                -48.0,
                48.0,
                1.0,
                false,
                {
                    let s = s.clone();
                    let cb = cb.clone();
                    let clip_id = clip_id.to_string();
                    move |semi, w, cx| {
                        let fine = (s.pitch_shift_semitones as f64
                            - s.pitch_shift_semitones.trunc() as f64)
                            as f32;
                        let mut next = s.clone();
                        next.pitch_shift_semitones = (semi as f32 + fine).clamp(-48.0, 48.0);
                        next.dirty = true;
                        cb(&(clip_id.clone(), next), w, cx);
                    }
                },
            ),
        ))
        .child(shared_inspector_row(
            "Fine",
            false,
            inspector_numeric_stepper(
                "clip-pitch-fine",
                fine,
                format!("{fine:+.0} ct"),
                -99.0,
                99.0,
                50.0,
                false,
                {
                    let s = s.clone();
                    let cb = cb.clone();
                    let clip_id = clip_id.to_string();
                    move |fine, w, cx| {
                        let semi = s.pitch_shift_semitones.trunc();
                        let mut next = s.clone();
                        next.pitch_shift_semitones =
                            (semi + (fine as f32 / 100.0)).clamp(-48.0, 48.0);
                        next.dirty = true;
                        cb(&(clip_id.clone(), next), w, cx);
                    }
                },
            ),
        ))
        .child(shared_inspector_row(
            "Formant",
            false,
            shared_inspector_checkbox(
                "clip-pitch-formant",
                s.formant_preserve,
                false,
                if s.formant_preserve { "On" } else { "Off" },
                {
                    let s = s.clone();
                    let cb = cb.clone();
                    let clip_id = clip_id.to_string();
                    move |checked, w, cx| {
                        let mut next = s.clone();
                        next.formant_preserve = checked;
                        next.dirty = true;
                        cb(&(clip_id.clone(), next), w, cx);
                    }
                },
            ),
        ))
}

/// TRANSIENT section body — preserve + sensitivity.
fn transient_section_body(
    clip_id: &str,
    s: &AudioClipStretchState,
    cb: &ClipStretchCb,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(3.0))
        .child(shared_inspector_row(
            "Preserve",
            false,
            shared_inspector_checkbox(
                "clip-trans-preserve",
                s.transient_preserve,
                false,
                if s.transient_preserve { "On" } else { "Off" },
                {
                    let s = s.clone();
                    let cb = cb.clone();
                    let clip_id = clip_id.to_string();
                    move |checked, w, cx| {
                        let mut next = s.clone();
                        next.transient_preserve = checked;
                        next.dirty = true;
                        cb(&(clip_id.clone(), next), w, cx);
                    }
                },
            ),
        ))
        .child(shared_inspector_row(
            "Sensitivity",
            false,
            inspector_numeric_stepper(
                "clip-trans-sens",
                s.transient_sensitivity as f64,
                format!("{:.2}", s.transient_sensitivity),
                0.0,
                1.0,
                0.05,
                false,
                {
                    let s = s.clone();
                    let cb = cb.clone();
                    let clip_id = clip_id.to_string();
                    move |value, w, cx| {
                        let mut next = s.clone();
                        next.transient_sensitivity = value as f32;
                        next.dirty = true;
                        cb(&(clip_id.clone(), next), w, cx);
                    }
                },
            ),
        ))
}

/// WARP section body — marker count + add/clear, with an honest pending note.
fn warp_section_body(
    clip_id: &str,
    s: &AudioClipStretchState,
    callbacks: &InspectorCallbacks,
) -> impl IntoElement {
    let add = callbacks.on_clip_warp_add_at_playhead.clone();
    let clear = callbacks.on_clip_warp_clear.clone();
    let add_id = clip_id.to_string();
    let clear_id = clip_id.to_string();
    let has_markers = !s.warp_markers.is_empty();

    div()
        .flex()
        .flex_col()
        .gap(px(3.0))
        .child(shared_inspector_row(
            "Markers",
            false,
            inspector_value(s.warp_markers.len().to_string()),
        ))
        .child(shared_inspector_row(
            "",
            false,
            div()
                .flex()
                .flex_row()
                .gap(px(4.0))
                .child(inspector_mini_button(
                    "clip-warp-add",
                    "Add at Playhead",
                    true,
                    move |_, w, cx| add(&add_id, w, cx),
                ))
                .child(inspector_mini_button(
                    "clip-warp-clear",
                    "Clear",
                    has_markers,
                    move |_, w, cx| clear(&clear_id, w, cx),
                )),
        ))
        .child(inspector_hint_text(
            "Warp markers stored; playback uses global stretch",
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
    tempo: StretchTempoUiSnapshot,
    callbacks: &InspectorCallbacks,
) -> impl IntoElement {
    let clip_id = clip.clip_id.to_string();
    let open_bottom = callbacks.on_open_clip_bottom_editor.clone();
    let open_external = callbacks.on_open_clip_external_midi_editor.clone();

    if clip.kind == "Audio" {
        let source_duration = clip
            .source_duration_seconds
            .map(|seconds| format!("{seconds:.2} s"))
            .unwrap_or_else(|| "Pending".to_string());
        let file_name = clip
            .source_path
            .map(file_name_from_path)
            .unwrap_or_else(|| "Missing source".to_string());
        let path = clip.source_path.unwrap_or("-");
        let gain_db = linear_gain_to_db(clip.gain);
        let muted_id = clip.clip_id.to_string();
        let mute_cb = callbacks.on_set_clip_muted.clone();
        let s = clip.stretch;
        let stretch_cb = callbacks.on_set_clip_stretch.clone();
        // Precomputed next-states for the inline AUDIO-section stretch controls.
        let reverse_next = {
            let mut n = s.clone();
            n.reverse = !s.reverse;
            n.dirty = true;
            n
        };
        let mut body = scroll_body()
            .gap(px(10.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .w(px(4.0))
                                    .h(px(30.0))
                                    .rounded_sm()
                                    .bg(Colors::accent_primary()),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(13.0))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(Colors::text_primary())
                                    .child(clip.name.to_string()),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .px(px(7.0))
                                    .py(px(2.0))
                                    .rounded_sm()
                                    .bg(Colors::with_alpha(Colors::accent_primary(), 0.16))
                                    .text_size(px(9.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(Colors::accent_primary())
                                    .child("Audio Clip"),
                            ),
                    )
                    .child(
                        div()
                            .pl(px(12.0))
                            .min_w_0()
                            .truncate()
                            .text_size(px(10.5))
                            .text_color(Colors::text_muted())
                            .child(format!(
                                "{} • {} source • Gain {:.1} dB",
                                clip.track_name, source_duration, gain_db
                            )),
                    ),
            )
            .child(inspector_section(
                "CLIP",
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.0))
                    .child(compact_property_row(
                        "Name",
                        text_field(clip_name_input, clip_name_focused),
                    )),
            ))
            .child(inspector_section(
                "TIMING",
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.0))
                    .child(compact_property_row(
                        "Start",
                        beat_stepper(
                            "clip-start",
                            clip.clip_id,
                            clip.start_beat,
                            callbacks.on_set_clip_start.clone(),
                            0.0,
                        ),
                    ))
                    .child(compact_property_row(
                        "Length",
                        beat_stepper(
                            "clip-length",
                            clip.clip_id,
                            clip.duration_beats,
                            callbacks.on_set_clip_length.clone(),
                            0.25,
                        ),
                    ))
                    .child(compact_property_row(
                        "End",
                        readonly_value(format!("{:.2} bt", clip.start_beat + clip.duration_beats)),
                    )),
            ))
            .child(inspector_section(
                "AUDIO",
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.0))
                    .child(compact_property_row(
                        "Muted",
                        fb_checkbox("clip-muted", "Muted", clip.muted, true, move |_, w, cx| {
                            mute_cb(&(muted_id.clone(), !clip.muted), w, cx)
                        }),
                    ))
                    .child(compact_property_row(
                        "Gain",
                        gain_stepper(clip.clip_id, clip.gain, callbacks.on_set_clip_gain.clone()),
                    ))
                    .child(compact_property_row(
                        "Reverse",
                        shared_inspector_checkbox(
                            "clip-reverse",
                            s.reverse,
                            false,
                            if s.reverse { "On" } else { "Off" },
                            {
                                let clip_id = clip.clip_id.to_string();
                                let stretch_cb = stretch_cb.clone();
                                move |_, w, cx| {
                                    stretch_cb(&(clip_id.clone(), reverse_next.clone()), w, cx);
                                }
                            },
                        ),
                    ))
                    .child(compact_property_row(
                        "Normalize",
                        shared_inspector_checkbox(
                            "clip-normalize",
                            s.normalize_gain,
                            true,
                            "Pending",
                            |_, _, _| {},
                        ),
                    ))
                    .child(compact_property_row(
                        "Fade In",
                        inspector_numeric_stepper(
                            "clip-fade-in",
                            s.fade_in_ms as f64,
                            format!("{:.0} ms", s.fade_in_ms),
                            0.0,
                            60_000.0,
                            5.0,
                            false,
                            {
                                let s = s.clone();
                                let stretch_cb = stretch_cb.clone();
                                let clip_id = clip.clip_id.to_string();
                                move |value, w, cx| {
                                    let mut next = s.clone();
                                    next.fade_in_ms = value as f32;
                                    next.dirty = true;
                                    stretch_cb(&(clip_id.clone(), next), w, cx);
                                }
                            },
                        ),
                    ))
                    .child(compact_property_row(
                        "Fade Out",
                        inspector_numeric_stepper(
                            "clip-fade-out",
                            s.fade_out_ms as f64,
                            format!("{:.0} ms", s.fade_out_ms),
                            0.0,
                            60_000.0,
                            5.0,
                            false,
                            {
                                let s = s.clone();
                                let stretch_cb = stretch_cb.clone();
                                let clip_id = clip.clip_id.to_string();
                                move |value, w, cx| {
                                    let mut next = s.clone();
                                    next.fade_out_ms = value as f32;
                                    next.dirty = true;
                                    stretch_cb(&(clip_id.clone(), next), w, cx);
                                }
                            },
                        ),
                    )),
            ))
            .child(shared_inspector_section(
                "Audio Stretch",
                None::<String>,
                stretch_section_body(&clip, s, clip.project_bpm, &tempo, &stretch_cb, callbacks),
            ))
            .child(shared_inspector_section(
                "Source",
                None::<String>,
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.0))
                    .child(shared_inspector_row(
                        "File",
                        false,
                        truncate_value(file_name),
                    ))
                    .child(shared_inspector_row(
                        "Duration",
                        false,
                        truncate_value(source_duration),
                    ))
                    .child(shared_inspector_row(
                        "Path",
                        false,
                        truncate_value(path.to_string()),
                    ))
                    .child(shared_inspector_row(
                        "",
                        false,
                        div()
                            .flex()
                            .flex_row()
                            .gap(px(4.0))
                            // TODO(source-actions): reveal/replace need shell + relink callbacks.
                            .child(compact_action_button(
                                "clip-reveal",
                                "Reveal",
                                false,
                                |_, _, _| {},
                            ))
                            .child(compact_action_button(
                                "clip-replace",
                                "Replace",
                                false,
                                |_, _, _| {},
                            )),
                    )),
            ));

        if std::env::var_os("FUTUREBOARD_INSPECTOR_DEBUG").is_some() {
            body = body.child(inspector_section(
                "DEBUG",
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(kv_row("Track ID", clip.track_id.to_string())),
            ));
        }

        return body;
    }

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
