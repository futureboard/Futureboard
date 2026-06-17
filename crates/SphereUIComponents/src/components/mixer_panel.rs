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
//! * Strip internals are a vertical stack with explicit shared insert/send
//!   viewport heights; only the lower pan/fader area grows to fill remaining
//!   height.

use gpui::prelude::FluentBuilder;
use gpui::{
    div, px, svg, App, AppContext, ClickEvent, DragMoveEvent, Empty, InteractiveElement,
    IntoElement, MouseDownEvent, ParentElement, Render, StatefulInteractiveElement, Styled, Window,
};

use crate::assets;
use crate::components::fader::{db_scale_column, db_value_pill, fader as render_fader};
use crate::components::knob::knob_bipolar;
use crate::components::panel::FxSlotDrag;
use crate::components::reorder::{drag_handle, drop_over_highlight};
use crate::components::timeline::timeline_state::{
    volume, InsertLoadStatus, InsertSlotState, MasterBusState, SendSlotState, TrackState,
    TrackType, MASTER_TRACK_ID,
};
use crate::components::timeline::vu_meter::meter_surface;
use crate::theme::Colors;

// ── Section dimensions ─────────────────────────────────────────────────────
const STRIP_WIDTH: f32 = 88.0;
/// Minimum height for a channel strip. Below this the mixer should scroll/clip
/// as a whole rather than compressing the pan/fader controls into unusability.
const STRIP_MIN_HEIGHT: f32 = 320.0;

const SEC_HEADER_H: f32 = 40.0;
const SEC_SECTION_HEADER_H: f32 = 20.0;
const SEC_PAN_H: f32 = 60.0;
const SEC_BUTTONS_H: f32 = 24.0;
const SEC_FOOTER_H: f32 = 22.0;
const SEC_FADER_MIN_H: f32 = 66.0;
const LOWER_CONTROL_MIN_H: f32 = SEC_PAN_H + SEC_FADER_MIN_H + SEC_BUTTONS_H;

// ── Vertical mixer section resizing ─────────────────────────────────────────
// Inserts and sends each own a fixed-height clipped viewport with their own
// vertical scrolling. Heights are shared across all strips so rows stay aligned
// across the mixer. Splitter actions are routed to `StudioLayout`, which owns
// the shared values and mirrors them into the detached mixer window snapshot.
/// Visual + hitbox height of the splitter handle.
const SEC_SPLITTER_H: f32 = 6.0;
const SECTION_VIEWPORT_MIN_H: f32 = 42.0;
const SECTION_VIEWPORT_MAX_H: f32 = 180.0;
pub const MIXER_INSERT_SECTION_DEFAULT_PX: f32 = 72.0;
pub const MIXER_SEND_SECTION_DEFAULT_PX: f32 = 54.0;

/// Clamp one insert/send section height into the static supported range.
pub fn clamp_mixer_section_height_px(value: f32) -> f32 {
    value.clamp(SECTION_VIEWPORT_MIN_H, SECTION_VIEWPORT_MAX_H)
}

