use gpui::{div, px, rgba, svg, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement, Styled};

use crate::assets;
use crate::components::fader::db_value_pill;
use crate::components::knob::value_pill;
use crate::components::slider::slider;
use crate::components::timeline::timeline_state::{
    volume, TimelineState, TrackState, TrackType, HEADER_WIDTH, TRACK_HEIGHT,
};
use crate::components::timeline::vu_meter::vu_meter_with_levels;
use crate::theme::Colors;

type TrackCallback = std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>;
type VolumeCallback = std::sync::Arc<dyn Fn(&(String, f32), &mut gpui::Window, &mut gpui::App) + 'static>;

/// Bundle of callbacks the TrackHeader can fire. Keeping them in one struct
/// keeps the function signature manageable and lets new actions land without
/// re-threading every call site.
#[derive(Clone)]
pub struct TrackHeaderCallbacks {
    pub on_select_track: TrackCallback,
    pub on_toggle_mute: TrackCallback,
    pub on_toggle_solo: TrackCallback,
    pub on_toggle_arm: TrackCallback,
    pub on_toggle_input: TrackCallback,
    pub on_delete_track: TrackCallback,
    pub on_volume_change: VolumeCallback,
}

fn type_badge(kind: TrackType, color: gpui::Rgba) -> impl IntoElement {
    let label = match kind {
        TrackType::Audio => "AUD",
        TrackType::Midi => "MID",
        TrackType::Instrument => "INS",
        TrackType::Master => "MAS",
    };
    let mut bg = color;
    bg.a = 0.16;
    div()
        .px(px(3.0))
        .py(px(0.5))
        .rounded_sm()
        .bg(bg)
        .text_color(color)
        .text_size(px(8.0))
        .font_weight(gpui::FontWeight::BOLD)
        .child(label)
}

