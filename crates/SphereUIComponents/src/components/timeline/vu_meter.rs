use crate::theme::Colors;
use gpui::{div, px, IntoElement, ParentElement, Styled};

/// Legacy zero meter, kept so old call sites still link. New code should use
/// [`vu_meter_with_levels`] and pass real engine-backed meter state.
pub fn vu_meter(track_id: &str) -> impl IntoElement {
    let _ = track_id;
    vu_meter_with_levels(0.0, 0.0)
}

pub fn vu_meter_with_levels(level_l: f32, level_r: f32) -> impl IntoElement {
    vu_meter_sized(level_l, level_r, 4.0, 16.0, 2.0)
}

pub fn vu_meter_vertical(level_l: f32, level_r: f32, height: f32) -> impl IntoElement {
    vu_meter_sized(level_l, level_r, 5.0, height, 1.0)
}

/// Full-height variant used by the mixer fader area: the meter stretches to
/// fill the parent's height, so it scales with the channel strip's flex_1
/// fader slot. Bars are positioned as a fraction of parent height (`top` /
/// `h(relative(...))`).
pub fn vu_meter_vertical_full(level_l: f32, level_r: f32) -> impl IntoElement {
    let width = 5.0_f32;
    let gap = 1.0_f32;

    let draw_bar = |level: f32| {
        let green_pct = 0.70_f32;
        let yellow_pct = 0.90_f32;

        let level_n = level.clamp(0.0, 1.0);
        let green_n = level_n.min(green_pct);
        let yellow_n = if level_n > green_n {
            (level_n - green_n).min(yellow_pct - green_pct)
        } else {
            0.0
        };
        let red_n = if level_n > green_n + yellow_n {
            level_n - green_n - yellow_n
        } else {
            0.0
        };

        let mut bar = div()
            .w(px(width))
            .h_full()
            .bg(gpui::rgba(0xFFFFFF0D))
            .rounded_sm()
            .relative();

        if green_n > 0.0 {
            bar = bar.child(
                div()
                    .absolute()
                    .left(px(0.0))
                    .right(px(0.0))
                    .bottom(px(0.0))
                    .h(gpui::relative(green_n))
                    .bg(Colors::status_success()),
            );
        }
        if yellow_n > 0.0 {
            bar = bar.child(
                div()
                    .absolute()
                    .left(px(0.0))
                    .right(px(0.0))
                    .bottom(gpui::relative(green_n))
                    .h(gpui::relative(yellow_n))
                    .bg(Colors::status_warning()),
            );
        }
        if red_n > 0.0 {
            bar = bar.child(
                div()
                    .absolute()
                    .left(px(0.0))
                    .right(px(0.0))
                    .bottom(gpui::relative(green_n + yellow_n))
                    .h(gpui::relative(red_n))
                    .bg(Colors::status_error()),
            );
        }
        bar
    };

    div()
        .flex()
        .flex_row()
        .gap(px(gap))
        .w(px(width * 2.0 + gap))
        .h_full()
        .child(draw_bar(level_l))
        .child(draw_bar(level_r))
}

fn vu_meter_sized(
    level_l: f32,
    level_r: f32,
    width: f32,
    height: f32,
    gap: f32,
) -> impl IntoElement {
    let draw_bar = |level: f32| {
        let total_height = height.max(1.0);
        let green_pct = 0.70;
        let yellow_pct = 0.90;

        let level_h = (level.clamp(0.0, 1.0) * total_height).round();
        let green_h = level_h.min((green_pct * total_height).round());
        let yellow_h = if level_h > green_h {
            (level_h - green_h).min(((yellow_pct - green_pct) * total_height).round())
        } else {
            0.0
        };
        let red_h = if level_h > green_h + yellow_h {
            level_h - green_h - yellow_h
        } else {
            0.0
        };

        div()
            .w(px(width))
            .h(px(total_height))
            .bg(gpui::rgba(0xFFFFFF0D)) // background track
            .rounded_sm()
            .relative()
            // Green segment
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .w_full()
                    .h(px(green_h))
                    .bg(Colors::status_success()),
            )
            // Yellow segment
            .child(
                div()
                    .absolute()
                    .bottom(px(green_h))
                    .w_full()
                    .h(px(yellow_h))
                    .bg(Colors::status_warning()),
            )
            // Red segment
            .child(
                div()
                    .absolute()
                    .bottom(px(green_h + yellow_h))
                    .w_full()
                    .h(px(red_h))
                    .bg(Colors::status_error()),
            )
    };

    div()
        .flex()
        .flex_row()
        .gap(px(gap))
        .w(px(width * 2.0 + gap))
        .h(px(height.max(1.0)))
        .child(draw_bar(level_l))
        .child(draw_bar(level_r))
}