/// Clamp both section heights while preserving a usable lower pan/fader area
/// for the current strip allocation.
pub fn clamp_mixer_section_heights_for_strip(
    insert_px: f32,
    send_px: f32,
    strip_available_px: f32,
) -> (f32, f32) {
    let mut insert_px = clamp_mixer_section_height_px(insert_px);
    let mut send_px = clamp_mixer_section_height_px(send_px);
    let fixed_without_sections =
        2.0 + SEC_HEADER_H + (SEC_SPLITTER_H * 2.0) + LOWER_CONTROL_MIN_H + SEC_FOOTER_H;
    let max_total = (strip_available_px - fixed_without_sections).max(SECTION_VIEWPORT_MIN_H * 2.0);

    let total = insert_px + send_px;
    if total > max_total {
        let overflow = total - max_total;
        let shrinkable_insert = insert_px - SECTION_VIEWPORT_MIN_H;
        let shrinkable_send = send_px - SECTION_VIEWPORT_MIN_H;
        let shrinkable_total = shrinkable_insert + shrinkable_send;
        if shrinkable_total > 0.0 {
            insert_px -= overflow * (shrinkable_insert / shrinkable_total);
            send_px -= overflow * (shrinkable_send / shrinkable_total);
        }
        insert_px = clamp_mixer_section_height_px(insert_px);
        send_px = clamp_mixer_section_height_px(send_px);
    }

    (insert_px, send_px)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MixerSplitTarget {
    InsertSend,
    SendFader,
}

/// Splitter drag/reset intents emitted by the channel-strip splitter handle.
/// Pointer Y values are window-space (matches `MouseDownEvent::position.y`).
#[derive(Clone, Copy, Debug)]
pub enum MixerSplitAction {
    /// Pointer pressed on the splitter — record the drag anchor.
    ResizeStart(MixerSplitTarget, f32),
    /// Pointer moved while dragging — recompute the shared rack height.
    ResizeMove(f32),
    /// Pointer released — commit the drag.
    ResizeEnd,
    /// Double-click — reset the targeted section to its default height.
    Reset(MixerSplitTarget),
}

/// Shared split layout passed into the mixer. Insert/send heights are already
/// clamped by the owner; `on_action` routes splitter intents back to the owner
/// so all strips resize together.
#[derive(Clone)]
pub struct MixerSplit {
    pub insert_px: f32,
    pub send_px: f32,
    pub active_target: Option<MixerSplitTarget>,
    pub on_action: std::sync::Arc<dyn Fn(MixerSplitAction, &mut Window, &mut App) + 'static>,
}

impl MixerSplit {
    /// Inert split for fallback UI (no live owner to route drags to).
    pub fn inert() -> Self {
        Self {
            insert_px: MIXER_INSERT_SECTION_DEFAULT_PX,
            send_px: MIXER_SEND_SECTION_DEFAULT_PX,
            active_target: None,
            on_action: std::sync::Arc::new(|_, _, _| {}),
        }
    }
}

/// Zero-sized GPUI drag payload for the mixer splitter handle. Mirrors the
/// bottom-panel resize pattern: `on_drag` registers it, `on_drag_move` on the
/// mixer root recomputes height while the pointer is captured.
#[derive(Clone, Copy, Debug, Default)]
pub struct MixerSplitDrag;

impl Render for MixerSplitDrag {
    fn render(&mut self, _w: &mut Window, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        Empty
    }
}

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
    /// Drag-reorder commit for an insert slot. `(track_id, dragged_insert_id,
    /// insertion_index)` where `insertion_index` is the gap (0..=len) the
    /// dragged slot moves into. Identity is the stable `plugin_instance_id`,
    /// never the visual index. One completed drag = one undo entry (mirrors the
    /// Inspector's `on_reorder_insert` / `reorder_insert_cb`).
    pub on_reorder_insert: std::sync::Arc<
        dyn Fn(&(String, String, usize), &mut gpui::Window, &mut gpui::App) + 'static,
    >,
    /// User clicked the slot chip — Phase 4 will open the native plugin
    /// editor; Phase 1 logs the request.
    pub on_open_insert_editor: std::sync::Arc<
        dyn Fn(&(String, usize, String), &mut gpui::Window, &mut gpui::App) + 'static,
    >,
    /// Open the send target picker for `(track_id, x, y)`.
    pub on_add_send:
        std::sync::Arc<dyn Fn(&(String, f32, f32), &mut gpui::Window, &mut gpui::App) + 'static>,
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
    let noop_insert_reorder =
        Arc::new(|_: &(String, String, usize), _: &mut Window, _: &mut App| {});
    let noop_add_send = Arc::new(|_: &(String, f32, f32), _: &mut Window, _: &mut App| {});
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
        on_reorder_insert: noop_insert_reorder,
        on_open_insert_editor: noop_insert_open.clone(),
        on_add_send: noop_add_send,
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
        .flex_none()
        .gap(px(3.0))
        .h(px(SEC_SECTION_HEADER_H))
        .px(px(5.0))
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
        .flex_none()
        .items_center()
        .justify_center()
        .mx(px(4.0))
        .py(px(2.0))
        .rounded_sm()
        .text_size(px(8.0))
        .text_color(Colors::text_faint())
        .opacity(0.62)
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
            track.input_monitor.is_active(track.armed),
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

    let (bg, text) = match &slot.load_status {
        InsertLoadStatus::Ready if !bypassed => (Colors::accent_muted(), Colors::text_primary()),
        InsertLoadStatus::Ready => (Colors::surface_input(), Colors::text_muted()),
        InsertLoadStatus::Loading => (Colors::surface_input(), Colors::text_muted()),
        InsertLoadStatus::Missing(_) | InsertLoadStatus::Failed(_) => (
            Colors::with_alpha(Colors::status_error(), 0.16),
            Colors::status_error(),
        ),
        InsertLoadStatus::Disabled => (Colors::surface_input(), Colors::text_faint()),
        InsertLoadStatus::Empty => (Colors::surface_input(), Colors::slot_empty_text()),
    };

    let id_owned = slot_id.clone();
    let bypass_pair = (track_id_owned.clone(), slot_id.clone());
    let remove_pair = (track_id_owned.clone(), slot_id.clone());

    // Drag source: only the grip handle starts a reorder, so the chip's
    // open-editor click and the ▲/▼/×/bypass controls keep their own hit-
    // testing. The payload carries the stable plugin_instance_id, so reorder
    // identity follows the instance — never the visual index (model only
    // reorders existing slots; bypass/preset/editor/automation state come
    // along). `.occlude()` stops a press on the grip from also opening the
    // editor. ElementId includes the track id because the mixer renders every
    // track's strips at once (the Inspector shows one track).
    let drag_payload = FxSlotDrag {
        track_id: track_id_owned.clone(),
        insert_id: slot_id.clone(),
        display_name: slot.display_name.clone(),
    };
    let handle = drag_handle()
        .id(gpui::SharedString::from(format!(
            "mixer-fx-drag-{track_id}-{slot_id}"
        )))
        .occlude()
        .on_drag(drag_payload, |drag, _offset, _window, cx| {
            cx.new(|_| drag.clone())
        });

    // Drop target: dropping a compatible drag onto this chip moves it into the
    // gap *above* this slot (`insertion_index == insert_index`, the slot's full
    // insert-chain index). `can_drop` restricts drops to the same track and
    // `drag_over` paints the shared accent drop-position line.
    let drop_track = track_id_owned.clone();
    let can_drop_track = track_id_owned.clone();
    let reorder = callbacks.on_reorder_insert.clone();
    let drop_gap = insert_index;

    let open_target = (track_id_owned, insert_index, slot_id);

    div()
        .id(gpui::SharedString::from(format!(
            "insert-chip-{}",
            id_owned
        )))
        .can_drop(move |dragged, _window, _cx| {
            dragged
                .downcast_ref::<FxSlotDrag>()
                .is_some_and(|d| d.track_id == can_drop_track)
        })
        .drag_over::<FxSlotDrag>(|style, _drag, _window, _cx| drop_over_highlight(style))
        .on_drop::<FxSlotDrag>(move |drag, window, cx| {
            if drag.track_id == drop_track {
                reorder(
                    &(drop_track.clone(), drag.insert_id.clone(), drop_gap),
                    window,
                    cx,
                );
            }
        })
        .flex()
        .flex_none()
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
        // Grip drag handle (leftmost) — the only reorder drag source.
        .child(handle)
        .child(div().flex_1().min_w(px(0.0)).truncate().child(display))
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

/// Trailing drop zone rendered below the last insert chip so a dragged slot can
/// land at the very end of the chain (`gap == inserts.len()`); the per-chip drop
/// targets only cover the gaps *above* each existing slot. Same-track guarded and
/// shows the shared accent drop-position line while a compatible drag hovers.
fn insert_drop_end(track_id: &str, gap: usize, callbacks: &MixerCallbacks) -> impl IntoElement {
    let track_id_owned = track_id.to_string();
    let can_drop_track = track_id_owned.clone();
    let reorder = callbacks.on_reorder_insert.clone();
    div()
        .id(gpui::SharedString::from(format!(
            "mixer-fx-drop-end-{track_id_owned}"
        )))
        .flex_none()
        .h(px(6.0))
        .mx(px(2.0))
        .can_drop(move |dragged, _window, _cx| {
            dragged
                .downcast_ref::<FxSlotDrag>()
                .is_some_and(|d| d.track_id == can_drop_track)
        })
        .drag_over::<FxSlotDrag>(|style, _drag, _window, _cx| drop_over_highlight(style))
        .on_drop::<FxSlotDrag>(move |drag, window, cx| {
            if drag.track_id == track_id_owned {
                reorder(
                    &(track_id_owned.clone(), drag.insert_id.clone(), gap),
                    window,
                    cx,
                );
            }
        })
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
        .flex_none()
        .items_center()
        .justify_center()
        .gap(px(3.0))
        .mx(px(2.0))
        .px(px(4.0))
        .h(px(18.0))
        .rounded_sm()
        .border(px(1.0))
        .border_dashed()
        .border_color(Colors::with_alpha(Colors::slot_border(), 0.68))
        .text_size(px(9.0))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(Colors::text_faint())
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|s| {
            s.bg(Colors::surface_control_hover())
                .border_color(Colors::slot_border())
                .text_color(Colors::text_muted())
        })
        .child(
            svg()
                .path(assets::ICON_PLUS_PATH)
                .w(px(8.0))
                .h(px(8.0))
                .text_color(Colors::text_faint()),
        )
        .child("Insert")
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
    height_px: f32,
) -> impl IntoElement {
    let effect_start = if track.track_type == TrackType::Instrument {
        1
    } else {
        0
    };
    let used = track.inserts.len();
    let at_max = used >= MAX_INSERT_SLOTS;

    let mut chips = div().flex().flex_col().flex_none().gap(px(2.0)).px(px(2.0));
    let effects = track.effect_inserts();
    for (offset, slot) in effects.iter().enumerate() {
        let insert_index = effect_start + offset;
        chips = chips.child(insert_chip(&track.id, insert_index, slot, callbacks));
    }
    // Drop-at-end zone below the last chip (gap == full insert-chain length, so
    // the instrument slot at index 0 is counted). Only meaningful once a slot
    // exists to drag.
    if !effects.is_empty() {
        chips = chips.child(insert_drop_end(&track.id, track.inserts.len(), callbacks));
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
        .flex_none()
        .h(px(height_px))
        .overflow_hidden()
        .border_b(px(1.0))
        .border_color(Colors::divider())
        .child(section_header("INSERTS", track.color, header_plus))
        .child(
            div()
                .id(gpui::SharedString::from(format!(
                    "insert-slot-scroll-{}",
                    track.id
                )))
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(chips),
        )
}

fn master_inserts_section(
    accent: gpui::Rgba,
    master: &MasterBusState,
    callbacks: &MixerCallbacks,
    height_px: f32,
) -> impl IntoElement {
    let used = master.inserts.len();
    let at_max = used >= MAX_INSERT_SLOTS;

    let mut chips = div().flex().flex_col().flex_none().gap(px(2.0)).px(px(2.0));
    for (insert_index, slot) in master.inserts.iter().enumerate() {
        chips = chips.child(insert_chip(MASTER_TRACK_ID, insert_index, slot, callbacks));
    }
    if !master.inserts.is_empty() {
        chips = chips.child(insert_drop_end(
            MASTER_TRACK_ID,
            master.inserts.len(),
            callbacks,
        ));
    }
    if !at_max {
        chips = chips.child(add_insert_button(MASTER_TRACK_ID, used, callbacks));
    }

    let header_plus = if at_max {
        None
    } else {
        let on_add = callbacks.on_add_insert.clone();
        Some(HeaderPlus {
            id: gpui::SharedString::from("insert-header-add-master"),
            on_click: std::sync::Arc::new(move |w, cx| {
                eprintln!("[mixer] INSERTS header + clicked track=master slot={used}");
                on_add(&MASTER_TRACK_ID.to_string(), w, cx);
            }),
        })
    };

    div()
        .flex()
        .flex_col()
        .flex_none()
        .h(px(height_px))
        .overflow_hidden()
        .border_b(px(1.0))
        .border_color(Colors::divider())
        .child(section_header("INSERTS", accent, header_plus))
        .child(
            div()
                .id("insert-slot-scroll-master")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(chips),
        )
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
        .flex_none()
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
        .flex_none()
        .items_center()
        .justify_center()
        .gap(px(3.0))
        .mx(px(2.0))
        .px(px(4.0))
        .h(px(18.0))
        .rounded_sm()
        .border(px(1.0))
        .border_dashed()
        .border_color(Colors::with_alpha(Colors::slot_border(), 0.68))
        .text_size(px(9.0))
        .text_color(Colors::text_faint())
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|s| {
            s.bg(Colors::surface_control_hover())
                .border_color(Colors::slot_border())
                .text_color(Colors::text_muted())
        })
        .child(
            svg()
                .path(assets::ICON_PLUS_PATH)
                .w(px(8.0))
                .h(px(8.0))
                .text_color(Colors::text_faint()),
        )
        .child("Send")
        .on_mouse_down(gpui::MouseButton::Left, move |event, w, cx| {
            let x: f32 = event.position.x.into();
            let y: f32 = event.position.y.into();
            on_add(&(track_id_owned.clone(), x, y), w, cx);
        })
        .occlude()
}