fn pill_button(
    id: gpui::ElementId,
    label: &'static str,
    icon: Option<&'static str>,
    active: bool,
    active_bg: gpui::Rgba,
    active_fg: gpui::Rgba,
    on_click: impl Fn(&gpui::MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    let mut btn = div()
        .flex()
        .items_center()
        .justify_center()
        .w(px(16.0))
        .h(px(16.0))
        .rounded_sm()
        .cursor(gpui::CursorStyle::PointingHand)
        .text_size(px(9.0))
        .font_weight(gpui::FontWeight::BOLD)
        .id(id)
        .on_mouse_down(gpui::MouseButton::Left, on_click);

    if active {
        btn = btn.bg(active_bg).text_color(active_fg);
    } else {
        btn = btn
            .bg(rgba(0xFFFFFF0D_u32))
            .text_color(Colors::text_secondary())
            .hover(|s| s.bg(rgba(0xFFFFFF18_u32)));
    }

    if let Some(path) = icon {
        btn.child(
            svg()
                .path(path)
                .w(px(10.0))
                .h(px(10.0))
                .text_color(if active { active_fg } else { Colors::text_secondary() }),
        )
    } else {
        btn.child(label)
    }
}

pub fn track_header(
    track: &TrackState,
    index: usize,
    state: &TimelineState,
    callbacks: TrackHeaderCallbacks,
) -> impl IntoElement {
    let track_id = track.id.clone();
    let is_selected = state.selection.selected_track_id.as_ref() == Some(&track.id);
    let header_bg = if is_selected {
        gpui::rgb(0x252c35)
    } else {
        gpui::rgb(0x1c2028)
    };

    let id_num = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        track.id.hash(&mut hasher);
        hasher.finish() as usize
    };

    // Build click handlers up-front so the closure types stay simple.
    let select_id = track_id.clone();
    let on_select_root = {
        let cb = callbacks.on_select_track.clone();
        move |_: &gpui::MouseDownEvent, window: &mut gpui::Window, cx: &mut gpui::App| {
            cb(&select_id, window, cx);
        }
    };

    let mute_id = track_id.clone();
    let on_mute = {
        let cb = callbacks.on_toggle_mute.clone();
        move |_: &gpui::MouseDownEvent, window: &mut gpui::Window, cx: &mut gpui::App| {
            cb(&mute_id, window, cx);
        }
    };

    let solo_id = track_id.clone();
    let on_solo = {
        let cb = callbacks.on_toggle_solo.clone();
        move |_: &gpui::MouseDownEvent, window: &mut gpui::Window, cx: &mut gpui::App| {
            cb(&solo_id, window, cx);
        }
    };

    let arm_id = track_id.clone();
    let on_arm = {
        let cb = callbacks.on_toggle_arm.clone();
        move |_: &gpui::MouseDownEvent, window: &mut gpui::Window, cx: &mut gpui::App| {
            cb(&arm_id, window, cx);
        }
    };

    let input_id = track_id.clone();
    let on_input = {
        let cb = callbacks.on_toggle_input.clone();
        move |_: &gpui::MouseDownEvent, window: &mut gpui::Window, cx: &mut gpui::App| {
            cb(&input_id, window, cx);
        }
    };

    let delete_id = track_id.clone();
    let on_delete = {
        let cb = callbacks.on_delete_track.clone();
        move |_: &gpui::MouseDownEvent, window: &mut gpui::Window, cx: &mut gpui::App| {
            cb(&delete_id, window, cx);
        }
    };

    let vol_id = track_id.clone();
    let on_volume_norm = {
        let cb = callbacks.on_volume_change.clone();
        move |new_norm: &f32, window: &mut gpui::Window, cx: &mut gpui::App| {
            cb(&(vol_id.clone(), *new_norm), window, cx);
        }
    };

    div()
        .flex()
        .flex_row()
        .w(px(HEADER_WIDTH))
        .h(px(TRACK_HEIGHT))
        .bg(header_bg)
        .border_r(px(1.0))
        .border_b(px(1.0))
        .border_color(Colors::border_subtle())
        .id(("track-header", id_num))
        .on_mouse_down(gpui::MouseButton::Left, on_select_root)
        // Left accent strip — same column as the track lane stripe
        .child(div().w(px(3.0)).h_full().bg(track.color))
        .child(
            div()
                .flex()
                .flex_col()
                .justify_between()
                .flex_1()
                .px(px(8.0))
                .py(px(7.0))
                // Row 1: name + type badge + per-track buttons
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .w_full()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(6.0))
                                .child(
                                    svg()
                                        .path(assets::ICON_GRIP_VERTICAL_PATH)
                                        .w(px(9.0))
                                        .h(px(9.0))
                                        .text_color(Colors::text_faint()),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .min_w(px(0.0))
                                        .child(
                                            div()
                                                .flex()
                                                .flex_row()
                                                .items_center()
                                                .gap(px(4.0))
                                                .child(
                                                    div()
                                                        .text_size(px(11.0))
                                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                                        .text_color(Colors::text_primary())
                                                        .child(track.name.clone()),
                                                )
                                                .child(type_badge(track.track_type, track.color)),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(8.5))
                                                .text_color(Colors::text_muted())
                                                .child(format!(
                                                    "CH {:02} · {} clips",
                                                    index + 1,
                                                    track.clips.len()
                                                )),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(2.0))
                                .px(px(3.0))
                                .py(px(2.0))
                                .rounded_md()
                                .bg(rgba(0x0000003A_u32))
                                .border(px(1.0))
                                .border_color(rgba(0xFFFFFF0F_u32))
                                .child(pill_button(
                                    ("mute-btn", id_num).into(),
                                    "M",
                                    None,
                                    track.muted,
                                    gpui::rgb(0xF3C969),
                                    gpui::rgb(0x101216),
                                    on_mute,
                                ))
                                .child(pill_button(
                                    ("solo-btn", id_num).into(),
                                    "S",
                                    None,
                                    track.solo,
                                    gpui::rgb(0x7BD88F),
                                    gpui::rgb(0x101216),
                                    on_solo,
                                ))
                                .child(pill_button(
                                    ("arm-btn", id_num).into(),
                                    "R",
                                    None,
                                    track.armed,
                                    gpui::rgb(0xF06A61),
                                    gpui::rgb(0x101216),
                                    on_arm,
                                ))
                                .child(pill_button(
                                    ("input-btn", id_num).into(),
                                    "I",
                                    None,
                                    track.input_monitor,
                                    Colors::accent_primary(),
                                    gpui::rgb(0x101216),
                                    on_input,
                                ))
                                .child(pill_button(
                                    ("del-btn", id_num).into(),
                                    "",
                                    Some(assets::ICON_X_PATH),
                                    false,
                                    rgba(0xFFFFFF0D_u32),
                                    Colors::text_secondary(),
                                    on_delete,
                                )),
                        ),
                )
                // Row 2: volume slider + pan pill + meter + dB pill
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .w_full()
                        .px(px(8.0))
                        .py(px(4.0))
                        .rounded_md()
                        .bg(rgba(0x0000002A_u32))
                        .border(px(1.0))
                        .border_color(rgba(0xFFFFFF09_u32))
                        // Real horizontal slider
                        .child(slider(
                            format!("track-vol-{}", track.id),
                            track.volume,
                            track.color,
                            on_volume_norm,
                        ))
                        // Bordered pan pill (same component the mixer knob uses).
                        .child(value_pill(pan_label(track.pan), track.color, is_selected))
                        // Compact meter
                        .child(vu_meter_with_levels(track.meter_level_l, track.meter_level_r))
                        // Bordered dB pill
                        .child(db_value_pill(volume::format_db(track.volume), is_selected)),
                ),
        )
}

fn pan_label(pan: f32) -> String {
    if pan.abs() < 0.01 {
        "C".to_string()
    } else {
        let p = (pan.abs() * 100.0).round() as i32;
        let p = p.clamp(1, 100);
        if pan < 0.0 { format!("L{}", p) } else { format!("R{}", p) }
    }
}
