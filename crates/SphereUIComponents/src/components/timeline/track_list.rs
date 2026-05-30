use gpui::{div, px, IntoElement, ParentElement, Styled};

use crate::components::timeline::automation_lane::automation_lane;
use crate::components::timeline::timeline_state::{TimelineState, HEADER_WIDTH, TRACK_HEIGHT};
use crate::components::timeline::timeline_surface::timeline_surface;
use crate::components::timeline::track_header::{track_header, TrackHeaderCallbacks};
use crate::components::timeline::track_lane::track_lane;
use crate::theme::Colors;

/// Rows above/below the visible viewport that are kept rendered to prevent
/// pop-in during fast scrolling. Measured in track rows.
const OVERSCAN: usize = 2;

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
    on_open_editor: Option<std::sync::Arc<dyn Fn(&mut gpui::Window, &mut gpui::App) + 'static>>,
) -> impl IntoElement {
    let _s = crate::perf::PerfScope::enter("TrackList");
    let grid_width = state.viewport.viewport_width.max(1.0);
    let grid_height = state.viewport.viewport_height.max(TRACK_HEIGHT);
    let track_count = state.tracks.len();
    let total_tracks_height = track_count as f32 * TRACK_HEIGHT;
    // Where the "tail" (below the last actual track) begins in viewport coords.
    // Rows are shifted by `-scroll_y`, so we need to subtract scroll to find the
    // last track's bottom edge relative to the visible viewport.
    let tail_start_y = (total_tracks_height - state.viewport.scroll_y).max(0.0);

    if std::env::var_os("FUTUREBOARD_TIMELINE_BG_DEBUG").is_some() {
        eprintln!(
            "[timeline bg] tracks={} total_h={:.1} scroll_y={:.1} viewport_h={:.1} tail_start_y={:.1}",
            track_count,
            total_tracks_height,
            state.viewport.scroll_y,
            grid_height,
            tail_start_y
        );
    }
    let insert_y = state.drag_target_index.map(|index| {
        (index as f32 * TRACK_HEIGHT - state.viewport.scroll_y)
            .clamp(0.0, grid_height.max(TRACK_HEIGHT))
    });

    // ── Virtual row window ──────────────────────────────────────────────────
    // Only rows whose screen-space Y overlaps [0, viewport_height] are built.
    // The remainder is represented by opaque spacer divs at the top/bottom of
    // the flex column so the scroll geometry (total height, scrollbar thumb
    // size, drop-indicator positions) stays correct.
    let scroll_y = state.viewport.scroll_y;
    let viewport_height = state.viewport.viewport_height;

    let first_visible = (scroll_y / TRACK_HEIGHT).floor() as usize;
    let visible_start = first_visible.saturating_sub(OVERSCAN);
    let last_visible = ((scroll_y + viewport_height) / TRACK_HEIGHT).ceil() as usize;
    let visible_end = (last_visible + OVERSCAN).min(track_count);

    let top_spacer_h = visible_start as f32 * TRACK_HEIGHT;
    let bottom_spacer_h = track_count.saturating_sub(visible_end) as f32 * TRACK_HEIGHT;

    crate::perf::count(
        "visible_track_rows",
        visible_end.saturating_sub(visible_start) as u64,
    );

    let mut rows: Vec<gpui::AnyElement> =
        Vec::with_capacity(visible_end.saturating_sub(visible_start) + 2);

    if top_spacer_h > 0.0 {
        rows.push(
            div()
                .w_full()
                .h(px(top_spacer_h))
                .flex_none()
                .into_any_element(),
        );
    }

    for (rel_idx, track) in state.tracks[visible_start..visible_end].iter().enumerate() {
        let index = visible_start + rel_idx;
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
                        on_open_editor.clone(),
                    )),
            )
            .children(
                track
                    .automation_lanes
                    .iter()
                    .filter(|l| l.visible)
                    .map(|lane| automation_lane(lane, track.color, state)),
            );
        rows.push(row.into_any_element());
    }

    if bottom_spacer_h > 0.0 {
        rows.push(
            div()
                .w_full()
                .h(px(bottom_spacer_h))
                .flex_none()
                .into_any_element(),
        );
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
                // Base body background: always fill the full content viewport,
                // even when there are 0 tracks or the track list doesn't fill
                // the visible height.
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .bg(Colors::timeline_content_background()),
                )
                // Empty tail background below the last track, if any.
                .children((tail_start_y < grid_height).then(|| {
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(px(tail_start_y))
                        .bottom_0()
                        .bg(Colors::timeline_empty_body_background())
                }))
                .child(timeline_surface(state, grid_width, grid_height)),
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
        .children(insert_y.map(|y| {
            div()
                .absolute()
                .left_0()
                .right_0()
                .top(px((y - 1.0).max(0.0)))
                .h(px(2.0))
                .bg(Colors::accent_primary())
                .shadow_lg()
        }))
}
