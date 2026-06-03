//! MixerPanel — the bottom-panel mixer view.
//!
//! Layout structure:
//!
//! ```text
//! ┌─ mixer_sub_header ───────────────────────────────────────────────────┐
//! ├──────────────────────────────────────────────────────────────────────┤
//! │                                                                      │
//! │  channel_scroll_area (flex_1, overflow-x scroll)        │  master    │
//! │  ┌───────┐┌───────┐┌───────┐ ...                        │  block     │
//! │  │ strip ││ strip ││ strip │                            │ (fixed)    │
//! │  └───────┘└───────┘└───────┘                            │            │
//! │                                                          │            │
//! └──────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! * Channel strips are a horizontal flex row inside the scroll area; they
//!   never share width with the master block.
//! * The master block is pinned to the right edge and has its own bordered
//!   gutter so the empty middle (when track count is small) reads as
//!   intentional, not as floating dead space.
//! * Strip internals are a vertical flex with explicit per-section heights;
//!   only the fader area grows to fill remaining height.

use gpui::prelude::FluentBuilder;
use gpui::{div, px, svg, App, InteractiveElement, IntoElement, ParentElement, Styled, Window};

use crate::assets;
use crate::components::fader::{db_scale_column, db_value_pill, fader as render_fader};
use crate::components::knob::knob_bipolar;
use crate::components::timeline::timeline_state::{
    volume, InsertLoadStatus, InsertSlotState, MasterBusState, SendSlotState, TrackState, TrackType,
};
use crate::components::timeline::vu_meter::meter_surface;
use crate::theme::Colors;

// ── Section dimensions ─────────────────────────────────────────────────────
const STRIP_WIDTH: f32 = 88.0;
/// Minimum height for a channel strip. Fixed sections sum to 236; below this
/// the fader area still renders (it uses `flex_1` and `h_full` internals so
/// the rail/meter/scale shrink with the slot) but the rail becomes hard to
/// read. The scroll area shows a vertical scrollbar below this threshold.
/// Sections: header 40 + inserts 44 + sends 44 + pan 60 + buttons 24 +
/// footer 22 = 234 + at least ~86 for the fader cluster.
const STRIP_MIN_HEIGHT: f32 = 320.0;

const SEC_HEADER_H: f32 = 40.0;
const SEC_INSERTS_H: f32 = 44.0;
const SEC_SENDS_H: f32 = 44.0;
const SEC_PAN_H: f32 = 60.0;
const SEC_BUTTONS_H: f32 = 24.0;
const SEC_FOOTER_H: f32 = 22.0;

/// Maximum insert slots per track. Once reached, the trailing empty "+ Add
/// Insert" slot and the INSERTS header "+" are hidden/disabled.
const MAX_INSERT_SLOTS: usize = 8;

/// Optional clickable "+" affordance for a [`section_header`]. `None` renders
/// the inert decorative plus (used by SENDS and the master strip); `Some`
/// renders an interactive, hit-tested plus that runs `on_click`.
struct HeaderPlus {
    id: gpui::SharedString,
    on_click: std::sync::Arc<dyn Fn(&mut gpui::Window, &mut gpui::App)>,
}

/// Bundle of mixer interactions hooked up from the layout. Closures land in
/// the same TimelineState mutation methods used by the TrackHeader so the two
/// views can never disagree.
#[derive(Clone)]
pub struct MixerCallbacks {
    pub on_select_track:
        std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>,
    pub on_volume_change:
        std::sync::Arc<dyn Fn(&(String, f32), &mut gpui::Window, &mut gpui::App) + 'static>,
    pub on_pan_change:
        std::sync::Arc<dyn Fn(&(String, f32), &mut gpui::Window, &mut gpui::App) + 'static>,
    pub on_toggle_mute:
        std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>,
    pub on_toggle_solo:
        std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>,
    pub on_toggle_arm: std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>,
    pub on_toggle_input:
        std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>,
    pub on_master_volume_change:
        std::sync::Arc<dyn Fn(&f32, &mut gpui::Window, &mut gpui::App) + 'static>,
    pub on_context_menu: Option<
        std::sync::Arc<dyn Fn(&(String, f32, f32), &mut gpui::Window, &mut gpui::App) + 'static>,
    >,
    /// Open the insert plugin picker overlay for the track (Phase 2b). The
    /// slot is created only when the user picks a plugin; an empty registry
    /// offers a stub fallback so the project round-trip stays exercisable.
    pub on_add_insert: std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>,
    /// Remove the named insert slot from the track.
    pub on_remove_insert:
        std::sync::Arc<dyn Fn(&(String, String), &mut gpui::Window, &mut gpui::App) + 'static>,
    /// Toggle bypass on the named insert slot.
    pub on_toggle_insert_bypass:
        std::sync::Arc<dyn Fn(&(String, String), &mut gpui::Window, &mut gpui::App) + 'static>,
    /// Reorder the named insert slot within the chain. `(track_id, insert_id,
    /// up)` — `up = true` moves it one position earlier, `false` later.
    pub on_move_insert: std::sync::Arc<
        dyn Fn(&(String, String, bool), &mut gpui::Window, &mut gpui::App) + 'static,
    >,
    /// User clicked the slot chip — Phase 4 will open the native plugin
    /// editor; Phase 1 logs the request.
    pub on_open_insert_editor: std::sync::Arc<
        dyn Fn(&(String, usize, String), &mut gpui::Window, &mut gpui::App) + 'static,
    >,
    /// Add an aux send from the track to the first available Bus/Return
    /// (Phase 3). A target picker is a follow-up.
    pub on_add_send: std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>,
    /// Remove the named send `(track_id, send_id)`.
    pub on_remove_send:
        std::sync::Arc<dyn Fn(&(String, String), &mut gpui::Window, &mut gpui::App) + 'static>,
}

