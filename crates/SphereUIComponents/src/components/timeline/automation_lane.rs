use crate::components::timeline::timeline_state::{
    automation_value_to_y, evaluate_automation, AutomationLaneState, AutomationMarquee,
    TimelineState, TrackState, HEADER_WIDTH,
};
use crate::theme::Colors;
use gpui::{
    canvas, div, fill, point, px, size, Bounds, IntoElement, ParentElement, Pixels, Styled,
};

/// Standalone expanded sub-lane (used when a lane is explicitly `visible`).
/// Kept for the older row-style automation display; the primary editor is the
/// in-track [`automation_overlay`].
pub fn automation_lane(
    lane: &AutomationLaneState,
    track_color: gpui::Rgba,
    state: &TimelineState,
) -> impl IntoElement {
    let points_elements: Vec<_> = lane
        .points
        .iter()
        .map(|pt| {
            let x = state.beats_to_x(pt.beat);
            // Map 0..1 value to height 0..30 (with 5px vertical padding)
            let y = (1.0 - pt.value) * 30.0 + 5.0;

            div()
                .absolute()
                .left(px(x - 3.0))
                .top(px(y - 3.0))
                .w(px(6.0))
                .h(px(6.0))
                .rounded_full()
                .bg(track_color)
                .border(px(1.0))
                .border_color(Colors::text_primary())
        })
        .collect();

    div()
        .flex()
        .flex_row()
        .h(px(40.0))
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
                .px(px(16.0))
                .child(
                    div()
                        .text_size(px(9.0))
                        .text_color(Colors::text_muted())
                        .child(lane.name.clone()),
                ),
        )
        .child(
            // Clip automation points to the lane content rect so a point whose x
            // is at/left of the content edge (during scroll) never draws over the
            // left lane header.
            div()
                .flex_1()
                .h_full()
                .relative()
                .overflow_hidden()
                .children(points_elements)
                .children(crate::perf::ui_debug_clips_enabled().then(|| {
                    div()
                        .absolute()
                        .inset_0()
                        .border(px(1.0))
                        .border_color(gpui::rgb(0xff00ff))
                })),
        )
}

/// Draw the automation line + points for a track inside its own lane (the
/// in-track editor shown while the track is in Automation mode). Pure render of
/// state — never mutates. The line is sampled per visible column via
/// [`evaluate_automation`] so Hold steps and Linear ramps are both correct.
pub fn automation_overlay(
    track: &TrackState,
    state: &TimelineState,
    lane_height: f32,
    marquee: Option<&AutomationMarquee>,
) -> impl IntoElement {
    let target = state.active_automation_target(&track.id);
    let default_value = target.default_value();
    let lane = track.automation_lanes.iter().find(|l| l.target == target);
    let points = lane.map(|l| l.points.clone()).unwrap_or_default();

    let lane_w = state.viewport.viewport_width.max(1.0);
    let num_cols = lane_w.ceil().max(1.0) as usize;

    // Sample the curve at each column edge (num_cols + 1 vertices).
    let mut samples: Vec<f32> = Vec::with_capacity(num_cols + 1);
    for col in 0..=num_cols {
        let beat = state.x_to_beat(col as f32);
        let v = evaluate_automation(&points, beat, default_value);
        samples.push(automation_value_to_y(v, lane_height));
    }
    let baseline_y = automation_value_to_y(default_value, lane_height);

    let line_color = Colors::accent_primary();
    let baseline_color = Colors::with_alpha(Colors::text_primary(), 0.10);

    let line = canvas(
        |_b, _w, _cx| {},
        move |bounds: Bounds<Pixels>, (), window, _cx| {
            // Faint baseline at the target's default value.
            let bl = Bounds::new(
                bounds.origin + point(px(0.0), px(baseline_y)),
                size(px(lane_w), px(1.0)),
            );
            window.paint_quad(fill(bl, baseline_color));
            // Connect samples with 1px columns spanning the vertical slope so
            // steep ramps stay visually continuous.
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

    // Point markers — clickable visuals (hit-testing happens at the lane level).
    let markers: Vec<_> = points
        .iter()
        .filter_map(|p| {
            let x = state.beats_to_x(p.beat);
            if x < -8.0 || x > lane_w + 8.0 {
                return None;
            }
            let y = automation_value_to_y(p.value, lane_height);
            let (fill_color, ring) = if p.selected {
                (Colors::text_primary(), Colors::accent_primary())
            } else {
                (Colors::accent_primary(), Colors::text_primary())
            };
            let size_px = if p.selected { 9.0 } else { 7.0 };
            Some(
                div()
                    .absolute()
                    .left(px(x - size_px / 2.0))
                    .top(px(y - size_px / 2.0))
                    .w(px(size_px))
                    .h(px(size_px))
                    .rounded_full()
                    .bg(fill_color)
                    .border(px(1.0))
                    .border_color(ring),
            )
        })
        .collect();

    // Optional marquee rectangle (value/beat space → lane pixels).
    let marquee_el = marquee.filter(|m| m.track_id == track.id).map(|m| {
        let x0 = state.beats_to_x(m.start_beat.min(m.cur_beat));
        let x1 = state.beats_to_x(m.start_beat.max(m.cur_beat));
        let y0 = automation_value_to_y(m.start_value.max(m.cur_value), lane_height);
        let y1 = automation_value_to_y(m.start_value.min(m.cur_value), lane_height);
        div()
            .absolute()
            .left(px(x0))
            .top(px(y0))
            .w(px((x1 - x0).max(1.0)))
            .h(px((y1 - y0).max(1.0)))
            .bg(Colors::with_alpha(Colors::accent_primary(), 0.14))
            .border(px(1.0))
            .border_color(Colors::with_alpha(Colors::accent_primary(), 0.7))
    });

    div()
        .absolute()
        .inset_0()
        .child(line)
        .children(markers)
        .children(marquee_el)
}
