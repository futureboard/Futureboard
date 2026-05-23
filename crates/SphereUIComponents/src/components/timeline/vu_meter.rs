use gpui::{div, px, IntoElement, ParentElement, Styled};
use crate::theme::Colors;

/// Legacy fallback meter, kept so old call sites still link. New code should
/// use [`vu_meter_with_levels`] and pass the track's real meter state.
pub fn vu_meter(track_id: &str) -> impl IntoElement {
    let (level_l, level_r) = match track_id {
        "track-1" => (0.62, 0.68),
        "track-2" => (0.42, 0.48),
        "track-3" => (0.15, 0.12),
        _ => (0.0, 0.0),
    };
    vu_meter_with_levels(level_l, level_r)
}

pub fn vu_meter_with_levels(level_l: f32, level_r: f32) -> impl IntoElement {
    let draw_bar = |level: f32| {
        let total_height = 16.0;
        let green_pct = 0.70;
        let yellow_pct = 0.90;

        let level_h = (level * total_height).round();
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
            .w(px(4.0))
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
        .gap(px(2.0))
        .w(px(10.0))
        .h(px(16.0))
        .child(draw_bar(level_l))
        .child(draw_bar(level_r))
}