/// Inert callbacks for fallback UI when the studio entity is unavailable.
pub fn noop_mixer_callbacks() -> MixerCallbacks {
    use std::sync::Arc;

    let noop_track = Arc::new(|_: &String, _: &mut Window, _: &mut App| {});
    let noop_vol = Arc::new(|_: &(String, f32), _: &mut Window, _: &mut App| {});
    let noop_pan = Arc::new(|_: &(String, f32), _: &mut Window, _: &mut App| {});
    let noop_master = Arc::new(|_: &f32, _: &mut Window, _: &mut App| {});
    let noop_insert_pair = Arc::new(|_: &(String, String), _: &mut Window, _: &mut App| {});
    let noop_insert_open = Arc::new(|_: &(String, usize, String), _: &mut Window, _: &mut App| {});
    let noop_insert_move = Arc::new(|_: &(String, String, bool), _: &mut Window, _: &mut App| {});
    MixerCallbacks {
        on_select_track: noop_track.clone(),
        on_volume_change: noop_vol,
        on_pan_change: noop_pan,
        on_toggle_mute: noop_track.clone(),
        on_toggle_solo: noop_track.clone(),
        on_toggle_arm: noop_track.clone(),
        on_toggle_input: noop_track.clone(),
        on_master_volume_change: noop_master,
        on_context_menu: None,
        on_add_insert: noop_track.clone(),
        on_remove_insert: noop_insert_pair.clone(),
        on_toggle_insert_bypass: noop_insert_pair.clone(),
        on_move_insert: noop_insert_move,
        on_open_insert_editor: noop_insert_open.clone(),
        on_add_send: noop_track,
        on_remove_send: noop_insert_pair,
    }
}

// ─── Mixer sub-header ("Mixer  N ch") ────────────────────────────────────────

