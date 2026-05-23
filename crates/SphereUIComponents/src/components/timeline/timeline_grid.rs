use gpui::{div, px, IntoElement, ParentElement, Styled};
use crate::components::timeline::timeline_state::{TimelineState, GridLineLevel};

pub fn timeline_grid(state: &TimelineState, grid_width: f32, _grid_height: f32) -> impl IntoElement {
    let lines = state.get_arrangement_grid_lines(grid_width);
    
    // Alternating bar shading
    let ppb = state.viewport.pixels_per_second * state.seconds_per_beat();
    let bpb = state.beats_per_bar();
    let bar_w = bpb * ppb;
    
    let mut shading_elements = Vec::new();
    if bar_w >= 2.0 {
        let start_beat = state.viewport.scroll_x / ppb;
        let first_bar = (start_beat / bpb).floor() as i32;
        let last_bar = ((state.viewport.scroll_x + grid_width) / bar_w).ceil() as i32;
        
        for bar in first_bar..=last_bar {
            if bar % 2 == 0 {
                let bx = (bar as f32 * bar_w - state.viewport.scroll_x).round();
                shading_elements.push(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(px(bx))
                        .w(px(bar_w.round()))
                        .bg(gpui::Rgba { r: 1.0, g: 1.0, b: 1.0, a: 0.022 }) // matching "rgba(255,255,255,0.022)"
                );
            }
        }
    }

    div()
        .absolute()
        .inset_0()
        // Alternating shading
        .children(shading_elements)
        // Grid lines (sub, beat, then bar on top)
        .children(lines.into_iter().map(|line| {
            let alpha = match line.level {
                GridLineLevel::Bar => 0.14,
                GridLineLevel::Beat => 0.062,
                GridLineLevel::Sub => 0.026,
            };

            div()
                .absolute()
                .left(px(line.x))
                .top_0()
                .bottom_0()
                .w(px(1.0))
                .bg(gpui::Rgba { r: 1.0, g: 1.0, b: 1.0, a: alpha })
        }))
}
