use crate::components::timeline::timeline_state::{GridLineLevel, TimelineState};
use crate::theme::Colors;
use gpui::{div, px, IntoElement, ParentElement, Styled};

pub fn timeline_grid(
    state: &TimelineState,
    grid_width: f32,
    _grid_height: f32,
) -> impl IntoElement {
    let _s = crate::perf::PerfScope::enter("TimelineGrid");
    let lines = state.get_arrangement_grid_lines(grid_width);
    crate::perf::count("grid_lines", lines.len() as u64);

    let ppb = state.viewport.pixels_per_second * state.seconds_per_beat();
    let (visible_start, visible_end) = state.visible_beat_range(grid_width);
    let bar_rects = state
        .time_signature_map
        .visible_bar_rects(visible_start as f64, visible_end as f64);

    let mut shading_elements = Vec::new();
    for rect in bar_rects {
        if rect.bar % 2 != 0 {
            continue;
        }
        let x0 = (rect.start_beat as f32 * ppb - state.viewport.scroll_x).round();
        let x1 = (rect.end_beat as f32 * ppb - state.viewport.scroll_x).round();
        let width = x1 - x0;
        if width < 2.0 {
            continue;
        }
        shading_elements.push(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(x0))
                .w(px(width))
                .bg(Colors::with_alpha(Colors::text_primary(), 0.022)),
        );
    }

    div()
        .absolute()
        .inset_0()
        .child(div().absolute().inset_0().children(shading_elements))
        .child(
            div()
                .absolute()
                .inset_0()
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
                        .bg(Colors::with_alpha(Colors::text_primary(), alpha))
                })),
        )
}
