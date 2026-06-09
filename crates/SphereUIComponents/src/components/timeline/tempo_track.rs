use crate::components::timeline::timeline_state::{
    bpm_to_y, TempoMap, TimelineState, HEADER_WIDTH, TEMPO_LANE_PAD,
};
use crate::theme::Colors;
use gpui::prelude::FluentBuilder;
use gpui::{
    canvas, div, fill, point, px, size, Bounds, InteractiveElement, IntoElement, ParentElement,
    Pixels, Styled,
};

/// Tempo Track mouse-down: `(beat, bpm, point_id, additive, click_count)`.
pub type TempoTrackDownCallback = std::sync::Arc<
    dyn Fn(&(f64, f64, Option<String>, bool, u32), &mut gpui::Window, &mut gpui::App) + 'static,
>;

/// Tempo Track context menu: `(beat, bpm, point_id, screen_x, screen_y)`.
pub type TempoTrackContextCallback = std::sync::Arc<
    dyn Fn(&(f64, f64, Option<String>, f32, f32), &mut gpui::Window, &mut gpui::App) + 'static,
>;

const APP_CHROME_HEIGHT: f32 = 36.0;

/// Global Tempo Track lane — header + automation curve over the project TempoMap.
pub fn tempo_track_lane(
    state: &TimelineState,
    lane_height: f32,
    on_down: Option<TempoTrackDownCallback>,
    on_context: Option<TempoTrackContextCallback>,
    on_hide: Option<std::sync::Arc<dyn Fn(&(), &mut gpui::Window, &mut gpui::App) + 'static>>,
    on_toggle_collapsed: Option<
        std::sync::Arc<dyn Fn(&(), &mut gpui::Window, &mut gpui::App) + 'static>,
    >,
) -> impl IntoElement {
    let (min_bpm, max_bpm) = state.tempo_lane_bpm_range();
    let lane_w = state.viewport.viewport_width.max(1.0);
    let num_cols = lane_w.ceil().max(1.0) as usize;

    let mut samples: Vec<f32> = Vec::with_capacity(num_cols + 1);
    for col in 0..=num_cols {
        let beat = state.x_to_beat(col as f32);
        let bpm = state.effective_bpm_at_beat(beat);
        samples.push(bpm_to_y(bpm, lane_height, min_bpm, max_bpm));
    }

    let line_color = Colors::accent_primary();
    let baseline_bpm = if state.tempo_map.points.is_empty() {
        state.bpm as f64
    } else {
        state.tempo_map.points[0].bpm
    };
    let baseline_y = bpm_to_y(baseline_bpm, lane_height, min_bpm, max_bpm);
    let baseline_color = Colors::with_alpha(Colors::text_primary(), 0.08);

    let curve = canvas(
        |_b, _w, _cx| {},
        move |bounds: Bounds<Pixels>, (), window, _cx| {
            let bl = Bounds::new(
                bounds.origin + point(px(0.0), px(baseline_y)),
                size(px(lane_w), px(1.0)),
            );
            window.paint_quad(fill(bl, baseline_color));
            for col in 0..num_cols {
                let y0 = samples[col];
                let y1 = samples[col + 1];
                let top = y0.min(y1);
                let h = (y0 - y1).abs().max(1.6);
                let r = Bounds::new(
                    bounds.origin + point(px(col as f32), px(top)),
                    size(px(1.0), px(h)),
                );
                window.paint_quad(fill(r, line_color));
            }
        },
    )
    .absolute()
    .inset_0();

    let points = state.tempo_map.points.clone();
    let selected_id = state.selected_tempo_point_id.clone();
    let markers: Vec<_> = points
        .iter()
        .filter_map(|p| {
            let x = state.beats_to_x(p.beat as f32);
            if x < -12.0 || x > lane_w + 12.0 {
                return None;
            }
            let y = bpm_to_y(p.bpm, lane_height, min_bpm, max_bpm);
            let selected = selected_id.as_deref() == Some(p.id.as_str());
            let (fill_color, ring) = if selected {
                (Colors::text_primary(), Colors::accent_primary())
            } else {
                (Colors::accent_primary(), Colors::text_primary())
            };
            let size_px = if selected { 9.0 } else { 7.0 };
            let label = TempoMap::format_marker_label(p.bpm);
            Some(
                div()
                    .absolute()
                    .left(px(x - size_px / 2.0))
                    .top(px(y - size_px / 2.0))
                    .child(
                        div()
                            .absolute()
                            .bottom(px(size_px + 2.0))
                            .left(px(-8.0))
                            .text_size(px(8.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(Colors::text_secondary())
                            .child(label),
                    )
                    .child(
                        div()
                            .w(px(size_px))
                            .h(px(size_px))
                            .rounded_full()
                            .bg(fill_color)
                            .border(px(1.0))
                            .border_color(ring),
                    ),
            )
        })
        .collect();

    let range_label = format!("{:.0}–{:.0}", min_bpm.round(), max_bpm.round());
    let collapsed = state.tempo_track_collapsed;
    let collapse_label = if collapsed { "▸" } else { "▾" };

    let content_top = APP_CHROME_HEIGHT + crate::components::timeline::timeline_state::RULER_HEIGHT;

    let interaction = on_down.map(|cb| {
        let state_left = state.clone();
        let lane_h = lane_height;
        let min = min_bpm;
        let max = max_bpm;
        let mut layer = div()
            .absolute()
            .inset_0()
            .id("tempo-track-hit")
            .on_mouse_down(
                gpui::MouseButton::Left,
                move |event: &gpui::MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    let wx: f32 = event.position.x.into();
                    let wy: f32 = event.position.y.into();
                    let lane_x = wx - crate::components::sidebar::SIDEBAR_WIDTH - HEADER_WIDTH;
                    let beat = state_left.x_to_beat(lane_x).max(0.0);
                    let snapped = state_left.snap_beats(beat as f32) as f64;
                    let local_y = wy - content_top - TEMPO_LANE_PAD;
                    let bpm = crate::components::timeline::timeline_state::y_to_bpm(
                        local_y, lane_h, min, max,
                    );
                    let ppb = state_left.viewport.pixels_per_beat.max(1.0) as f64;
                    let beat_tol = 10.0 / ppb;
                    let usable = (lane_h - 2.0 * TEMPO_LANE_PAD).max(1.0);
                    let bpm_tol = (max - min) * 10.0 / usable as f64;
                    let point_id = state_left.tempo_point_at(snapped, bpm, beat_tol, bpm_tol);
                    let additive = event.modifiers.shift || event.modifiers.control;
                    cb(
                        &(snapped, bpm, point_id, additive, event.click_count as u32),
                        window,
                        cx,
                    );
                },
            );
        if let Some(ctx_cb) = on_context {
            let state_right = state.clone();
            layer = layer.on_mouse_down(
                gpui::MouseButton::Right,
                move |event: &gpui::MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    let wx: f32 = event.position.x.into();
                    let wy: f32 = event.position.y.into();
                    let sx: f32 = event.position.x.into();
                    let sy: f32 = event.position.y.into();
                    let lane_x = wx - crate::components::sidebar::SIDEBAR_WIDTH - HEADER_WIDTH;
                    let beat = state_right.x_to_beat(lane_x).max(0.0);
                    let local_y = wy - content_top - TEMPO_LANE_PAD;
                    let bpm = crate::components::timeline::timeline_state::y_to_bpm(
                        local_y, lane_h, min, max,
                    );
                    let ppb = state_right.viewport.pixels_per_beat.max(1.0) as f64;
                    let beat_tol = 10.0 / ppb;
                    let usable = (lane_h - 2.0 * TEMPO_LANE_PAD).max(1.0);
                    let bpm_tol = (max - min) * 10.0 / usable as f64;
                    let point_id = state_right.tempo_point_at(beat, bpm, beat_tol, bpm_tol);
                    ctx_cb(&(beat, bpm, point_id, sx, sy), window, cx);
                },
            );
        }
        layer
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
                .flex_col()
                .justify_center()
                .px(px(12.0))
                .gap(px(2.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            div()
                                .text_size(px(10.0))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(Colors::text_primary())
                                .child("Tempo"),
                        )
                        .when_some(on_toggle_collapsed, |row, toggle| {
                            row.child(
                                div()
                                    .text_size(px(9.0))
                                    .text_color(Colors::text_muted())
                                    .cursor(gpui::CursorStyle::PointingHand)
                                    .hover(|s| s.text_color(Colors::text_secondary()))
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        move |_e, window, cx| {
                                            cx.stop_propagation();
                                            toggle(&(), window, cx);
                                        },
                                    )
                                    .child(collapse_label),
                            )
                        })
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
                        .text_size(px(8.0))
                        .text_color(Colors::text_muted())
                        .child(format!("{range_label} BPM")),
                ),
        )
        .child(
            div()
                .flex_1()
                .h_full()
                .relative()
                .overflow_hidden()
                .child(curve)
                .children(markers)
                .children(interaction),
        )
}