fn mixer_sub_header(track_count: usize) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .h(px(30.0))
        .px(px(10.0))
        .border_b(px(1.0))
        .border_color(Colors::divider())
        .child(
            svg()
                .path(assets::ICON_SLIDERS_HORIZONTAL_PATH)
                .w(px(14.0))
                .h(px(14.0))
                .text_color(Colors::text_faint()),
        )
        .child(
            div()
                .text_size(px(10.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(Colors::text_primary())
                .child("Mixer"),
        )
        .child(
            div()
                .flex()
                .items_center()
                .px(px(5.0))
                .py(px(1.0))
                .rounded_md()
                .bg(Colors::slot_bg())
                .border(px(1.0))
                .border_color(Colors::slot_border())
                .text_size(px(9.0))
                .text_color(Colors::text_faint())
                .child(format!("{} ch", track_count)),
        )
}

// ─── Section header ──────────────────────────────────────────────────────────

fn section_header(
    label: &'static str,
    accent: gpui::Rgba,
    plus: Option<HeaderPlus>,
) -> impl IntoElement {
    let icon_path = match label {
        "INSERTS" => Some(assets::ICON_PLUG_PATH),
        "SENDS" => Some(assets::ICON_ROUTE_PATH),
        _ => None,
    };
    let soft_accent = Colors::with_alpha(accent, 0.55); // Approved: dynamic accent decoration alpha

    // The trailing "+" — interactive when `plus` is Some, otherwise an inert
    // decorative glyph. Interactive variant carries its own id + occlude so the
    // strip's track-select mouse handler can't swallow the click.
    let plus_el: gpui::AnyElement = match plus {
        Some(HeaderPlus { id, on_click }) => div()
            .id(id)
            .w(px(12.0))
            .h(px(12.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded_sm()
            .cursor(gpui::CursorStyle::PointingHand)
            .hover(|s| s.bg(Colors::surface_control_hover()))
            .child(
                svg()
                    .path(assets::ICON_PLUS_PATH)
                    .w(px(9.0))
                    .h(px(9.0))
                    .text_color(Colors::text_muted()),
            )
            .on_mouse_down(gpui::MouseButton::Left, move |_e, w, cx| {
                on_click(w, cx);
            })
            .occlude()
            .into_any_element(),
        None => div()
            .w(px(12.0))
            .h(px(12.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded_sm()
            .child(
                svg()
                    .path(assets::ICON_PLUS_PATH)
                    .w(px(9.0))
                    .h(px(9.0))
                    .text_color(Colors::text_faint()),
            )
            .into_any_element(),
    };

    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(3.0))
        .px(px(5.0))
        .py(px(3.0))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(4.0))
                .child(div().w(px(2.0)).h(px(8.0)).rounded_full().bg(soft_accent))
                .children(icon_path.map(|path| {
                    svg()
                        .path(path)
                        .w(px(9.0))
                        .h(px(9.0))
                        .text_color(Colors::text_muted())
                }))
                .child(
                    div()
                        .text_size(px(7.5))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(Colors::text_muted())
                        .child(label),
                ),
        )
        .child(plus_el)
}

fn empty_slot() -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_center()
        .mx(px(4.0))
        .py(px(2.0))
        .rounded_sm()
        .border(px(1.0))
        .border_dashed()
        .border_color(Colors::slot_border())
        .text_size(px(8.0))
        .text_color(Colors::slot_empty_text())
        .child("empty")
}

// ─── M/S/R/I buttons ────────────────────────────────────────────────────────

fn msri_button(
    id: gpui::ElementId,
    label: &'static str,
    active: bool,
    active_bg: gpui::Rgba,
    active_fg: gpui::Rgba,
    on_click: impl Fn(&gpui::MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    let mut btn = div()
        .flex()
        .items_center()
        .justify_center()
        .h(px(16.0))
        .flex_1()
        .rounded_sm()
        .text_size(px(9.0))
        .font_weight(gpui::FontWeight::BOLD)
        .id(id)
        .cursor(gpui::CursorStyle::PointingHand)
        .on_mouse_down(gpui::MouseButton::Left, on_click)
        .child(label);

    if active {
        btn = btn.bg(active_bg).text_color(active_fg);
    } else {
        btn = btn
            .bg(Colors::slot_bg())
            .border(px(1.0))
            .border_color(Colors::slot_border())
            .text_color(Colors::text_muted())
            .hover(|s| s.bg(Colors::slot_bg_hover()));
    }
    btn
}

fn button_row(track: &TrackState, callbacks: &MixerCallbacks, id_num: usize) -> impl IntoElement {
    let track_id = track.id.clone();

    let on_mute = {
        let id = track_id.clone();
        let cb = callbacks.on_toggle_mute.clone();
        move |_: &gpui::MouseDownEvent, w: &mut gpui::Window, cx: &mut gpui::App| cb(&id, w, cx)
    };
    let on_solo = {
        let id = track_id.clone();
        let cb = callbacks.on_toggle_solo.clone();
        move |_: &gpui::MouseDownEvent, w: &mut gpui::Window, cx: &mut gpui::App| cb(&id, w, cx)
    };
    let on_arm = {
        let id = track_id.clone();
        let cb = callbacks.on_toggle_arm.clone();
        move |_: &gpui::MouseDownEvent, w: &mut gpui::Window, cx: &mut gpui::App| cb(&id, w, cx)
    };
    let on_input = {
        let id = track_id.clone();
        let cb = callbacks.on_toggle_input.clone();
        move |_: &gpui::MouseDownEvent, w: &mut gpui::Window, cx: &mut gpui::App| cb(&id, w, cx)
    };

    div()
        .flex()
        .flex_row()
        .gap(px(2.0))
        .px(px(4.0))
        .h(px(SEC_BUTTONS_H))
        .items_center()
        .child(msri_button(
            ("mix-m-btn", id_num).into(),
            "M",
            track.muted,
            Colors::accent_warning(),
            Colors::text_inverse(),
            on_mute,
        ))
        .child(msri_button(
            ("mix-s-btn", id_num).into(),
            "S",
            track.solo,
            Colors::accent_success(),
            Colors::text_inverse(),
            on_solo,
        ))
        .child(msri_button(
            ("mix-r-btn", id_num).into(),
            "R",
            track.armed,
            Colors::accent_danger(),
            Colors::text_inverse(),
            on_arm,
        ))
        .child(msri_button(
            ("mix-i-btn", id_num).into(),
            "I",
            track.input_monitor,
            Colors::accent_primary(),
            Colors::text_inverse(),
            on_input,
        ))
}

// ─── Meter ──────────────────────────────────────────────────────────────────

// ─── Strip sections ─────────────────────────────────────────────────────────

fn strip_header(track: &TrackState, index: usize) -> impl IntoElement {
    let type_label = match track.track_type {
        TrackType::Audio => "AUDIO",
        TrackType::Midi => "MIDI",
        TrackType::Instrument => "INST",
        TrackType::Bus => "BUS",
        TrackType::Return => "RTN",
        TrackType::Master => "MST",
    };

    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(4.0))
        .h(px(SEC_HEADER_H))
        .px(px(5.0))
        .border_b(px(1.0))
        .border_color(Colors::divider())
        .child(div().w(px(2.0)).h(px(20.0)).rounded_full().bg(track.color))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.0))
                .child(
                    div()
                        .min_w(px(0.0))
                        .flex_1()
                        .truncate()
                        .text_size(px(10.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(Colors::text_primary())
                        .child(track.name.clone()),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(3.0))
                        .child(
                            div()
                                .text_size(px(7.5))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(Colors::text_faint())
                                .child(type_label),
                        )
                        .child(
                            div()
                                .text_size(px(7.5))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(Colors::text_faint())
                                .child(format!("CH{:02}", index + 1)),
                        ),
                ),
        )
}

