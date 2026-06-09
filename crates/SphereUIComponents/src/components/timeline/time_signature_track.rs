use crate::components::timeline::timeline_state::{TimelineState, HEADER_WIDTH};
use crate::theme::Colors;
use gpui::prelude::FluentBuilder;
use gpui::{
    div, px, InteractiveElement, IntoElement, ParentElement, Styled,
};

pub type TimeSignatureTrackDownCallback = std::sync::Arc<
    dyn Fn(&(f64, Option<String>, bool, u32), &mut gpui::Window, &mut gpui::App) + 'static,
>;

pub type TimeSignatureTrackContextCallback = std::sync::Arc<
    dyn Fn(&(f64, Option<String>, f32, f32), &mut gpui::Window, &mut gpui::App) + 'static,
>;

/// Global Time Signature lane — compact marker blocks over the project map.
pub fn time_signature_track_lane(
    state: &TimelineState,
    lane_height: f32,
    on_down: Option<TimeSignatureTrackDownCallback>,
    on_context: Option<TimeSignatureTrackContextCallback>,
    on_hide: Option<std::sync::Arc<dyn Fn(&(), &mut gpui::Window, &mut gpui::App) + 'static>>,
) -> impl IntoElement {
    let lane_w = state.viewport.viewport_width.max(1.0);
    let points = state.time_signature_map.points.clone();
    let selected = state.selected_time_signature_point_id.clone();

    let markers: Vec<_> = points
        .iter()
        .filter_map(|p| {
            let x = state.beats_to_x(p.beat as f32);
            if x < -40.0 || x > lane_w + 40.0 {
                return None;
            }
            let selected = selected.as_deref() == Some(p.id.as_str());
            let (bg, border) = if selected {
                (
                    Colors::with_alpha(Colors::accent_primary(), 0.28),
                    Colors::accent_primary(),
                )
            } else {
                (
                    Colors::with_alpha(Colors::surface_raised(), 0.9),
                    Colors::border_subtle(),
                )
            };
            Some(
                div()
                    .absolute()
                    .left(px(x + 2.0))
                    .top(px(10.0))
                    .h(px(lane_height - 18.0))
                    .min_w(px(36.0))
                    .px(px(6.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .bg(bg)
                    .border(px(1.0))
                    .border_color(border)
                    .text_size(px(9.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(Colors::text_primary())
                    .child(p.label()),
            )
        })
        .collect();

    let interaction = on_down.map(|cb| {
        let state_hit = state.clone();
        div()
            .absolute()
            .inset_0()
            .id("time-signature-track-hit")
            .on_mouse_down(
                gpui::MouseButton::Left,
                move |event: &gpui::MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    let wx: f32 = event.position.x.into();
                    let lane_x = wx
                        - crate::components::sidebar::SIDEBAR_WIDTH
                        - HEADER_WIDTH;
                    let beat = state_hit.x_to_beat(lane_x).max(0.0);
                    let snapped = state_hit.snap_beats(beat as f32) as f64;
                    let ppb = state_hit.viewport.pixels_per_beat.max(1.0) as f64;
                    let beat_tol = 12.0 / ppb;
                    let point_id = state_hit.time_signature_point_at(snapped, beat_tol);
                    cb(
                        &(snapped, point_id, false, event.click_count as u32),
                        window,
                        cx,
                    );
                },
            )
            .when_some(on_context, |layer, ctx_cb| {
                let state_ctx = state.clone();
                layer.on_mouse_down(
                    gpui::MouseButton::Right,
                    move |event: &gpui::MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        let wx: f32 = event.position.x.into();
                        let sx: f32 = event.position.x.into();
                        let sy: f32 = event.position.y.into();
                        let lane_x = wx
                            - crate::components::sidebar::SIDEBAR_WIDTH
                            - HEADER_WIDTH;
                        let beat = state_ctx.x_to_beat(lane_x).max(0.0);
                        let ppb = state_ctx.viewport.pixels_per_beat.max(1.0) as f64;
                        let point_id =
                            state_ctx.time_signature_point_at(beat, 12.0 / ppb);
                        ctx_cb(&(beat, point_id, sx, sy), window, cx);
                    },
                )
            })
    });

    div()
        .flex()
        .flex_row()
        .h(px(lane_height))
        .w_full()
        .bg(Colors::surface_panel_alt())
        .border_b(px(1.0))
        .border_color(Colors::border_subtle())
        .child(
            div()
                .w(px(HEADER_WIDTH))
                .h_full()
                .border_r(px(1.0))
                .border_color(Colors::border_subtle())
                .flex()
                .items_center()
                .justify_between()
                .px(px(12.0))
                .child(
                    div()
                        .text_size(px(10.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(Colors::text_primary())
                        .child("Time Sig"),
                )
                .when_some(on_hide, |row, hide| {
                    row.child(
                        div()
                            .text_size(px(8.0))
                            .text_color(Colors::text_muted())
                            .cursor(gpui::CursorStyle::PointingHand)
                            .hover(|s| s.text_color(Colors::text_secondary()))
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                move |_e, window, cx| {
                                    cx.stop_propagation();
                                    hide(&(), window, cx);
                                },
                            )
                            .child("Hide"),
                    )
                }),
        )
        .child(
            div()
                .flex_1()
                .h_full()
                .relative()
                .overflow_hidden()
                .children(markers)
                .children(interaction),
        )
}