fn sends_section(
    track: &TrackState,
    all_tracks: &[TrackState],
    callbacks: &MixerCallbacks,
    height_px: f32,
) -> impl IntoElement {
    // Routing tracks (bus/return) don't themselves carry an aux-send rack in
    // this slice — they are send *targets*. Show an empty placeholder.
    let is_routing = track.track_type.is_routing();
    let mut chips = div().flex().flex_col().flex_none().gap(px(2.0)).px(px(2.0));
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
        .flex_none()
        .h(px(height_px))
        .overflow_hidden()
        .border_b(px(1.0))
        .border_color(Colors::divider())
        .child(section_header("SENDS", track.color, None))
        .child(
            div()
                .id(gpui::SharedString::from(format!(
                    "send-slot-scroll-{}",
                    track.id
                )))
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(chips),
        )
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
    let display_vol = track.display_volume();
    let db_str = volume::format_db(display_vol);
    let has_volume_automation = track.has_active_volume_automation();
    let automation_reading = has_volume_automation && track.volume_automation_read;
    let track_id = track.id.clone();
    let vol_cb = callbacks.on_volume_change.clone();
    let on_vol_change = move |new_norm: &f32, w: &mut gpui::Window, cx: &mut gpui::App| {
        vol_cb(&(track_id.clone(), *new_norm), w, cx);
    };

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(SEC_FADER_MIN_H))
        .items_center()
        .w_full()
        .px(px(4.0))
        .pt(px(5.0))
        .pb(px(6.0))
        .gap(px(5.0))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_center()
                .gap(px(3.0))
                .child(db_value_pill(db_str, is_selected || automation_reading))
                .when(has_volume_automation, |this| {
                    this.child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .h(px(14.0))
                            .min_w(px(16.0))
                            .px(px(3.0))
                            .rounded_sm()
                            .bg(if automation_reading {
                                Colors::accent_muted()
                            } else {
                                Colors::slot_bg()
                            })
                            .border(px(1.0))
                            .border_color(if automation_reading {
                                Colors::with_alpha(track.color, 0.58)
                            } else {
                                Colors::slot_border()
                            })
                            .text_size(px(8.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(if automation_reading {
                                Colors::text_primary()
                            } else {
                                Colors::text_faint()
                            })
                            .child("A"),
                    )
                }),
        )
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
                            display_vol,
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