fn insert_chip(
    track_id: &str,
    insert_index: usize,
    slot: &InsertSlotState,
    callbacks: &MixerCallbacks,
) -> impl IntoElement {
    let track_id_owned = track_id.to_string();
    let slot_id = slot.id.clone();
    let display = slot.display_name.clone();
    let display_for_log = display.clone();
    let bypassed = slot.bypassed;
    let on_open = callbacks.on_open_insert_editor.clone();
    let on_bypass = callbacks.on_toggle_insert_bypass.clone();
    let on_remove = callbacks.on_remove_insert.clone();
    let on_move_up = callbacks.on_move_insert.clone();
    let on_move_down = callbacks.on_move_insert.clone();

    let (bg, text) = match &slot.load_status {
        InsertLoadStatus::Ready if !bypassed => (Colors::accent_muted(), Colors::text_primary()),
        InsertLoadStatus::Ready => (Colors::surface_input(), Colors::text_muted()),
        InsertLoadStatus::Loading => (Colors::surface_input(), Colors::text_muted()),
        InsertLoadStatus::Failed(_) => (
            Colors::with_alpha(Colors::status_error(), 0.16),
            Colors::status_error(),
        ),
        InsertLoadStatus::Disabled => (Colors::surface_input(), Colors::text_faint()),
        InsertLoadStatus::Empty => (Colors::surface_input(), Colors::slot_empty_text()),
    };

    let id_owned = slot_id.clone();
    let bypass_pair = (track_id_owned.clone(), slot_id.clone());
    let remove_pair = (track_id_owned.clone(), slot_id.clone());
    let move_up_tuple = (track_id_owned.clone(), slot_id.clone(), true);
    let move_down_tuple = (track_id_owned.clone(), slot_id.clone(), false);
    let open_target = (track_id_owned, insert_index, slot_id);

    div()
        .id(gpui::SharedString::from(format!(
            "insert-chip-{}",
            id_owned
        )))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(3.0))
        .mx(px(2.0))
        .px(px(4.0))
        .h(px(18.0))
        .rounded_sm()
        .bg(bg)
        .text_size(px(9.0))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(text)
        .cursor(gpui::CursorStyle::PointingHand)
        .on_mouse_down(gpui::MouseButton::Left, move |_e, w, cx| {
            eprintln!(
                "[mixer] insert row clicked track_id={} insert_index={} plugin={} plugin_instance_id={}",
                open_target.0, open_target.1, display_for_log, open_target.2
            );
            on_open(&open_target, w, cx);
        })
        .occlude()
        .child(div().truncate().child(display))
        // Bypass dot — small interactive square.
        .child(
            div()
                .id(gpui::SharedString::from(format!(
                    "insert-bypass-{}",
                    bypass_pair.1
                )))
                .w(px(8.0))
                .h(px(8.0))
                .rounded_sm()
                .bg(if bypassed {
                    Colors::text_faint()
                } else {
                    Colors::status_success()
                })
                .on_mouse_down(gpui::MouseButton::Left, move |_e, w, cx| {
                    on_bypass(&bypass_pair, w, cx);
                })
                .occlude(),
        )
        // Reorder up ▲.
        .child(
            div()
                .id(gpui::SharedString::from(format!(
                    "insert-up-{}",
                    move_up_tuple.1
                )))
                .text_size(px(8.0))
                .text_color(Colors::text_faint())
                .px(px(1.0))
                .cursor(gpui::CursorStyle::PointingHand)
                .hover(|s| s.text_color(Colors::text_primary()))
                .child("▲")
                .on_mouse_down(gpui::MouseButton::Left, move |_e, w, cx| {
                    on_move_up(&move_up_tuple, w, cx);
                })
                .occlude(),
        )
        // Reorder down ▼.
        .child(
            div()
                .id(gpui::SharedString::from(format!(
                    "insert-down-{}",
                    move_down_tuple.1
                )))
                .text_size(px(8.0))
                .text_color(Colors::text_faint())
                .px(px(1.0))
                .cursor(gpui::CursorStyle::PointingHand)
                .hover(|s| s.text_color(Colors::text_primary()))
                .child("▼")
                .on_mouse_down(gpui::MouseButton::Left, move |_e, w, cx| {
                    on_move_down(&move_down_tuple, w, cx);
                })
                .occlude(),
        )
        // Remove ×.
        .child(
            div()
                .id(gpui::SharedString::from(format!(
                    "insert-remove-{}",
                    remove_pair.1
                )))
                .text_size(px(10.0))
                .text_color(Colors::text_faint())
                .px(px(2.0))
                .cursor(gpui::CursorStyle::PointingHand)
                .child("×")
                .on_mouse_down(gpui::MouseButton::Left, move |_e, w, cx| {
                    on_remove(&remove_pair, w, cx);
                })
                .occlude(),
        )
}

