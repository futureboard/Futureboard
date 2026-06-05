use crate::components::sidebar::SIDEBAR_WIDTH;
use crate::components::timeline::audio_clip::audio_clip;
use crate::components::timeline::automation_lane::automation_overlay;
use crate::components::timeline::midi_clip::midi_clip;
use crate::components::timeline::timeline_state::{
    automation_y_to_value, AutomationMarquee, ClipType, TimelineState, TimelineTool, TrackLaneMode,
    TrackState, HEADER_WIDTH, RULER_HEIGHT, TRACK_HEIGHT,
};
use crate::theme::Colors;
use gpui::prelude::FluentBuilder;
use gpui::{div, px, InteractiveElement, IntoElement, ParentElement, Styled};

/// Top chrome height above the timeline ruler — mirrors `timeline.rs` so the
/// automation lane can map a window-space click into a lane-local value.
const APP_CHROME_HEIGHT: f32 = 36.0;

/// Automation lane mouse-down payload: `(track_id, beat, value_norm, additive)`.
pub type AutomationDownCallback =
    std::sync::Arc<dyn Fn(&(String, f32, f32, bool), &mut gpui::Window, &mut gpui::App) + 'static>;

/// Cycle the automation target for a track (fired by the in-lane target chip).
pub type AutomationCycleCallback =
    std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>;

