use gpui::{div, px, IntoElement, ParentElement, Styled};

use crate::components::timeline::automation_lane::automation_lane;
use crate::components::timeline::timeline_grid::timeline_grid;
use crate::components::timeline::timeline_state::{TimelineState, HEADER_WIDTH, TRACK_HEIGHT};
use crate::components::timeline::track_header::{track_header, TrackHeaderCallbacks};
use crate::components::timeline::track_lane::track_lane;
use crate::theme::Colors;

pub fn track_list(
    state: &TimelineState,
    header_callbacks: TrackHeaderCallbacks,
    on_select_track: std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>,
    on_select_clip: std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>,
    on_add_clip: std::sync::Arc<
        dyn Fn(&(String, f32), &mut gpui::Window, &mut gpui::App) + 'static,
    >,
    on_track_context_menu: Option<
        std::sync::Arc<dyn Fn(&(String, f32, f32), &mut gpui::Window, &mut gpui::App) + 'static>,
    >,
    on_clip_context_menu: Option<
        std::sync::Arc<dyn Fn(&(String, f32, f32), &mut gpui::Window, &mut gpui::App) + 'static>,
    >,
) -> impl IntoElement {
    let _s = crate::perf::PerfScope::enter("TrackList");
    let grid_width = state.viewport.viewport_width.max(1.0);
    let grid_height = state.viewport.viewport_height.max(TRACK_HEIGHT);

    let mut rows = Vec::new();
    for (index, track) in state.tracks.iter().enumerate() {
        let row = div()
            .flex()
            .flex_col()
            .w_full()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .h(px(TRACK_HEIGHT))
                    .child(track_header(track, index, state, header_callbacks.clone()))
                    .child(track_lane(
                        track,
                        index,
                        state,
                        on_select_track.clone(),
                        on_select_clip.clone(),
                        on_add_clip.clone(),
                        on_track_context_menu.clone(),
                        on_clip_context_menu.clone(),
                    )),
            )
            .children(
                track
                    .automation_lanes
                    .iter()
                    .filter(|l| l.visible)
                    .map(|lane| automation_lane(lane, track.color, state)),
            );
        rows.push(row);
    }

    // Use `size_full()` (not `flex_1()`) so the track list fills the
    // surrounding wrapper. The parent in `timeline.rs` is a plain
    // (non-flex) sized block — `flex_1` there is a no-op and would
    // collapse this container to its content height. Since the rows and
    // grid below are absolutely positioned, that collapse hides every
    // TrackHeader / TrackLane row even though `state.tracks` is populated.
    div()
        .relative()
        .size_full()
        .bg(Colors::surface_base())
        .overflow_hidden()
        // Full-height left pane background. Individual TrackHeader rows scroll
        // over this, while the empty area below the final row still reads as
        // the same fixed header column and never receives grid/playhead paint.
        .child(
            div()
                .absolute()
                .left_0()
                .top_0()
                .bottom_0()
                .w(px(HEADER_WIDTH))
                .bg(Colors::surface_panel())
                .border_r(px(1.0))
                .border_color(Colors::border_strong())
                .child(
                    div()
                        .absolute()
                        .right(px(0.0))
                        .top_0()
                        .bottom_0()
                        .w(px(1.0))
                        .bg(Colors::border_strong()),
                ),
        )
        // Back layer: timeline body grid, clipped to the right content area.
        // This must stay before the row stack so lane row backgrounds and clips
        // are always painted above the base arrangement surface.
        .child(
            div()
                .absolute()
                .left(px(HEADER_WIDTH))
                .right_0()
                .top_0()
                .bottom_0()
                .child(timeline_grid(state, grid_width, grid_height)),
        )
        // Foreground layer: scrolling TrackHeader/TrackLane rows.
        .child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .top(px(-state.viewport.scroll_y))
                .flex()
                .flex_col()
                .w_full()
                .children(rows),
        )
}