/// Trailing empty insert slot. Clicking it opens the plugin picker for the
/// next available slot (`next_slot`) on this track. `next_slot` is used for
/// debug logging only — the picker appends to the track's insert chain.
fn add_insert_button(
    track_id: &str,
    next_slot: usize,
    callbacks: &MixerCallbacks,
) -> impl IntoElement {
    let track_id_owned = track_id.to_string();
    let on_add = callbacks.on_add_insert.clone();
    div()
        .id(gpui::SharedString::from(format!(
            "insert-add-{}",
            track_id_owned
        )))
        .flex()
        .items_center()
        .justify_center()
        .mx(px(2.0))
        .px(px(4.0))
        .h(px(18.0))
        .rounded_sm()
        .border(px(1.0))
        .border_dashed()
        .border_color(Colors::slot_border())
        .text_size(px(10.0))
        .text_color(Colors::slot_empty_text())
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|s| s.bg(Colors::surface_control_hover()))
        .child("+ Add Insert")
        .on_mouse_down(gpui::MouseButton::Left, move |_e, w, cx| {
            eprintln!(
                "[mixer] empty insert slot + clicked track={track_id_owned} slot={next_slot}"
            );
            on_add(&track_id_owned, w, cx);
        })
        .occlude()
}

fn inserts_section(
    track: &TrackState,
    _index: usize,
    callbacks: &MixerCallbacks,
) -> impl IntoElement {
    let used = track.inserts.len();
    let at_max = used >= MAX_INSERT_SLOTS;

    let mut chips = div().flex().flex_col().gap(px(2.0)).px(px(2.0));
    for (insert_index, slot) in track.inserts.iter().enumerate() {
        chips = chips.child(insert_chip(&track.id, insert_index, slot, callbacks));
    }
    // Requirement: always render one trailing empty slot after the last insert,
    // until MAX_INSERT_SLOTS is reached.
    if !at_max {
        chips = chips.child(add_insert_button(&track.id, used, callbacks));
    }

    // Header "+" adds to the next available slot for *this* track; hidden
    // (inert) once the rack is full.
    let header_plus = if at_max {
        None
    } else {
        let track_id = track.id.clone();
        let on_add = callbacks.on_add_insert.clone();
        Some(HeaderPlus {
            id: gpui::SharedString::from(format!("insert-header-add-{}", track.id)),
            on_click: std::sync::Arc::new(move |w, cx| {
                eprintln!("[mixer] INSERTS header + clicked track={track_id} slot={used}");
                on_add(&track_id, w, cx);
            }),
        })
    };

    div()
        .flex()
        .flex_col()
        // `min_h` (not fixed `h`) so the trailing "+ Add Insert" slot is never
        // clipped once a plugin chip is present — the fader area (flex_1)
        // absorbs the difference. This was the root cause of the missing slot.
        .min_h(px(SEC_INSERTS_H))
        .border_b(px(1.0))
        .border_color(Colors::divider())
        .child(section_header("INSERTS", track.color, header_plus))
        .child(chips)
}

fn send_chip(
    track_id: &str,
    send: &SendSlotState,
    target_name: &str,
    callbacks: &MixerCallbacks,
) -> impl IntoElement {
    let remove_pair = (track_id.to_string(), send.id.clone());
    let on_remove = callbacks.on_remove_send.clone();
    let (bg, text) = if send.enabled {
        (Colors::accent_muted(), Colors::text_primary())
    } else {
        (Colors::surface_input(), Colors::text_muted())
    };
    div()
        .id(gpui::SharedString::from(format!("send-chip-{}", send.id)))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(3.0))
        .mx(px(2.0))
        .px(px(4.0))
        .h(px(18.0))
        .rounded_sm()
        .bg(bg)
        .text_size(px(9.0))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(text)
        .child(div().truncate().flex_1().child(format!("→ {target_name}")))
        .child(
            div()
                .id(gpui::SharedString::from(format!("send-remove-{}", send.id)))
                .text_size(px(10.0))
                .text_color(Colors::text_faint())
                .px(px(2.0))
                .child("×")
                .on_mouse_down(gpui::MouseButton::Left, move |_e, w, cx| {
                    on_remove(&remove_pair, w, cx);
                })
                .occlude(),
        )
}

fn add_send_button(track_id: &str, callbacks: &MixerCallbacks) -> impl IntoElement {
    let track_id_owned = track_id.to_string();
    let on_add = callbacks.on_add_send.clone();
    div()
        .id(gpui::SharedString::from(format!(
            "send-add-{}",
            track_id_owned
        )))
        .flex()
        .items_center()
        .justify_center()
        .mx(px(2.0))
        .px(px(4.0))
        .h(px(18.0))
        .rounded_sm()
        .border(px(1.0))
        .border_dashed()
        .border_color(Colors::slot_border())
        .text_size(px(10.0))
        .text_color(Colors::slot_empty_text())
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|s| s.bg(Colors::surface_control_hover()))
        .child("+")
        .on_mouse_down(gpui::MouseButton::Left, move |_e, w, cx| {
            on_add(&track_id_owned, w, cx);
        })
        .occlude()
}

