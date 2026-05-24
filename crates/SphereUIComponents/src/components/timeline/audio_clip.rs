use crate::components::timeline::timeline_state::{
    ClipDragItem, ClipState, TimelineState, TRACK_HEIGHT,
};
use crate::components::timeline::waveform_canvas::waveform_canvas;
use crate::theme::Colors;
use gpui::prelude::FluentBuilder;
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

pub fn audio_clip(
    clip: &ClipState,
    track_id: &str,
    track_color: gpui::Rgba,
    state: &TimelineState,
    on_select_clip: std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>,
    on_context_menu: Option<
        std::sync::Arc<dyn Fn(&(String, f32, f32), &mut gpui::Window, &mut gpui::App) + 'static>,
    >,
) -> impl IntoElement {
    let clip_id = clip.id.clone();
    let drag_clip_id = clip.id.clone();
    let drag_track_id = track_id.to_string();
    let drag_name = clip.name.clone();
    let drag_start_beat = clip.start_beat;
    let selected = state.selection.selected_clip_ids.contains(&clip.id);
    let pixels_per_second = state.viewport.pixels_per_second;
    let seconds_per_beat = state.seconds_per_beat();

    let left = state.beats_to_x(clip.start_beat);
    let width = (clip.duration_beats * seconds_per_beat * pixels_per_second).max(10.0);

    // Geometry offsets matching layout
    let pad = 7.0;
    let clip_h = TRACK_HEIGHT - pad * 2.0;

    let id_num = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        clip.id.hash(&mut hasher);
        hasher.finish() as usize
    };

    let on_select = on_select_clip.clone();
    let context_clip_id = clip.id.clone();

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
        .border_color(if selected {
            Colors::text_primary()
        } else {
            let mut c = track_color;
            c.a = 0.4;
            c
        })
        .cursor(gpui::CursorStyle::PointingHand)
        .id(("audio-clip", id_num))
        .on_mouse_down(
            gpui::MouseButton::Left,
            move |_event: &gpui::MouseDownEvent, window, cx| {
                on_select(&clip_id, window, cx);
            },
        )
        .when_some(on_context_menu, |this, cb| {
            this.on_mouse_down(
                gpui::MouseButton::Right,
                move |event: &gpui::MouseDownEvent, window, cx| {
                    let x: f32 = event.position.x.into();
                    let y: f32 = event.position.y.into();
                    cb(&(context_clip_id.clone(), x, y), window, cx);
                },
            )
        })
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
                .bg(gpui::rgba(0x0000003A)) // dark bar
                .border_t(px(1.0))
                .border_color(gpui::rgba(0xFFFFFF0F))
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
                .child(
                    div()
                        .text_size(px(8.0))
                        .ml_auto()
                        .flex_none()
                        .text_color(Colors::text_muted())
                        // display duration e.g. "8.0 bt"
                        .child(format!("{:.1} bt", clip.duration_beats)),
                ),
        )
}
