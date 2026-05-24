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

use gpui::{
    div, px, rgba, svg, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement,
    Styled,
};

use crate::assets;
use crate::components::fader::{
    db_scale_column, db_value_pill, fader as render_fader, FADER_TRACK_HEIGHT,
};
use crate::components::knob::{format_pan_label, knob_bipolar};
use crate::components::timeline::timeline_state::{volume, MasterBusState, TrackState, TrackType};
use crate::components::timeline::vu_meter::vu_meter_vertical;
use crate::theme::Colors;

// ── Section dimensions ─────────────────────────────────────────────────────
const STRIP_WIDTH: f32 = 88.0;
/// Minimum height needed to render every section without overlap.
/// Below this the channel scroll area shows a vertical scrollbar instead of
/// clipping. Sections sum: header 46 + inserts 56 + sends 56 + pan 80 +
/// fader_area (pill 18 + rail 130 + pad 14) + buttons 46 + footer 26.
const STRIP_MIN_HEIGHT: f32 = 320.0;

const SEC_HEADER_H: f32 = 46.0;
const SEC_INSERTS_H: f32 = 56.0;
const SEC_SENDS_H: f32 = 56.0;
const SEC_PAN_H: f32 = 80.0;
const SEC_BUTTONS_H: f32 = 46.0;
const SEC_FOOTER_H: f32 = 26.0;

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
        .border_color(rgba(0xFFFFFF0F_u32))
        .child(
            svg()
                .path(assets::ICON_SLIDERS_HORIZONTAL_PATH)
                .w(px(14.0))
                .h(px(14.0))
                .text_color(rgba(0xFFFFFF47_u32)),
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
                .bg(rgba(0xFFFFFF08_u32))
                .border(px(1.0))
                .border_color(rgba(0xFFFFFF12_u32))
                .text_size(px(9.0))
                .text_color(rgba(0xFFFFFF59_u32))
                .child(format!("{} ch", track_count)),
        )
}

// ─── Section header ──────────────────────────────────────────────────────────

fn section_header(label: &'static str, accent: gpui::Rgba) -> impl IntoElement {
    let icon_path = match label {
        "INSERTS" => Some(assets::ICON_PLUG_PATH),
        "SENDS" => Some(assets::ICON_ROUTE_PATH),
        _ => None,
    };
    let mut soft_accent = accent;
    soft_accent.a = 0.55;

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
                        .text_color(rgba(0xDCE8F066_u32))
                }))
                .child(
                    div()
                        .text_size(px(7.5))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgba(0xDCE8F066_u32))
                        .child(label),
                ),
        )
        .child(
            div()
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
                        .text_color(rgba(0xFFFFFF38_u32)),
                ),
        )
}

fn insert_row(name: &str, accent: gpui::Rgba) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(4.0))
        .mx(px(3.0))
        .border_l(px(2.0))
        .border_color(accent)
        .px(px(4.0))
        .py(px(2.0))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_size(px(9.0))
                .text_color(rgba(0xFFFFFFB8_u32))
                .child(name.to_string()),
        )
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
        .border_color(rgba(0xFFFFFF0D_u32))
        .text_size(px(8.0))
        .text_color(rgba(0xFFFFFF38_u32))
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
        .h(px(18.0))
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
            .bg(rgba(0xFFFFFF0A_u32))
            .border(px(1.0))
            .border_color(rgba(0xFFFFFF17_u32))
            .text_color(rgba(0xDCE8F085_u32))
            .hover(|s| s.bg(rgba(0xFFFFFF1A_u32)));
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
        .py(px(3.0))
        .h(px(SEC_BUTTONS_H))
        .items_center()
        .border_t(px(1.0))
        .border_color(rgba(0xFFFFFF0B_u32))
        .child(msri_button(
            ("mix-m-btn", id_num).into(),
            "M",
            track.muted,
            gpui::rgb(0xF3C969),
            gpui::rgb(0x101216),
            on_mute,
        ))
        .child(msri_button(
            ("mix-s-btn", id_num).into(),
            "S",
            track.solo,
            gpui::rgb(0x7BD88F),
            gpui::rgb(0x101216),
            on_solo,
        ))
        .child(msri_button(
            ("mix-r-btn", id_num).into(),
            "R",
            track.armed,
            gpui::rgb(0xF06A61),
            gpui::rgb(0x101216),
            on_arm,
        ))
        .child(msri_button(
            ("mix-i-btn", id_num).into(),
            "I",
            track.input_monitor,
            Colors::accent_primary(),
            gpui::rgb(0x101216),
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
        .border_color(rgba(0xFFFFFF0B_u32))
        .child(div().w(px(2.0)).h(px(24.0)).rounded_full().bg(track.color))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.0))
                .child(
                    div()
                        .text_size(px(10.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgba(0xFFFFFFCC_u32))
                        .child(track.name.clone()),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(3.0))
                        .mt(px(1.0))
                        .child(
                            div()
                                .text_size(px(7.5))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(rgba(0xFFFFFF47_u32))
                                .child(type_label),
                        )
                        .child(
                            div()
                                .text_size(px(7.5))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(rgba(0xFFFFFF47_u32))
                                .child(format!("CH{:02}", index + 1)),
                        ),
                ),
        )
}

