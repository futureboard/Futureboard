use gpui::{div, px, IntoElement, ParentElement, Styled};
use crate::components::timeline::timeline_state::{TimelineState, HEADER_WIDTH};
use crate::components::timeline::track_header::track_header;
use crate::components::timeline::track_lane::track_lane;
use crate::components::timeline::automation_lane::automation_lane;
use crate::components::timeline::timeline_grid::timeline_grid;

pub fn track_list(
    state: &TimelineState,
    on_select_track: std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>,
    on_select_clip: std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>,
    on_toggle_mute: std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>,
    on_toggle_solo: std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>,
    on_toggle_arm: std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>,
    on_delete_track: std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>,
    on_volume_change: std::sync::Arc<dyn Fn(&(String, f32), &mut gpui::Window, &mut gpui::App) + 'static>,
    on_add_clip: std::sync::Arc<dyn Fn(&(String, f32), &mut gpui::Window, &mut gpui::App) + 'static>,
) -> impl IntoElement {
    let grid_width = 5000.0;
    let grid_height = state.tracks.len() as f32 * 76.0;

    // Render stacked rows side by side
    let mut rows = Vec::new();
    for (index, track) in state.tracks.iter().enumerate() {
        let row = div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .child(track_header(
                        track,
                        index,
                        state,
                        on_select_track.clone(),
                        on_toggle_mute.clone(),
                        on_toggle_solo.clone(),
                        on_toggle_arm.clone(),
                        on_delete_track.clone(),
                        on_volume_change.clone(),
                    ))
                    .child(track_lane(
                        track,
                        index,
                        state,
                        on_select_track.clone(),
                        on_select_clip.clone(),
                        on_add_clip.clone(),
                    ))
            )
            .children(track.automation_lanes.iter().filter(|l| l.visible).map(|lane| {
                automation_lane(lane, track.color, state)
            }));
        rows.push(row);
    }

    div()
        .relative()
        .flex_1()
        .w_full()
        .child(
            // Shaded Grid lines background (right side)
            div()
                .absolute()
                .left(px(HEADER_WIDTH))
                .right_0()
                .top_0()
                .bottom_0()
                .child(timeline_grid(state, grid_width, grid_height))
        )
        // Stack of rows
        .child(
            div()
                .flex()
                .flex_col()
                .w_full()
                .children(rows)
        )

}
