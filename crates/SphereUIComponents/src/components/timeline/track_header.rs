use gpui::prelude::FluentBuilder;
use gpui::{
    div, px, svg, AppContext, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Window,
};

use crate::assets;
use crate::components::fader::db_value_pill;
use crate::components::knob::format_pan_label;
use crate::components::slider::slider_with_drag_callbacks;
use crate::components::timeline::timeline_state::{
    volume, TimelineState, TrackDragItem, TrackLaneMode, TrackState, TrackType, HEADER_WIDTH,
    TRACK_HEADER_CONTROLS_MIN_HEIGHT,
};
use crate::components::timeline::vu_meter::vu_meter_with_levels;
use crate::theme::Colors;

type TrackCallback = std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>;
type VolumeCallback =
    std::sync::Arc<dyn Fn(&(String, f32), &mut gpui::Window, &mut gpui::App) + 'static>;
type VolumeCommitCallback =
    std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>;
type TrackContextCallback =
    std::sync::Arc<dyn Fn(&(String, f32, f32), &mut gpui::Window, &mut gpui::App) + 'static>;

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
    /// Toggle the track between Clip and Automation edit mode.
    pub on_toggle_automation: TrackCallback,
    pub on_delete_track: TrackCallback,
    pub on_volume_change: VolumeCallback,
    pub on_volume_drag_start: VolumeCallback,
    pub on_volume_drag_preview: VolumeCallback,
    pub on_volume_drag_commit: VolumeCommitCallback,
    pub on_context_menu: Option<TrackContextCallback>,
}

pub struct TrackDragPreview {
    pub name: String,
    pub color: gpui::Rgba,
}