fn sends_section(
    track: &TrackState,
    all_tracks: &[TrackState],
    callbacks: &MixerCallbacks,
) -> impl IntoElement {
    // Routing tracks (bus/return) don't themselves carry an aux-send rack in
    // this slice — they are send *targets*. Show an empty placeholder.
    let is_routing = track.track_type.is_routing();
    let mut chips = div().flex().flex_col().gap(px(2.0)).px(px(2.0));
    if is_routing {
        chips = chips.child(empty_slot());
    } else {
        for send in &track.sends {
            // Resolve the live target name (handles renames) with the stored
            // label as a fallback.
            let target_name = all_tracks
                .iter()
                .find(|t| t.id == send.target_track_id)
                .map(|t| t.name.clone())
                .unwrap_or_else(|| send.target_name.clone());
            chips = chips.child(send_chip(&track.id, send, &target_name, callbacks));
        }
        chips = chips.child(add_send_button(&track.id, callbacks));
    }

    div()
        .flex()
        .flex_col()
        .h(px(SEC_SENDS_H))
        .border_b(px(1.0))
        .border_color(Colors::divider())
        .child(section_header("SENDS", track.color, None))
        .child(chips)
}

fn pan_section(
    track: &TrackState,
    callbacks: &MixerCallbacks,
    _is_selected: bool,
) -> impl IntoElement {
    let track_id = track.id.clone();
    let pan_cb = callbacks.on_pan_change.clone();
    let on_pan_change = move |new_pan: &f32, w: &mut gpui::Window, cx: &mut gpui::App| {
        pan_cb(&(track_id.clone(), *new_pan), w, cx);
    };

    // Match web MixerPanel pan row: knob only, then a tight L/R legend (no caption
    // under the disk — center is shown by the bipolar tick + arc).
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(2.0))
        .h(px(SEC_PAN_H))
        .py(px(5.0))
        .border_b(px(1.0))
        .border_color(Colors::divider())
        .child(knob_bipolar(
            format!("mix-pan-{}", track.id),
            track.pan,
            -1.0,
            1.0,
            track.color,
            None,
            0.0,
            on_pan_change,
        ))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .w_full()
                .px(px(8.0))
                .child(
                    div()
                        .text_size(px(7.0))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(Colors::text_faint())
                        .child("L"),
                )
                .child(
                    div()
                        .text_size(px(7.0))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(Colors::text_faint())
                        .child("R"),
                ),
        )
}

fn fader_area(
    track: &TrackState,
    callbacks: &MixerCallbacks,
    is_selected: bool,
) -> impl IntoElement {
    let db_str = volume::format_db(track.volume);
    let track_id = track.id.clone();
    let vol_cb = callbacks.on_volume_change.clone();
    let on_vol_change = move |new_norm: &f32, w: &mut gpui::Window, cx: &mut gpui::App| {
        vol_cb(&(track_id.clone(), *new_norm), w, cx);
    };

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .items_center()
        .w_full()
        .px(px(4.0))
        .pt(px(5.0))
        .pb(px(6.0))
        .gap(px(5.0))
        .child(db_value_pill(db_str, is_selected))
        .child(
            div()
                .flex()
                .flex_row()
                .gap(px(2.0))
                .flex_1()
                .min_h_0()
                .w_full()
                .justify_center()
                .child(db_scale_column())
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .h_full()
                        .items_center()
                        .justify_center()
                        .child(render_fader(
                            format!("mix-fader-{}", track.id),
                            track.volume,
                            track.color,
                            on_vol_change,
                        )),
                )
                .child(meter_surface(
                    track.meter_level_l,
                    track.meter_level_r,
                    track.meter_peak_hold_l,
                    track.meter_peak_hold_r,
                    track.meter_clip,
                )),
        )
}

fn strip_footer(name: &str) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_center()
        .h(px(SEC_FOOTER_H))
        .px(px(4.0))
        .border_t(px(1.0))
        .border_color(Colors::divider())
        .bg(Colors::surface_panel_alt())
        .child(
            div()
                .w_full()
                .min_w(px(0.0))
                .truncate()
                .text_size(px(10.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(Colors::text_secondary())
                .child(name.to_string()),
        )
}

// ─── Channel strip ──────────────────────────────────────────────────────────

fn channel_strip(
    track: &TrackState,
    all_tracks: &[TrackState],
    index: usize,
    is_selected: bool,
    callbacks: &MixerCallbacks,
) -> impl IntoElement {
    let id_num = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        track.id.hash(&mut hasher);
        hasher.finish() as usize
    };

    let strip_bg = if is_selected {
        Colors::mixer_strip_selected_bg()
    } else if index % 2 == 0 {
        Colors::mixer_strip_bg()
    } else {
        Colors::mixer_strip_bg_alt()
    };
    let border_col = if is_selected {
        Colors::strip_border()
    } else {
        Colors::strip_border_subtle()
    };

    let select_id = track.id.clone();
    let select_cb = callbacks.on_select_track.clone();
    let on_select_strip =
        move |_: &gpui::MouseDownEvent, w: &mut gpui::Window, cx: &mut gpui::App| {
            select_cb(&select_id, w, cx);
        };
    let context_id = track.id.clone();
    let on_context = callbacks.on_context_menu.clone();

    div()
        .flex()
        .flex_col()
        .flex_none()
        .w(px(STRIP_WIDTH))
        .min_h(px(STRIP_MIN_HEIGHT))
        .h_full()
        .bg(strip_bg)
        .border_r(px(1.0))
        .border_color(border_col)
        .id(("mix-strip", id_num))
        .on_mouse_down(gpui::MouseButton::Left, on_select_strip)
        .when_some(on_context, |this, cb| {
            this.on_mouse_down(
                gpui::MouseButton::Right,
                move |event: &gpui::MouseDownEvent, window, cx| {
                    let x: f32 = event.position.x.into();
                    let y: f32 = event.position.y.into();
                    cb(&(context_id.clone(), x, y), window, cx);
                },
            )
        })
        // Top accent line
        .child(div().w_full().h(px(2.0)).bg(track.color))
        .child(strip_header(track, index))
        .child(inserts_section(track, index, callbacks))
        .child(sends_section(track, all_tracks, callbacks))
        .child(pan_section(track, callbacks, is_selected))
        .child(fader_area(track, callbacks, is_selected))
        .child(button_row(track, callbacks, id_num))
        .child(strip_footer(&track.name))
}

