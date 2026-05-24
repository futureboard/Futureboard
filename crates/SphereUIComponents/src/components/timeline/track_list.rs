use gpui::{div, px, IntoElement, ParentElement, Styled};

use crate::components::timeline::automation_lane::automation_lane;
use crate::components::timeline::timeline_grid::timeline_grid;
use crate::components::timeline::timeline_state::{TimelineState, HEADER_WIDTH, TRACK_HEIGHT};
use crate::components::timeline::track_header::{track_header, TrackHeaderCallbacks};
use crate::components::timeline::track_lane::track_lane;

pub fn track_list(
    state: &TimelineState,
    header_callbacks: TrackHeaderCallbacks,
    on_select_track: std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>,
    on_select_clip: std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>,
    on_add_clip: std::sync::Arc<
        dyn Fn(&(String, f32), &mut gpui::Window, &mut gpui::App) + 'static,
    >,
) -> impl IntoElement {
    let grid_width = 5000.0;
    let grid_height = state.tracks.len() as f32 * TRACK_HEIGHT;

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
        .overflow_hidden()
        .child(
            div()
                .absolute()
                .left(px(HEADER_WIDTH))
                .right_0()
                .top_0()
                .bottom_0()
                .child(timeline_grid(state, grid_width, grid_height)),
        )
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