// ─── Vertical split handle ───────────────────────────────────────────────────

/// Compact horizontal splitter between mixer vertical sections. 6px hitbox
/// with a short centered grip line; hover/active use theme tokens (no web-style
/// chunky handle). `id_num` namespaces the GPUI element id per strip so
/// drag/click state never bleeds between strips. The drag uses the same
/// `on_drag` + ancestor `on_drag_move` capture pattern as the bottom panel.
fn vertical_split_handle(
    id_num: usize,
    target: MixerSplitTarget,
    split: &MixerSplit,
) -> impl IntoElement {
    let on_down = split.on_action.clone();
    let on_dbl = split.on_action.clone();
    let is_resizing = split.active_target == Some(target);

    let grip = if is_resizing {
        Colors::accent_primary()
    } else {
        Colors::strip_border()
    };

    let mut handle = div()
        .id((
            match target {
                MixerSplitTarget::InsertSend => "mix-split-insert-send",
                MixerSplitTarget::SendFader => "mix-split-send-fader",
            },
            id_num,
        ))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .w_full()
        .h(px(SEC_SPLITTER_H))
        .border_b(px(1.0))
        .border_color(Colors::divider())
        .cursor(gpui::CursorStyle::ResizeUpDown)
        .child(div().w(px(20.0)).h(px(1.0)).rounded_full().bg(grip))
        .on_mouse_down(gpui::MouseButton::Left, move |e: &MouseDownEvent, w, cx| {
            let y: f32 = e.position.y.into();
            on_down(MixerSplitAction::ResizeStart(target, y), w, cx);
        })
        .on_drag(MixerSplitDrag, |_drag, _offset, _window, cx| {
            cx.new(|_| MixerSplitDrag)
        })
        .on_click(move |e: &ClickEvent, w, cx| {
            if e.click_count() >= 2 {
                on_dbl(MixerSplitAction::Reset(target), w, cx);
            }
        })
        .occlude();

    if is_resizing {
        handle = handle.bg(Colors::accent_soft());
    } else {
        handle = handle.hover(|s| s.bg(Colors::surface_control_hover()));
    }
    handle
}