fn inserts_section(track: &TrackState, index: usize) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .h(px(SEC_INSERTS_H))
        .border_b(px(1.0))
        .border_color(rgba(0xFFFFFF0B_u32))
        .child(section_header("INSERTS", track.color))
        .child(if index == 0 {
            insert_row("Pro-Q 4", track.color).into_any_element()
        } else {
            empty_slot().into_any_element()
        })
}

fn sends_section(track: &TrackState) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .h(px(SEC_SENDS_H))
        .border_b(px(1.0))
        .border_color(rgba(0xFFFFFF0B_u32))
        .child(section_header("SENDS", track.color))
        .child(empty_slot())
}

fn pan_section(
    track: &TrackState,
    callbacks: &MixerCallbacks,
    _is_selected: bool,
) -> impl IntoElement {
    let pan_label: gpui::SharedString = format_pan_label(track.pan).into();

    let track_id = track.id.clone();
    let pan_cb = callbacks.on_pan_change.clone();
    let on_pan_change = move |new_pan: &f32, w: &mut gpui::Window, cx: &mut gpui::App| {
        pan_cb(&(track_id.clone(), *new_pan), w, cx);
    };

    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(2.0))
        .h(px(SEC_PAN_H))
        .py(px(6.0))
        .border_b(px(1.0))
        .border_color(rgba(0xFFFFFF0B_u32))
        .child(knob_bipolar(
            format!("mix-pan-{}", track.id),
            track.pan,
            -1.0,
            1.0,
            track.color,
            Some(pan_label),
            0.0,
            on_pan_change,
        ))
        // L / R legend under the pill so the user can read the knob axis.
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .w(px(40.0))
                .mt(px(1.0))
                .child(
                    div()
                        .text_size(px(7.0))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(rgba(0xFFFFFF38_u32))
                        .child("L"),
                )
                .child(
                    div()
                        .text_size(px(7.0))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(rgba(0xFFFFFF38_u32))
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
                .h(px(FADER_TRACK_HEIGHT))
                .child(db_scale_column())
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_1()
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
                .child(vu_meter_vertical(
                    track.meter_level_l,
                    track.meter_level_r,
                    FADER_TRACK_HEIGHT,
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
        .border_color(rgba(0xFFFFFF0F_u32))
        .bg(rgba(0x0000003A_u32))
        .child(
            div()
                .text_size(px(10.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgba(0xEEF2F5AD_u32))
                .child(name.to_string()),
        )
}

// ─── Channel strip ──────────────────────────────────────────────────────────

fn channel_strip(
    track: &TrackState,
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
        rgba(0xFFFFFF14_u32)
    } else {
        rgba(0xFFFFFF07_u32)
    };
    let border_col = if is_selected {
        rgba(0xFFFFFF26_u32)
    } else {
        rgba(0xFFFFFF0A_u32)
    };

    let select_id = track.id.clone();
    let select_cb = callbacks.on_select_track.clone();
    let on_select_strip =
        move |_: &gpui::MouseDownEvent, w: &mut gpui::Window, cx: &mut gpui::App| {
            select_cb(&select_id, w, cx);
        };

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
        // Top accent line
        .child(div().w_full().h(px(2.0)).bg(track.color))
        .child(strip_header(track, index))
        .child(inserts_section(track, index))
        .child(sends_section(track))
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
        .bg(rgba(0x5FCED00C_u32))
        .border_l(px(1.0))
        .border_color(rgba(0xFFFFFF1A_u32))
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
                .border_color(rgba(0xFFFFFF0B_u32))
                .child(div().w(px(2.0)).h(px(24.0)).rounded_full().bg(accent))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(px(10.0))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(rgba(0xFFFFFFCC_u32))
                                .child("Master"),
                        )
                        .child(
                            div()
                                .text_size(px(7.5))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(rgba(0xFFFFFF47_u32))
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
                .border_color(rgba(0xFFFFFF0B_u32))
                .child(section_header("INSERTS", accent))
                .child(empty_slot()),
        )
        // Master skips sends, but we keep a same-sized spacer so its rows
        // line up with the channel strips.
        .child(
            div()
                .h(px(SEC_SENDS_H))
                .border_b(px(1.0))
                .border_color(rgba(0xFFFFFF0B_u32)),
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
                .border_color(rgba(0xFFFFFF0B_u32))
                .child({
                    let mut border = accent;
                    border.a = 0.55;
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .min_w(px(46.0))
                        .px(px(6.0))
                        .h(px(14.0))
                        .rounded_sm()
                        .bg(rgba(0x0000004A_u32))
                        .border(px(1.0))
                        .border_color(border)
                        .text_size(px(9.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(Colors::text_secondary())
                        .child("STEREO")
                })
                .child(
                    div()
                        .text_size(px(7.5))
                        .text_color(rgba(0xFFFFFF38_u32))
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
                        .h(px(FADER_TRACK_HEIGHT))
                        .child(db_scale_column())
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .flex_1()
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
                        .child(vu_meter_vertical(
                            master.meter_level_l,
                            master.meter_level_r,
                            FADER_TRACK_HEIGHT,
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
                .border_t(px(1.0))
                .border_color(rgba(0xFFFFFF0B_u32))
                .child(
                    div()
                        .text_size(px(8.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgba(0xFFFFFF38_u32))
                        .child("master"),
                ),
        )
        .child(strip_footer("Master"))
}

// ─── Public: Mixer Panel ─────────────────────────────────────────────────────

pub fn mixer_panel(
    tracks: &[TrackState],
    master: &MasterBusState,
    selected_track_id: Option<&str>,
    callbacks: MixerCallbacks,
) -> impl IntoElement {
    let accent = Colors::accent_primary();
    let track_count = tracks.len();
    let on_master = callbacks.on_master_volume_change.clone();

    let strips: Vec<gpui::AnyElement> = tracks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let is_sel = selected_track_id == Some(t.id.as_str());
            channel_strip(t, i, is_sel, &callbacks).into_any_element()
        })
        .collect();

    div()
        .flex()
        .flex_col()
        .size_full()
        .bg(rgba(0x111418FF_u32))
        .child(mixer_sub_header(track_count))
        // Content row: scrollable channels (flex_1) + master block (fixed).
        .child(
            div()
                .flex()
                .flex_row()
                .flex_1()
                .min_h_0()
                .child(
                    // Channel scroll area
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .h_full()
                        .id("mixer-strips-scroll")
                        .overflow_x_scroll()
                        .overflow_y_scroll()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                // No `items_start` here — leaving the default
                                // stretch alignment lets each strip fill the
                                // scroll-area height, so the mixer resizes
                                // with the bottom panel instead of pinning
                                // at STRIP_MIN_HEIGHT.
                                .h_full()
                                .min_h_full()
                                .children(strips)
                                // Soft trailing fill so the scroll surface
                                // reads as deliberate empty space, not as a
                                // gap before master.
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .h_full()
                                        .bg(rgba(0x0B0E1300_u32)),
                                ),
                        ),
                )
                // Gutter separating channels from the master block.
                .child(div().w(px(1.0)).h_full().bg(rgba(0xFFFFFF1F_u32)))
                // Pinned master block
                .child(master_strip(accent, master, on_master)),
        )
}
