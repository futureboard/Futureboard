use crate::components::sidebar::SIDEBAR_WIDTH;
use crate::components::timeline::audio_clip::audio_clip;
use crate::components::timeline::midi_clip::midi_clip;
use crate::components::timeline::timeline_state::{
    ClipType, TimelineState, TimelineTool, TrackState, HEADER_WIDTH, TRACK_HEIGHT,
};
use crate::theme::Colors;
use gpui::prelude::FluentBuilder;
use gpui::{div, px, InteractiveElement, IntoElement, ParentElement, Styled};

pub fn track_lane(
    track: &TrackState,
    track_index: usize,
    state: &TimelineState,
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
    let _s = crate::perf::PerfScope::enter("TrackLane");
    let track_id = track.id.clone();
    let is_track_selected = state.selection.selected_track_id.as_ref() == Some(&track.id);
    let even = track_index % 2 == 0;

    let bg = if is_track_selected {
        Colors::timeline_selected_lane_background()
    } else if even {
        Colors::timeline_lane_background()
    } else {
        Colors::timeline_lane_alt_background()
    };

    let on_select = on_select_track.clone();
    let track_id_select = track_id.clone();

    let on_add = on_add_clip.clone();
    let track_id_add = track_id.clone();
    let track_id_context = track_id.clone();

    let viewport_w = state.viewport.viewport_width.max(1.0);

    // Map clips — skip lanes outside the horizontal viewport.
    let clip_elements: Vec<_> = track
        .clips
        .iter()
        .filter_map(|clip| {
            let seconds_per_beat = state.seconds_per_beat();
            let pixels_per_second = state.viewport.pixels_per_second;
            let clip_left = state.beats_to_x(clip.start_beat);
            let clip_width =
                (clip.duration_beats * seconds_per_beat * pixels_per_second).max(10.0);
            if clip_left + clip_width < 0.0 || clip_left > viewport_w {
                return None;
            }

            let track_color = track.color;
            let on_sel_clip = on_select_clip.clone();
            let on_clip_context = on_clip_context_menu.clone();
            let on_open = on_open_editor.clone();
            Some(match clip.clip_type {
                ClipType::Audio { .. } => audio_clip(
                    clip,
                    &track.id,
                    track_color,
                    state,
                    on_sel_clip,
                    on_clip_context,
                )
                .into_any_element(),
                ClipType::Midi { .. } => midi_clip(
                    clip,
                    &track.id,
                    track_color,
                    state,
                    on_sel_clip,
                    on_clip_context,
                    on_open,
                )
                .into_any_element(),
            })
        })
        .collect();

    if crate::perf::enabled() {
        crate::perf::count("rendered_clips", clip_elements.len() as u64);
        crate::perf::count("total_clips", track.clips.len() as u64);
    }

    let active_tool = state.active_tool;
    let state_ref = state.clone();
    let id_num = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        track.id.hash(&mut hasher);
        hasher.finish() as usize
    };

    div()
        .flex_1()
        .h(px(TRACK_HEIGHT))
        .bg(bg)
        .border_b(px(1.0))
        .border_color(Colors::border_subtle())
        .relative()
        .overflow_hidden()
        .id(("track-lane", id_num))
        .on_mouse_down(
            gpui::MouseButton::Left,
            move |event: &gpui::MouseDownEvent, window, cx| {
                let x: f32 = event.position.x.into();
                let click_x = x - SIDEBAR_WIDTH - HEADER_WIDTH;

                if active_tool == TimelineTool::Pen {
                    // Pen tool adds a clip at the clicked location (snapped)
                    let click_beat = state_ref.x_to_beats(click_x);
                    let snapped_sec =
                        state_ref.snap_time(click_beat * state_ref.seconds_per_beat());
                    let snapped_beat = snapped_sec / state_ref.seconds_per_beat();
                    on_add(&(track_id_add.clone(), snapped_beat), window, cx);
                } else {
                    // Otherwise, clicking lane selects track and clears clip selection
                    on_select(&track_id_select, window, cx);
                }
            },
        )
        .when_some(on_track_context_menu, |this, cb| {
            this.on_mouse_down(
                gpui::MouseButton::Right,
                move |event: &gpui::MouseDownEvent, window, cx| {
                    let x: f32 = event.position.x.into();
                    let y: f32 = event.position.y.into();
                    cb(&(track_id_context.clone(), x, y), window, cx);
                },
            )
        })
        .children(clip_elements)
}
