use gpui::{div, px, IntoElement, ParentElement, Styled, InteractiveElement};
use crate::theme::Colors;
use crate::components::timeline::timeline_state::{TrackState, TimelineState, TRACK_HEIGHT, TimelineTool, ClipType};
use crate::components::timeline::audio_clip::audio_clip;
use crate::components::timeline::midi_clip::midi_clip;

pub fn track_lane(
    track: &TrackState,
    track_index: usize,
    state: &TimelineState,
    on_select_track: std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>,
    on_select_clip: std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>,
    on_add_clip: std::sync::Arc<dyn Fn(&(String, f32), &mut gpui::Window, &mut gpui::App) + 'static>,
) -> impl IntoElement {
    let track_id = track.id.clone();
    let is_track_selected = state.selection.selected_track_id.as_ref() == Some(&track.id);
    let even = track_index % 2 == 0;
    
    let bg = if is_track_selected {
        gpui::Rgba { r: 1.0, g: 1.0, b: 1.0, a: 0.028 }
    } else if even {
        gpui::Rgba { r: 1.0, g: 1.0, b: 1.0, a: 0.010 }
    } else {
        gpui::Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.12 }
    };

    let on_select = on_select_track.clone();
    let track_id_select = track_id.clone();
    
    let on_add = on_add_clip.clone();
    let track_id_add = track_id.clone();

    // Map clips
    let clip_elements: Vec<_> = track.clips.iter().map(|clip| {
        let track_color = track.color;
        let on_sel_clip = on_select_clip.clone();
        match clip.clip_type {
            ClipType::Audio { .. } => audio_clip(clip, track_color, state, on_sel_clip).into_any_element(),
            ClipType::Midi { .. } => midi_clip(clip, track_color, state, on_sel_clip).into_any_element(),
        }
    }).collect();

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
        .on_mouse_down(gpui::MouseButton::Left, move |event: &gpui::MouseDownEvent, window, cx| {
            // Screen coordinate subtraction
            // Sidebar is 272px, Track Header is HEADER_WIDTH
            let x: f32 = event.position.x.into();
            let click_x = x - 272.0 - crate::components::timeline::timeline_state::HEADER_WIDTH;
            
            if active_tool == TimelineTool::Pen {
                // Pen tool adds a clip at the clicked location (snapped)
                let click_beat = state_ref.x_to_beats(click_x);
                let snapped_sec = state_ref.snap_time(click_beat * state_ref.seconds_per_beat());
                let snapped_beat = snapped_sec / state_ref.seconds_per_beat();
                on_add(&(track_id_add.clone(), snapped_beat), window, cx);
            } else {
                // Otherwise, clicking lane selects track and clears clip selection
                on_select(&track_id_select, window, cx);
            }
        })
        .children(clip_elements)
}