// ─── Master block ───────────────────────────────────────────────────────────

fn master_strip(
    accent: gpui::Rgba,
    master: &MasterBusState,
    on_master_vol_change: std::sync::Arc<dyn Fn(&f32, &mut gpui::Window, &mut gpui::App) + 'static>,
) -> impl IntoElement {
    let db_str = volume::format_db(master.volume);
    let on_change = move |v: &f32, w: &mut gpui::Window, cx: &mut gpui::App| {
        on_master_vol_change(v, w, cx);
    };

    div()
        .flex()
        .flex_col()
        .flex_none()
        .w(px(STRIP_WIDTH))
        .min_h(px(STRIP_MIN_HEIGHT))
        .h_full()
        .bg(Colors::master_strip_bg())
        .border_l(px(1.0))
        .border_color(Colors::master_strip_border())
        .child(div().w_full().h(px(2.0)).bg(accent))
        // Header
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(4.0))
                .h(px(SEC_HEADER_H))
                .px(px(5.0))
                .border_b(px(1.0))
                .border_color(Colors::divider())
                .child(div().w(px(2.0)).h(px(20.0)).rounded_full().bg(accent))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(px(10.0))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(Colors::text_primary())
                                .child("Master"),
                        )
                        .child(
                            div()
                                .text_size(px(7.5))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(Colors::text_faint())
                                .child("MST·BUS"),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .h(px(SEC_INSERTS_H))
                .border_b(px(1.0))
                .border_color(Colors::divider())
                .child(section_header("INSERTS", accent, None))
                .child(empty_slot()),
        )
        // Master skips sends, but we keep a same-sized spacer so its rows
        // line up with the channel strips.
        .child(
            div()
                .h(px(SEC_SENDS_H))
                .border_b(px(1.0))
                .border_color(Colors::divider()),
        )
        // Master skips pan; show the level pill in this row instead so the
        // overall vertical rhythm matches a normal strip.
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(4.0))
                .h(px(SEC_PAN_H))
                .border_b(px(1.0))
                .border_color(Colors::divider())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .min_w(px(46.0))
                        .px(px(6.0))
                        .h(px(14.0))
                        .rounded_sm()
                        .bg(Colors::slot_bg())
                        .border(px(1.0))
                        .border_color(Colors::with_alpha(accent, 0.55)) // Approved: dynamic accent border highlight
                        .text_size(px(9.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(Colors::text_secondary())
                        .child("STEREO"),
                )
                .child(
                    div()
                        .text_size(px(7.5))
                        .text_color(Colors::text_faint())
                        .child("OUT 1-2"),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .items_center()
                .w_full()
                .px(px(4.0))
                .pt(px(5.0))
                .pb(px(6.0))
                .gap(px(5.0))
                .child(db_value_pill(db_str.clone(), true))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(2.0))
                        .flex_1()
                        .min_h_0()
                        .w_full()
                        .justify_center()
                        .child(db_scale_column())
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .h_full()
                                .items_center()
                                .justify_center()
                                .child(render_fader(
                                    "mix-fader-master",
                                    master.volume,
                                    accent,
                                    on_change,
                                )),
                        )
                        .child(meter_surface(
                            master.meter_level_l,
                            master.meter_level_r,
                            master.meter_peak_hold_l,
                            master.meter_peak_hold_r,
                            master.meter_clip,
                        )),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .h(px(SEC_BUTTONS_H))
                .px(px(4.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .h(px(16.0))
                        .px(px(6.0))
                        .rounded_sm()
                        .bg(Colors::slot_bg())
                        .border(px(1.0))
                        .border_color(Colors::slot_border())
                        .text_size(px(8.5))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(Colors::text_muted())
                        .child("OUT 1·2"),
                ),
        )
        .child(strip_footer("Master"))
}

// ─── Public: Mixer Panel ─────────────────────────────────────────────────────