// ─── Channel strip ──────────────────────────────────────────────────────────

fn channel_strip(
    track: &TrackState,
    all_tracks: &[TrackState],
    index: usize,
    is_selected: bool,
    callbacks: &MixerCallbacks,
    split: &MixerSplit,
    strip_available_px: f32,
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
    let (insert_h, send_h) = clamp_mixer_section_heights_for_strip(
        split.insert_px,
        split.send_px,
        strip_available_px.max(STRIP_MIN_HEIGHT),
    );

    div()
        .flex()
        .flex_col()
        .flex_none()
        .w(px(STRIP_WIDTH))
        .min_h(px(STRIP_MIN_HEIGHT))
        .h_full()
        .overflow_hidden()
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
        .child(inserts_section(track, index, callbacks, insert_h))
        .child(vertical_split_handle(
            id_num,
            MixerSplitTarget::InsertSend,
            split,
        ))
        .child(sends_section(track, all_tracks, callbacks, send_h))
        .child(vertical_split_handle(
            id_num,
            MixerSplitTarget::SendFader,
            split,
        ))
        // ── Lower Control — pan / fader / meter / M·S·R·I. Takes the remaining
        // height; the fader area is the flex_1 child so it absorbs growth and
        // shrinks first when space is tight (pan + buttons stay fixed).
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(LOWER_CONTROL_MIN_H))
                .overflow_hidden()
                .w_full()
                .child(pan_section(track, callbacks, is_selected))
                .child(fader_area(track, callbacks, is_selected))
                .child(button_row(track, callbacks, id_num)),
        )
        .child(strip_footer(&track.name))
}

