use gpui::{div, px, IntoElement, ParentElement, Styled};
use crate::theme::Colors;
use crate::components::timeline::timeline_state::{AutomationLaneState, TimelineState, HEADER_WIDTH};

pub fn automation_lane(
    lane: &AutomationLaneState,
    track_color: gpui::Rgba,
    state: &TimelineState,
) -> impl IntoElement {
    // Simply render a visual representation of automation points
    let points_elements: Vec<_> = lane.points.iter().map(|pt| {
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
    }).collect();

    div()
        .flex()
        .flex_row()
        .h(px(40.0))
        .w_full()
        .bg(gpui::rgba(0x0000001C)) // dark panel
        .border_b(px(1.0))
        .border_color(Colors::border_subtle())
        .child(
            // Left margin matches Track Header
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
                        .child(lane.name.clone())
                )
        )
        .child(
            // Right Lane Grid area
            div()
                .flex_1()
                .h_full()
                .relative()
                .children(points_elements)
        )
}