/// Strip columns above/below the visible viewport that are kept rendered to
/// prevent pop-in during horizontal mixer scrolling.
const MIXER_OVERSCAN: usize = 1;

pub fn mixer_panel(
    tracks: &[TrackState],
    master: &MasterBusState,
    selected_track_id: Option<&str>,
    callbacks: MixerCallbacks,
    // Current horizontal scroll offset in pixels.
    scroll_x: f32,
    // Width of the scrollable channel area in pixels (for computing visibility).
    viewport_width: f32,
    // Called with the new clamped scroll_x whenever the user scrolls the mixer.
    on_scroll: std::sync::Arc<dyn Fn(f32, &mut gpui::Window, &mut gpui::App) + 'static>,
) -> impl IntoElement {
    let _s = crate::perf::PerfScope::enter("MixerPanel");
    let track_count = tracks.len();
    crate::perf::count("mixer_strips", track_count as u64);

    let accent = Colors::accent_primary();
    let on_master = callbacks.on_master_volume_change.clone();

    // ── Virtual strip window ────────────────────────────────────────────────
    // Only strips whose screen-space X overlaps [0, viewport_width] are built.
    // The rest are represented by opaque spacer divs so the total scroll width
    // stays correct even though individual strip elements don't exist.
    let total_content_w = track_count as f32 * STRIP_WIDTH;
    let max_scroll_x = (total_content_w - viewport_width).max(0.0);
    let scroll_x = scroll_x.clamp(0.0, max_scroll_x.max(0.0));

    let first_visible = (scroll_x / STRIP_WIDTH).floor() as usize;
    let visible_start = first_visible.saturating_sub(MIXER_OVERSCAN);
    let last_visible = ((scroll_x + viewport_width) / STRIP_WIDTH).ceil() as usize;
    let visible_end = (last_visible + MIXER_OVERSCAN).min(track_count);

    let left_spacer_w = visible_start as f32 * STRIP_WIDTH;
    let right_spacer_w = track_count.saturating_sub(visible_end) as f32 * STRIP_WIDTH;

    crate::perf::count(
        "visible_mixer_strips",
        visible_end.saturating_sub(visible_start) as u64,
    );

    let visible_strips: Vec<gpui::AnyElement> = tracks[visible_start..visible_end]
        .iter()
        .enumerate()
        .map(|(rel_i, t)| {
            let abs_i = visible_start + rel_i;
            let is_sel = selected_track_id == Some(t.id.as_str());
            channel_strip(t, tracks, abs_i, is_sel, &callbacks).into_any_element()
        })
        .collect();

    // Scroll-wheel handler: translate wheel delta into scroll_x updates.
    let on_scroll_wheel = {
        let on_scroll = on_scroll.clone();
        move |event: &gpui::ScrollWheelEvent, window: &mut gpui::Window, cx: &mut gpui::App| {
            let (dx, dy) = match &event.delta {
                gpui::ScrollDelta::Pixels(p) => (f32::from(p.x), f32::from(p.y)),
                gpui::ScrollDelta::Lines(l) => (l.x * STRIP_WIDTH, l.y * STRIP_WIDTH * 0.5),
            };
            // Prefer horizontal delta; fall back to vertical (mouse-wheel-only users).
            let delta = if dx.abs() >= dy.abs() { dx } else { dy };
            let new_x = (scroll_x + delta).clamp(0.0, max_scroll_x.max(0.0));
            on_scroll(new_x, window, cx);
        }
    };

    div()
        .flex()
        .flex_col()
        .size_full()
        .bg(Colors::mixer_bg())
        .child(mixer_sub_header(track_count))
        // Content row: scrollable channels (flex_1) + master block (fixed).
        .child(
            div()
                .flex()
                .flex_row()
                .flex_1()
                .min_h_0()
                .child(
                    // Channel scroll area — manually virtualized in the x axis.
                    // The outer div clips overflow; the inner absolute div is
                    // shifted left by scroll_x so only visible strips appear.
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .h_full()
                        .relative()
                        .overflow_hidden()
                        .on_scroll_wheel(on_scroll_wheel)
                        .child(
                            div()
                                .absolute()
                                .left(px(-scroll_x))
                                .top_0()
                                .bottom_0()
                                .flex()
                                .flex_row()
                                .h_full()
                                .min_h(px(STRIP_MIN_HEIGHT))
                                // Left spacer for off-screen strips.
                                .when(left_spacer_w > 0.0, |d| {
                                    d.child(
                                        div()
                                            .w(px(left_spacer_w))
                                            .h_full()
                                            .flex_none()
                                            .bg(Colors::mixer_bg()),
                                    )
                                })
                                .children(visible_strips)
                                // Right spacer for off-screen strips.
                                .when(right_spacer_w > 0.0, |d| {
                                    d.child(
                                        div()
                                            .w(px(right_spacer_w))
                                            .h_full()
                                            .flex_none()
                                            .bg(Colors::mixer_bg()),
                                    )
                                }),
                        ),
                )
                // Gutter separating channels from the master block.
                .child(div().w(px(1.0)).h_full().bg(Colors::border_default()))
                // Pinned master block
                .child(master_strip(accent, master, on_master)),
        )
}
