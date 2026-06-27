use crate::components::timeline::timeline_state::{
    ClipDragItem, ClipEdge, ClipResizeDrag, ClipState, StretchMode, TimelineState,
};
use crate::components::timeline::waveform_canvas::waveform_canvas;
use crate::{custom_cursors, theme::Colors};
use gpui::{
    div, px, AppContext, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Window,
};

pub struct ClipDragPreview {
    pub name: String,
    pub color: gpui::Rgba,
}

impl Render for ClipDragPreview {
    fn render(&mut self, _w: &mut Window, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .h(px(24.0))
            .min_w(px(96.0))
            .max_w(px(220.0))
            .px(px(8.0))
            .rounded_sm()
            .border(px(1.0))
            .border_color({
                let mut c = self.color;
                c.a = 0.7;
                c
            })
            .bg(Colors::surface_raised())
            .shadow_lg()
            .child(
                div()
                    .min_w(px(0.0))
                    .flex_1()
                    .truncate()
                    .text_size(px(10.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(Colors::text_primary())
                    .child(self.name.clone()),
            )
    }
}

fn stretch_badge_label(clip: &ClipState, state: &TimelineState) -> Option<String> {
    if clip.stretch.mode == StretchMode::Off {
        return None;
    }
    if let Some(source_bpm) = clip.stretch.bpm_source {
        return Some(format!("{source_bpm:.0}->{:.0}", state.bpm));
    }
    let ratio = clip.stretch.effective_time_ratio(state.bpm as f64);
    if (ratio - 1.0).abs() > 0.001 {
        Some(format!("x{ratio:.2}"))
    } else {
        Some("Stretch".to_string())
    }
}

pub fn audio_clip(
    clip: &ClipState,
    track_id: &str,
    track_color: gpui::Rgba,
    state: &TimelineState,
    row_height: f32,
    on_select_clip: std::sync::Arc<
        dyn Fn(&(String, bool, bool), &mut gpui::Window, &mut gpui::App) + 'static,
    >,
    on_open_editor: Option<std::sync::Arc<dyn Fn(&mut gpui::Window, &mut gpui::App) + 'static>>,
    _on_context_menu: Option<
        std::sync::Arc<dyn Fn(&(String, f32, f32), &mut gpui::Window, &mut gpui::App) + 'static>,
    >,
    on_erase_clip: Option<
        std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>,
    >,
    erase_target: bool,
    auto_crossfade_in_beats: f32,
    auto_crossfade_out_beats: f32,
) -> impl IntoElement {
    let _s = crate::perf::PerfScope::enter("AudioClip");
    let clip_id = clip.id.clone();
    let drag_clip_id = clip.id.clone();
    let drag_track_id = track_id.to_string();
    let drag_name = clip.name.clone();
    let drag_start_beat = clip.start_beat;
    let selected = state.selection.selected_clip_ids.contains(&clip.id);
    let pixels_per_second = state.viewport.pixels_per_second;
    let seconds_per_beat = state.seconds_per_beat();
    let stretch_badge = stretch_badge_label(clip, state);
    let left = state.beats_to_x(clip.start_beat);
    let width = (clip.duration_beats * seconds_per_beat * pixels_per_second).max(10.0);
    let clip_duration_seconds = (clip.duration_beats * seconds_per_beat).max(0.001);
    let fade_in_seconds = (clip.stretch.fade_in_ms.max(0.0) / 1000.0)
        .max(auto_crossfade_in_beats.max(0.0) * seconds_per_beat)
        .min(clip_duration_seconds);
    let fade_out_seconds = (clip.stretch.fade_out_ms.max(0.0) / 1000.0)
        .max(auto_crossfade_out_beats.max(0.0) * seconds_per_beat)
        .min((clip_duration_seconds - fade_in_seconds).max(0.0));
    let fade_in_w = (fade_in_seconds * pixels_per_second).min(width);
    let fade_out_w = (fade_out_seconds * pixels_per_second).min((width - fade_in_w).max(0.0));

    // Geometry offsets matching layout
    let pad = 7.0;
    let clip_h = row_height - pad * 2.0;

    let id_num = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        clip.id.hash(&mut hasher);
        hasher.finish() as usize
    };

    let on_select = on_select_clip.clone();
    let open_editor = on_open_editor.clone();
    let clip_for_erase = clip.id.clone();
    let erase_cb = on_erase_clip.clone();
    let resize_left = ClipResizeDrag {
        clip_id: clip.id.clone(),
        edge: ClipEdge::Left,
        start_beat: clip.start_beat,
        duration_beats: clip.duration_beats,
    };
    let resize_right = ClipResizeDrag {
        clip_id: clip.id.clone(),
        edge: ClipEdge::Right,
        start_beat: clip.start_beat,
        duration_beats: clip.duration_beats,
    };
    const RESIZE_HANDLE_W: f32 = 6.0;

    div()
        .absolute()
        .left(px(left))
        .top(px(pad))
        .w(px(width))
        .h(px(clip_h))
        .rounded_md()
        .bg({
            let mut c = track_color;
            c.a = 0.12;
            c
        }) // semi-transparent background
        .border(px(1.0))
        .border_color(if erase_target {
            Colors::status_error()
        } else if selected {
            Colors::text_primary()
        } else {
            let mut c = track_color;
            c.a = 0.4;
            c
        })
        .cursor(custom_cursors::move_clip())
        .id(("audio-clip", id_num))
        .on_mouse_down(
            gpui::MouseButton::Left,
            move |event: &gpui::MouseDownEvent, window, cx| {
                cx.stop_propagation();
                let additive = event.modifiers.control || event.modifiers.platform;
                on_select(
                    &(clip_id.clone(), additive, event.modifiers.alt),
                    window,
                    cx,
                );
                if event.click_count >= 2 {
                    if let Some(open) = open_editor.as_ref() {
                        open(window, cx);
                    }
                }
            },
        )
        .on_mouse_down(
            gpui::MouseButton::Right,
            move |_event: &gpui::MouseDownEvent, window, cx| {
                cx.stop_propagation();
                if let Some(erase) = erase_cb.as_ref() {
                    erase(&clip_for_erase, window, cx);
                }
            },
        )
        .on_drag(
            ClipDragItem {
                clip_id: drag_clip_id,
                source_track_id: drag_track_id,
                start_beat: drag_start_beat,
            },
            move |_drag, _offset, _window, cx| {
                cx.new(|_| ClipDragPreview {
                    name: drag_name.clone(),
                    color: track_color,
                })
            },
        )
        .flex()
        .flex_col()
        .justify_between()
        // Waveform preview area
        .child(div().flex_1().min_h_0().child(waveform_canvas(
            clip,
            track_color,
            state,
            left,
            width,
        )))
        // Bottom Clip Label bar
        .child(
            div()
                .h(px(14.0))
                .bg(Colors::surface_panel_alt()) // dark bar
                .border_t(px(1.0))
                .border_color(Colors::divider())
                .px(px(6.0))
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(9.0))
                        .min_w(px(0.0))
                        .flex_1()
                        .truncate()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(if selected {
                            Colors::text_primary()
                        } else {
                            Colors::text_secondary()
                        })
                        .child(clip.name.clone()),
                )
                .children(stretch_badge.map(|label| {
                    div()
                        .ml(px(5.0))
                        .px(px(4.0))
                        .rounded_sm()
                        .bg(Colors::with_alpha(Colors::accent_primary(), 0.14))
                        .text_size(px(8.0))
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(Colors::accent_primary())
                        .child(label)
                })),
            // Clip length text intentionally not rendered on the clip body — the
            // name (flex_1) fills the bar, so no gap remains. Duration stays in the
            // model and the inspector; resize/trim handles are unaffected.
        )
        .children((fade_in_w > 1.0).then(|| {
            div()
                .absolute()
                .left_0()
                .top_0()
                .bottom(px(14.0))
                .w(px(fade_in_w))
                .cursor(custom_cursors::fade_in())
                .bg(Colors::with_alpha(Colors::surface_panel_alt(), 0.26))
                .border_r(px(1.0))
                .border_color(Colors::with_alpha(track_color, 0.55))
        }))
        .children((fade_out_w > 1.0).then(|| {
            div()
                .absolute()
                .right_0()
                .top_0()
                .bottom(px(14.0))
                .w(px(fade_out_w))
                .cursor(custom_cursors::fade_out())
                .bg(Colors::with_alpha(Colors::surface_panel_alt(), 0.26))
                .border_l(px(1.0))
                .border_color(Colors::with_alpha(track_color, 0.55))
        }))
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .h_full()
                .w(px(RESIZE_HANDLE_W))
                .cursor(custom_cursors::resize_left())
                .id(("audio-clip-resize-l", id_num))
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_drag(resize_left, |drag, _offset, _window, cx| {
                    cx.new(|_| drag.clone())
                }),
        )
        .child(
            div()
                .absolute()
                .top_0()
                .right_0()
                .h_full()
                .w(px(RESIZE_HANDLE_W))
                .cursor(custom_cursors::resize_right())
                .id(("audio-clip-resize-r", id_num))
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_drag(resize_right, |drag, _offset, _window, cx| {
                    cx.new(|_| drag.clone())
                }),
        )
}
