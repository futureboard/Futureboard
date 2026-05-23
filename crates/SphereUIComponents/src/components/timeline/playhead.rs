use gpui::{div, px, svg, IntoElement, ParentElement, Styled};
use crate::theme::Colors;
use crate::components::timeline::timeline_state::TimelineState;

pub fn playhead(state: &TimelineState) -> impl IntoElement {
    let x = state.beats_to_x(state.transport.playhead_beats);
    
    // Position it relative to the scrollable content area
    div()
        .absolute()
        .top_0()
        .bottom_0()
        .left(px(x))
        .w(px(1.0))
        .bg(Colors::accent_primary())
        .child(
            // Playhead ruler handle SVG triangle/marker centered around line
            svg()
                .path(crate::assets::ICON_PLAYHEAD_HANDLE_PATH)
                .absolute()
                .top_0()
                .left(px(-5.5)) // center a 12px wide handle: left = -6px + 0.5px (half line width) = -5.5px
                .w(px(12.0))
                .h(px(12.0))
                .text_color(Colors::accent_primary())
        )
}