// ─── Master block ───────────────────────────────────────────────────────────

fn master_strip(
    accent: gpui::Rgba,
    master: &MasterBusState,
    on_master_vol_change: std::sync::Arc<dyn Fn(&f32, &mut gpui::Window, &mut gpui::App) + 'static>,
    callbacks: &MixerCallbacks,
    split: &MixerSplit,
    strip_available_px: f32,
) -> impl IntoElement {
    // Fixed element-id namespace so the master rack scroll/splitter state never
    // collides with a hashed track id.
    let id_num = usize::MAX;
    let db_str = volume::format_db(master.volume);
    let on_change = move |v: &f32, w: &mut gpui::Window, cx: &mut gpui::App| {
        on_master_vol_change(v, w, cx);
    };
    let (insert_h, send_h) = clamp_mixer_section_heights_for_strip(
        split.insert_px,
        split.send_px,
        strip_available_px.max(STRIP_MIN_HEIGHT),
    );

    div()
        .flex()
        .flex_col()
        .flex_none()
        .w(px(STRIP_WIDTH))
        .min_h(px(STRIP_MIN_HEIGHT))
        .h_full()
        .overflow_hidden()
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
        .child(master_inserts_section(accent, master, callbacks, insert_h))
        .child(vertical_split_handle(
            id_num,
            MixerSplitTarget::InsertSend,
            split,
        ))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_none()
                .h(px(send_h))
                .overflow_hidden()
                .border_b(px(1.0))
                .border_color(Colors::divider())
                .child(section_header("SENDS", accent, None))
                .child(
                    div()
                        .id("send-slot-scroll-master")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .child(empty_slot()),
                ),
        )
        .child(vertical_split_handle(
            id_num,
            MixerSplitTarget::SendFader,
            split,
        ))
        // ── Lower Control — STEREO/OUT row, fader cluster, OUT button.
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(LOWER_CONTROL_MIN_H))
                .overflow_hidden()
                .w_full()
                // Master skips pan; show the level pill in this row instead so
                // the overall vertical rhythm matches a normal strip.
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
                        .min_h(px(SEC_FADER_MIN_H))
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
                ),
        )
        .child(strip_footer("Master"))
}

