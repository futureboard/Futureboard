use crate::components::timeline::global_lane_header::{
    global_lane_header, GlobalLaneHeaderActions,
};
use crate::components::timeline::timeline_state::TimelineState;
use crate::theme::Colors;
use gpui::prelude::FluentBuilder;
use gpui::{div, px, InteractiveElement, IntoElement, ParentElement, Styled};

pub type TimeSignatureTrackDownCallback = std::sync::Arc<
    dyn Fn(&(f64, Option<String>, bool, u32), &mut gpui::Window, &mut gpui::App) + 'static,
>;

pub type TimeSignatureTrackContextCallback = std::sync::Arc<
    dyn Fn(&(f64, Option<String>, f32, f32), &mut gpui::Window, &mut gpui::App) + 'static,
>;

pub type GlobalLaneVoidCallback =
    std::sync::Arc<dyn Fn(&(), &mut gpui::Window, &mut gpui::App) + 'static>;
pub type GlobalLaneMenuCallback =
    std::sync::Arc<dyn Fn(&(f32, f32), &mut gpui::Window, &mut gpui::App) + 'static>;

const TS_MARKER_W: f32 = 40.0;

/// Global Time Signature lane — compact marker blocks over the project map.
pub fn time_signature_track_lane(
    state: &TimelineState,
    lane_height: f32,
    on_down: Option<TimeSignatureTrackDownCallback>,
    on_context: Option<TimeSignatureTrackContextCallback>,
    on_add: Option<GlobalLaneVoidCallback>,
    on_header_menu: Option<GlobalLaneMenuCallback>,
    on_hide: Option<GlobalLaneVoidCallback>,
    on_toggle_collapsed: Option<GlobalLaneVoidCallback>,
) -> impl IntoElement {
    let lane_w = state.viewport.viewport_width.max(1.0);
    let points = state.time_signature_map.points.clone();
    let selected = state.selected_time_signature_point_id.clone();
    // Keep the pill compact (~24px) and always fully inside the lane so it never
    // clips against the top/bottom boundary.
    let marker_h = (lane_height - 12.0).clamp(18.0, 24.0);
    let marker_top = ((lane_height - marker_h) / 2.0).max(2.0);

    let markers: Vec<_> = points
        .iter()
        .filter_map(|p| {
            let x = state.beats_to_x(p.beat as f32);
            if x < -48.0 || x > lane_w + 48.0 {
                return None;
            }
            let selected = selected.as_deref() == Some(p.id.as_str());
            let (bg, border, text) = if selected {
                (
                    Colors::with_alpha(Colors::accent_primary(), 0.22),
                    Colors::accent_primary(),
                    Colors::text_primary(),
                )
            } else {
                (
                    Colors::with_alpha(Colors::surface_raised(), 0.92),
                    Colors::with_alpha(Colors::accent_primary(), 0.25),
                    Colors::text_secondary(),
                )
            };
            // Center the pill over the marker beat, then clamp it inside the lane
            // so labels near the left/right edge stay readable.
            let pill_x = (x - TS_MARKER_W / 2.0).clamp(2.0, (lane_w - TS_MARKER_W - 2.0).max(2.0));
            Some(
                div()
                    .absolute()
                    .left(px(pill_x))
                    .top(px(marker_top))
                    .w(px(TS_MARKER_W))
                    .h(px(marker_h))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .bg(bg)
                    .border(px(1.0))
                    .border_color(border)
                    .border_l(px(2.0))
                    .border_color(if selected {
                        Colors::accent_primary()
                    } else {
                        Colors::with_alpha(Colors::accent_primary(), 0.5)
                    })
                    .cursor(gpui::CursorStyle::PointingHand)
                    .hover(|s| {
                        s.bg(Colors::with_alpha(Colors::accent_primary(), 0.14))
                            .border_color(Colors::accent_primary())
                    })
                    .text_size(px(10.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(text)
                    .whitespace_nowrap()
                    .child(p.label()),
            )
        })
        .collect();

    let subtitle = state.time_signature_lane_header_subtitle();

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
                        - crate::components::timeline::timeline_state::HEADER_WIDTH;
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
                            - crate::components::timeline::timeline_state::HEADER_WIDTH;
                        let beat = state_ctx.x_to_beat(lane_x).max(0.0);
                        let ppb = state_ctx.viewport.pixels_per_beat.max(1.0) as f64;
                        let point_id = state_ctx.time_signature_point_at(beat, 12.0 / ppb);
                        ctx_cb(&(beat, point_id, sx, sy), window, cx);
                    },
                )
            })
    });

    let header = global_lane_header(
        "time-signature",
        "Time Signature",
        subtitle,
        state.time_signature_track_collapsed,
        "Hide Time Signature Track",
        GlobalLaneHeaderActions {
            on_add,
            on_menu: on_header_menu,
            on_hide,
            on_toggle_collapsed,
        },
    );

    div()
        .flex()
        .flex_row()
        .h(px(lane_height))
        .w_full()
        .bg(Colors::surface_panel_alt())
        .border_b(px(1.0))
        .border_color(Colors::border_subtle())
        .child(header)
        .child(
            div()
                .flex_1()
                .h_full()
                .relative()
                .overflow_hidden()
                .children(markers)
                .children(interaction)
                // Debug: outline time_signature_lane_content_rect (FUTUREBOARD_UI_DEBUG_CLIPS=1).
                .children(crate::perf::debug_clip_outline()),
        )
}
