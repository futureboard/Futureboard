use crate::components::timeline::timeline_state::{
    ClipDragItem, ClipState, ClipType, TimelineState, TRACK_HEIGHT,
};
use crate::theme::Colors;
use gpui::prelude::FluentBuilder;
use gpui::{
    div, px, AppContext, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled,
};

pub fn midi_clip(
    clip: &ClipState,
    track_id: &str,
    track_color: gpui::Rgba,
    state: &TimelineState,
    on_select_clip: std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>,
    on_context_menu: Option<
        std::sync::Arc<dyn Fn(&(String, f32, f32), &mut gpui::Window, &mut gpui::App) + 'static>,
    >,
    on_open_editor: Option<std::sync::Arc<dyn Fn(&mut gpui::Window, &mut gpui::App) + 'static>>,
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

    let pad = 7.0;
    let clip_h = TRACK_HEIGHT - pad * 2.0;
    let note_h = clip_h - 14.0; // height for notes preview

    // Draw notes inside notes preview area
    let mut note_elements = Vec::new();
    if let ClipType::Midi { notes } = &clip.clip_type {
        // Find pitch range
        let mut top_pitch = 72;
        let mut bottom_pitch = 48;
        if !notes.is_empty() {
            let lo = notes.iter().map(|n| n.pitch).min().unwrap_or(48);
            let hi = notes.iter().map(|n| n.pitch).max().unwrap_or(72);
            top_pitch = hi + 2;
            bottom_pitch = lo - 2;
        }
        let pitch_range = (top_pitch - bottom_pitch).max(12) as f32;
        let ppb = pixels_per_second * seconds_per_beat; // pixels per beat

        for note in notes {
            let note_left = note.start * ppb;
            let note_width = (note.duration * ppb).max(2.0);

            // Normalize pitch to 0..1, then map to top coordinate
            let norm_pitch = (note.pitch - bottom_pitch) as f32 / pitch_range;
            let note_top = (1.0 - norm_pitch) * (note_h - 4.0) + 1.0;

            note_elements.push(
                div()
                    .absolute()
                    .left(px(note_left))
                    .top(px(note_top))
                    .w(px(note_width))
                    .h(px(2.0)) // thin note line
                    .bg({
                        let mut c = track_color;
                        c.a = 0.8;
                        c
                    }),
            );
        }
    }

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
        })
        .border(px(1.0))
        .border_color(if selected {
            Colors::text_primary()
        } else {
            let mut c = track_color;
            c.a = 0.4;
            c
        })
        .cursor(gpui::CursorStyle::PointingHand)
        .id(("midi-clip", id_num))
        .on_mouse_down(
            gpui::MouseButton::Left,
            move |event: &gpui::MouseDownEvent, window, cx| {
                // Stop the parent lane handler from re-selecting the track and
                // clearing this clip selection — the piano roll edits the
                // selected clip, so selection must survive the click.
                cx.stop_propagation();
                on_select(&clip_id, window, cx);
                if event.click_count >= 2 {
                    if let Some(open) = on_open_editor.as_ref() {
                        open(window, cx);
                    }
                }
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
                cx.new(
                    |_| crate::components::timeline::audio_clip::ClipDragPreview {
                        name: drag_name.clone(),
                        color: track_color,
                    },
                )
            },
        )
        .flex()
        .flex_col()
        .justify_between()
        // Notes preview area
        .child(div().flex_1().min_h_0().relative().children(note_elements))
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
                .child(
                    div()
                        .text_size(px(8.0))
                        .ml_auto()
                        .flex_none()
                        .text_color(Colors::text_muted())
                        .child(format!("{:.1} bt", clip.duration_beats)),
                ),
        )
}