// ─── Public: Mixer Panel ─────────────────────────────────────────────────────

/// Strip columns above/below the visible viewport that are kept rendered to
/// prevent pop-in during horizontal mixer scrolling.
const MIXER_OVERSCAN: usize = 1;

fn mixer_empty_bay(spare_w: f32) -> impl IntoElement {
    let stripe_count = (spare_w / STRIP_WIDTH).ceil().clamp(0.0, 64.0) as usize;
    let stripes: Vec<gpui::AnyElement> = (0..stripe_count)
        .map(|i| {
            div()
                .absolute()
                .left(px(i as f32 * STRIP_WIDTH))
                .top_0()
                .bottom_0()
                .w(px(1.0))
                .bg(Colors::strip_border_subtle())
                .into_any_element()
        })
        .collect();

    div()
        .absolute()
        .top_0()
        .bottom_0()
        .left(px(0.0))
        .w(px(spare_w.max(0.0)))
        .bg(Colors::mixer_bg())
        .children(stripes)
}

pub fn mixer_panel(
    tracks: &[TrackState],
    master: &MasterBusState,
    selected_track_id: Option<&str>,
    callbacks: MixerCallbacks,
    // Current horizontal scroll offset in pixels.
    scroll_x: f32,
    // Width of the scrollable channel area in pixels (for computing visibility).
    viewport_width: f32,
    // Height of this mixer panel in pixels; used to keep the lower strip controls usable.
    viewport_height: f32,
    // Called with the new clamped scroll_x whenever the user scrolls the mixer.
    on_scroll: std::sync::Arc<dyn Fn(f32, &mut gpui::Window, &mut gpui::App) + 'static>,
    // Shared Upper Rack ↔ Lower Control split (height + drag routing).
    split: MixerSplit,
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
    let spare_channel_w = (viewport_width - total_content_w).max(0.0);
    let strip_available_px = (viewport_height - 30.0).max(STRIP_MIN_HEIGHT);

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
            channel_strip(
                t,
                tracks,
                abs_i,
                is_sel,
                &callbacks,
                &split,
                strip_available_px,
            )
            .into_any_element()
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

    // Splitter drag capture: any strip's handle starts the drag (records the
    // anchor via ResizeStart); the move/up events land on this root so the
    // pointer stays captured across the whole mixer surface (bottom-panel
    // resize pattern). ResizeEnd is a cheap no-op unless a drag is live.
    let split_for_move = split.clone();
    let split_for_end = split.clone();

    div()
        .flex()
        .flex_col()
        .size_full()
        .bg(Colors::mixer_bg())
        .on_drag_move::<MixerSplitDrag>(move |event: &DragMoveEvent<MixerSplitDrag>, w, cx| {
            let y: f32 = event.event.position.y.into();
            (split_for_move.on_action)(MixerSplitAction::ResizeMove(y), w, cx);
        })
        .on_mouse_up(gpui::MouseButton::Left, move |_e, w, cx| {
            (split_for_end.on_action)(MixerSplitAction::ResizeEnd, w, cx);
        })
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
                        .when(spare_channel_w > 0.0, |d| {
                            d.child(mixer_empty_bay(spare_channel_w))
                        })
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
                .child(master_strip(
                    accent,
                    master,
                    on_master,
                    &callbacks,
                    &split,
                    strip_available_px,
                )),
        )
}