impl Render for TrackDragPreview {
    fn render(&mut self, _w: &mut Window, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .h(px(28.0))
            .min_w(px(150.0))
            .px(px(8.0))
            .rounded_md()
            .border(px(1.0))
            .border_color({
                let mut c = self.color;
                c.a = 0.72;
                c
            })
            .bg(Colors::surface_raised())
            .shadow_lg()
            .child(div().w(px(3.0)).h(px(16.0)).rounded_full().bg(self.color))
            .child(
                div()
                    .ml(px(7.0))
                    .text_size(px(10.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(Colors::text_primary())
                    .child(self.name.clone()),
            )
    }
}

/// Semantic hue for a track type — keeps badges readable by type at a glance,
/// independent of the per-track identity color used for the accent strip.
fn track_type_color(kind: TrackType) -> gpui::Rgba {
    match kind {
        TrackType::Audio => Colors::accent_cyan(),
        TrackType::Instrument => Colors::accent_green(),
        TrackType::Midi => Colors::track_midi(),
        TrackType::Bus => Colors::track_bus(),
        TrackType::Return => Colors::track_return(),
        TrackType::Master => Colors::track_master(),
    }
}

fn type_badge(kind: TrackType) -> impl IntoElement {
    let label = match kind {
        TrackType::Audio => "AUD",
        TrackType::Midi => "MID",
        TrackType::Instrument => "INS",
        TrackType::Bus => "BUS",
        TrackType::Return => "RTN",
        TrackType::Master => "MAS",
    };
    let color = track_type_color(kind);
    // Readable, not neon: muted tinted chip with a slightly dimmed label.
    div()
        .px(px(3.0))
        .py(px(0.5))
        .rounded_sm()
        .bg(Colors::with_alpha(color, 0.14))
        .text_color(Colors::with_alpha(color, 0.92))
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
            .bg(Colors::with_alpha(Colors::text_primary(), 0.05))
            .text_color(Colors::text_secondary())
            .hover(|s| s.bg(Colors::surface_hover()));
    }

    if let Some(path) = icon {
        btn.child(
            svg()
                .path(path)
                .w(px(10.0))
                .h(px(10.0))
                .text_color(if active {
                    active_fg
                } else {
                    Colors::text_secondary()
                }),
        )
    } else {
        btn.child(label)
    }
}

pub fn track_header(
    track: &TrackState,
    index: usize,
    state: &TimelineState,
    row_height: f32,
    callbacks: TrackHeaderCallbacks,
) -> impl IntoElement {
    let _s = crate::perf::PerfScope::enter("TrackHeader");
    let track_id = track.id.clone();
    let is_selected = state.selection.selected_track_id.as_ref() == Some(&track.id);
    let is_automation = track.lane_mode == TrackLaneMode::Automation;
    // Adaptive header: the volume/pan/meter/dB control row only fits at the
    // default row height or taller. Below that we show the compact single-row
    // header so controls never overlap, clip, or float outside the row.
    let show_controls = row_height >= TRACK_HEADER_CONTROLS_MIN_HEIGHT;
    let is_dragging = state.dragging_track_id.as_deref() == Some(track.id.as_str());
    let is_drop_target =
        state.drag_target_index == Some(index) || state.drag_target_index == Some(index + 1);
    let header_bg = if is_dragging {
        Colors::with_alpha(Colors::text_primary(), 0.07)
    } else if is_automation {
        // Quiet graphite tint so the active automation track reads as active
        // without flooding the header with accent hue.
        Colors::surface_selected_soft()
    } else if is_selected {
        Colors::track_selected_overlay()
    } else if is_drop_target && state.dragging_track_id.is_some() {
        Colors::with_alpha(Colors::text_primary(), 0.05)
    } else {
        Colors::surface_panel()
    };
    // The parent header stays clean: it never names a single automation target.
    // When the automation section is expanded it shows only a compact lane count
    // indicator; the lane names live on the sub-lane headers below the track.
    let sub_label = if is_automation {
        let lane_count = track.automation_lanes.iter().filter(|l| l.visible).count();
        if lane_count == 1 {
            "1 automation lane".to_string()
        } else {
            format!("{lane_count} automation lanes")
        }
    } else {
        format!("CH {:02} · {} clips", index + 1, track.clips.len())
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

    let automation_id = track_id.clone();
    let on_automation = {
        let cb = callbacks.on_toggle_automation.clone();
        move |_: &gpui::MouseDownEvent, window: &mut gpui::Window, cx: &mut gpui::App| {
            cb(&automation_id, window, cx);
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
    let on_volume_drag_start = {
        let cb = callbacks.on_volume_drag_start.clone();
        let vol_id = vol_id.clone();
        move |new_norm: &f32, window: &mut gpui::Window, cx: &mut gpui::App| {
            cb(&(vol_id.clone(), *new_norm), window, cx);
        }
    };
    let on_volume_drag_preview = {
        let cb = callbacks.on_volume_drag_preview.clone();
        let vol_id = vol_id.clone();
        move |new_norm: &f32, window: &mut gpui::Window, cx: &mut gpui::App| {
            cb(&(vol_id.clone(), *new_norm), window, cx);
        }
    };
    let on_volume_drag_commit = {
        let cb = callbacks.on_volume_drag_commit.clone();
        let vol_id = vol_id.clone();
        move |window: &mut gpui::Window, cx: &mut gpui::App| {
            cb(&vol_id, window, cx);
        }
    };
    let reset_vol_id = track_id.clone();
    let on_volume_reset = {
        let cb = callbacks.on_volume_change.clone();
        move |window: &mut gpui::Window, cx: &mut gpui::App| {
            cb(
                &(
                    reset_vol_id.clone(),
                    crate::components::timeline::timeline_state::volume::db_to_norm(0.0),
                ),
                window,
                cx,
            );
        }
    };
    let context_id = track_id.clone();
    let on_context = callbacks.on_context_menu.clone();
    let drag_track_id = track_id.clone();
    let drag_name = track.name.clone();
    let drag_color = track.color;

    div()
        .flex()
        .flex_row()
        .w(px(HEADER_WIDTH))
        .h(px(row_height))
        // Clip to the row so a mid-resize frame can never paint controls
        // outside the row bounds; the adaptive layout keeps content within.
        .overflow_hidden()
        .bg(header_bg)
        .opacity(if is_dragging { 0.62 } else { 1.0 })
        // Stronger right border so the header column reads as a distinct
        // pane rather than blending into the lane area. The inner accent
        // strip on the right keeps the overall feel subtle.
        .border_r(px(1.0))
        .border_b(px(1.0))
        .border_color(Colors::border_strong())
        .id(("track-header", id_num))
        .on_mouse_down(gpui::MouseButton::Left, on_select_root)
        .when_some(on_context, |this, cb| {
            this.on_mouse_down(gpui::MouseButton::Right, move |event, window, cx| {
                let x: f32 = event.position.x.into();
                let y: f32 = event.position.y.into();
                cb(&(context_id.clone(), x, y), window, cx);
            })
        })
        // Left accent strip — same column as the track lane stripe
        .child(div().w(px(3.0)).h_full().bg(track.color))
        .child(
            div()
                .flex()
                .flex_col()
                // Two-row layout spreads; compact (single row) centers vertically.
                .when(show_controls, |c| c.justify_between())
                .when(!show_controls, |c| c.justify_center())
                .flex_1()
                .min_w_0()
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
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .w(px(15.0))
                                        .h(px(30.0))
                                        .rounded_sm()
                                        .id(("track-drag-handle", id_num))
                                        .cursor(gpui::CursorStyle::PointingHand)
                                        .hover(|s| s.bg(Colors::surface_hover()))
                                        .on_drag(
                                            TrackDragItem {
                                                track_id: drag_track_id,
                                                origin_index: index,
                                                name: drag_name.clone(),
                                                color: drag_color,
                                            },
                                            move |drag, _offset, _window, cx| {
                                                cx.new(|_| TrackDragPreview {
                                                    name: drag.name.clone(),
                                                    color: drag.color,
                                                })
                                            },
                                        )
                                        .child(
                                            svg()
                                                .path(assets::ICON_GRIP_VERTICAL_PATH)
                                                .w(px(9.0))
                                                .h(px(9.0))
                                                .text_color(Colors::text_faint()),
                                        ),
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
                                                        .min_w(px(0.0))
                                                        .truncate()
                                                        .text_size(px(11.0))
                                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                                        .text_color(Colors::text_primary())
                                                        .child(track.name.clone()),
                                                )
                                                .child(type_badge(track.track_type)),
                                        )
                                        .child(
                                            // Metadata stays in the muted text
                                            // ramp — never bright accent — so it
                                            // reads as secondary info, not a link.
                                            div()
                                                .text_size(px(8.5))
                                                .truncate()
                                                .text_color(if is_automation {
                                                    Colors::text_secondary()
                                                } else {
                                                    Colors::text_muted()
                                                })
                                                .child(sub_label),
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
                                .bg(Colors::surface_panel_alt())
                                .border(px(1.0))
                                .border_color(Colors::divider())
                                .child(pill_button(
                                    ("mute-btn", id_num).into(),
                                    "M",
                                    None,
                                    track.muted,
                                    Colors::accent_warning(),
                                    Colors::text_inverse(),
                                    on_mute,
                                ))
                                .child(pill_button(
                                    ("solo-btn", id_num).into(),
                                    "S",
                                    None,
                                    track.solo,
                                    Colors::accent_success(),
                                    Colors::text_inverse(),
                                    on_solo,
                                ))
                                .child(pill_button(
                                    ("arm-btn", id_num).into(),
                                    "R",
                                    None,
                                    track.armed,
                                    Colors::accent_danger(),
                                    Colors::text_inverse(),
                                    on_arm,
                                ))
                                .child(pill_button(
                                    ("input-btn", id_num).into(),
                                    "I",
                                    None,
                                    track.input_monitor.is_active(track.armed),
                                    Colors::accent_primary(),
                                    Colors::text_inverse(),
                                    on_input,
                                ))
                                // Automation mode toggle — switches the lane
                                // between Clip and Automation editing.
                                .child(pill_button(
                                    ("auto-btn", id_num).into(),
                                    "A",
                                    None,
                                    is_automation,
                                    Colors::accent_primary(),
                                    Colors::text_inverse(),
                                    on_automation,
                                ))
                                .child(pill_button(
                                    ("del-btn", id_num).into(),
                                    "",
                                    Some(assets::ICON_X_PATH),
                                    false,
                                    Colors::with_alpha(Colors::text_primary(), 0.05),
                                    Colors::text_secondary(),
                                    on_delete,
                                )),
                        ),
                )
                // Row 2: volume slider + pan pill + meter + dB pill.
                // Only rendered when the row is tall enough to hold it; the
                // compact header (short rows) shows just row 1.
                .when(show_controls, |col| {
                    col.child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.0))
                            .w_full()
                            .px(px(8.0))
                            .py(px(4.0))
                            .rounded_md()
                            .bg(Colors::with_alpha(Colors::surface_canvas(), 0.16))
                            .border(px(1.0))
                            .border_color(Colors::with_alpha(Colors::text_primary(), 0.03))
                            // Real horizontal slider
                            .child(slider_with_drag_callbacks(
                                format!("track-vol-{}", track.id),
                                state.display_track_volume(track),
                                track.color,
                                Some(on_volume_drag_start),
                                Some(on_volume_drag_preview),
                                Some(on_volume_drag_commit),
                                Some(on_volume_reset),
                            ))
                            // Pan readout — compact bordered label matching the
                            // dB pill alongside it.
                            .child({
                                let border = if is_selected {
                                    let mut c = track.color;
                                    c.a = 0.55;
                                    c
                                } else {
                                    Colors::border_default()
                                };
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .min_w(px(28.0))
                                    .px(px(5.0))
                                    .h(px(14.0))
                                    .rounded_sm()
                                    .bg(Colors::with_alpha(Colors::surface_canvas(), 0.3))
                                    .border(px(1.0))
                                    .border_color(border)
                                    .text_size(px(9.0))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(Colors::text_secondary())
                                    .child(format_pan_label(track.pan))
                            })
                            // Compact meter
                            .child(vu_meter_with_levels(
                                track.meter_level_l,
                                track.meter_level_r,
                            ))
                            // Bordered dB pill
                            .child(db_value_pill(
                                volume::format_db(state.display_track_volume(track)),
                                is_selected,
                            )),
                    )
                }),
        )
}