pub fn track_lane(
    track: &TrackState,
    track_index: usize,
    state: &TimelineState,
    on_select_track: std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>,
    on_select_clip: std::sync::Arc<
        dyn Fn(&(String, bool), &mut gpui::Window, &mut gpui::App) + 'static,
    >,
    on_add_clip: std::sync::Arc<
        dyn Fn(&(String, f32), &mut gpui::Window, &mut gpui::App) + 'static,
    >,
    _on_track_context_menu: Option<
        std::sync::Arc<dyn Fn(&(String, f32, f32), &mut gpui::Window, &mut gpui::App) + 'static>,
    >,
    on_clip_context_menu: Option<
        std::sync::Arc<dyn Fn(&(String, f32, f32), &mut gpui::Window, &mut gpui::App) + 'static>,
    >,
    on_open_editor: Option<std::sync::Arc<dyn Fn(&mut gpui::Window, &mut gpui::App) + 'static>>,
    on_range_start: Option<
        std::sync::Arc<dyn Fn(&(String, f32, bool), &mut gpui::Window, &mut gpui::App) + 'static>,
    >,
    on_erase_start: Option<
        std::sync::Arc<dyn Fn(&f32, &mut gpui::Window, &mut gpui::App) + 'static>,
    >,
    on_erase_clip: Option<
        std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>,
    >,
    erase_preview_ids: Option<&std::collections::HashSet<String>>,
    on_automation_down: Option<AutomationDownCallback>,
    on_automation_cycle: Option<AutomationCycleCallback>,
    automation_marquee: Option<&AutomationMarquee>,
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

    let viewport_w = state.viewport.viewport_width.max(1.0);

    // Map clips — skip lanes outside the horizontal viewport.
    let clip_elements: Vec<_> = track
        .clips
        .iter()
        .filter_map(|clip| {
            let seconds_per_beat = state.seconds_per_beat();
            let pixels_per_second = state.viewport.pixels_per_second;
            let clip_left = state.beats_to_x(clip.start_beat);
            let clip_width = (clip.duration_beats * seconds_per_beat * pixels_per_second).max(10.0);
            if clip_left + clip_width < 0.0 || clip_left > viewport_w {
                return None;
            }

            let track_color = track.color;
            let on_sel_clip = on_select_clip.clone();
            let on_clip_context = on_clip_context_menu.clone();
            let on_open = on_open_editor.clone();
            let on_del = on_erase_clip.clone();
            let erase_target = erase_preview_ids
                .map(|s| s.contains(&clip.id))
                .unwrap_or(false);
            Some(match clip.clip_type {
                ClipType::Audio { .. } => audio_clip(
                    clip,
                    &track.id,
                    track_color,
                    state,
                    on_sel_clip,
                    on_open,
                    on_clip_context,
                    on_del,
                    erase_target,
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
                    on_del,
                    erase_target,
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
    let state_erase = state.clone();
    let id_num = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        track.id.hash(&mut hasher);
        hasher.finish() as usize
    };

    let is_automation = track.lane_mode == TrackLaneMode::Automation;

    // In automation mode the clips are dimmed background context and a
    // full-lane interaction layer captures point edits so clip handlers never
    // fire. The automation line/points overlay is drawn on top.
    let automation_layers = is_automation.then(|| {
        let overlay = automation_overlay(track, state, TRACK_HEIGHT, automation_marquee);
        let interaction = on_automation_down.clone().map(|cb| {
            let state_auto = state.clone();
            let track_id_auto = track.id.clone();
            div()
                .absolute()
                .inset_0()
                .id(("automation-hit", id_num))
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    move |event: &gpui::MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        let wx: f32 = event.position.x.into();
                        let wy: f32 = event.position.y.into();
                        let lane_x = wx - SIDEBAR_WIDTH - HEADER_WIDTH;
                        let raw_beat = state_auto.x_to_beats(lane_x);
                        let snapped_sec =
                            state_auto.snap_time(raw_beat * state_auto.seconds_per_beat());
                        let beat = (snapped_sec / state_auto.seconds_per_beat()).max(0.0);
                        let local_y = (wy - APP_CHROME_HEIGHT - RULER_HEIGHT
                            + state_auto.viewport.scroll_y)
                            - track_index as f32 * TRACK_HEIGHT;
                        let value = automation_y_to_value(local_y, TRACK_HEIGHT);
                        let additive = event.modifiers.shift || event.modifiers.control;
                        cb(&(track_id_auto.clone(), beat, value, additive), window, cx);
                    },
                )
        });
        // Clickable target chip (top-left). Sits above the interaction layer so
        // clicking it cycles the target instead of adding a point.
        let target_name = state.active_automation_target(&track.id).display_name();
        let cycle_chip = on_automation_cycle.clone().map(|cb| {
            let tid = track.id.clone();
            div()
                .absolute()
                .left(px(4.0))
                .top(px(4.0))
                .id(("automation-target", id_num))
                .flex()
                .items_center()
                .gap(px(3.0))
                .px(px(6.0))
                .py(px(1.0))
                .rounded_sm()
                .bg(Colors::with_alpha(Colors::surface_base(), 0.85))
                .border(px(1.0))
                .border_color(Colors::with_alpha(Colors::accent_primary(), 0.6))
                .text_size(px(9.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(Colors::accent_primary())
                .cursor(gpui::CursorStyle::PointingHand)
                .hover(|s| s.bg(Colors::with_alpha(Colors::accent_primary(), 0.18)))
                .on_mouse_down(gpui::MouseButton::Left, move |_event, window, cx| {
                    cx.stop_propagation();
                    cb(&tid, window, cx);
                })
                .child(format!("AUTO · {}", target_name))
        });
        div()
            .absolute()
            .inset_0()
            .child(overlay)
            .children(interaction)
            .children(cycle_chip)
    });

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
                let click_beat = state_ref.x_to_beats(click_x);
                let snapped_sec = state_ref.snap_time(click_beat * state_ref.seconds_per_beat());
                let snapped_beat = snapped_sec / state_ref.seconds_per_beat();

                if active_tool == TimelineTool::Pen {
                    on_add(&(track_id_add.clone(), snapped_beat), window, cx);
                } else if active_tool == TimelineTool::Pointer {
                    let additive = event.modifiers.control || event.modifiers.platform;
                    if !additive {
                        on_select(&track_id_select, window, cx);
                    }
                    if let Some(start_range) = on_range_start.as_ref() {
                        start_range(
                            &(track_id_select.clone(), snapped_beat, additive),
                            window,
                            cx,
                        );
                    }
                } else {
                    on_select(&track_id_select, window, cx);
                }
            },
        )
        .when_some(on_erase_start, |this, start_erase| {
            let start_erase = start_erase.clone();
            this.on_mouse_down(
                gpui::MouseButton::Right,
                move |event: &gpui::MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    let x: f32 = event.position.x.into();
                    let click_x = x - SIDEBAR_WIDTH - HEADER_WIDTH;
                    let click_beat = state_erase.x_to_beats(click_x);
                    let snapped_sec =
                        state_erase.snap_time(click_beat * state_erase.seconds_per_beat());
                    let snapped_beat = snapped_sec / state_erase.seconds_per_beat();
                    start_erase(&snapped_beat, window, cx);
                },
            )
        })
        // Clips: full strength in Clip mode, dimmed background in Automation mode.
        .child(
            div()
                .absolute()
                .inset_0()
                .when(is_automation, |this| this.opacity(0.28))
                .children(clip_elements),
        )
        .children(automation_layers)
}
