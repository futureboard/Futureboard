use crate::components::timeline::timeline_state::{
    midi_debug_enabled, ClipDragItem, ClipState, ClipType, TimelineState, TRACK_HEIGHT,
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
    on_erase_clip: Option<
        std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>,
    >,
    erase_target: bool,
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

    // Draw notes inside notes preview area (clip-relative beats, clip bounds).
    let mut note_elements = Vec::new();
    let clip_len = clip.duration_beats;
    if let ClipType::Midi { notes } = &clip.clip_type {
        let in_bounds: Vec<_> = notes
            .iter()
            .filter(|n| n.start < clip_len && n.start + n.duration > 0.0)
            .collect();
        let mut top_pitch = 72u8;
        let mut bottom_pitch = 48u8;
        if !in_bounds.is_empty() {
            let lo = in_bounds.iter().map(|n| n.pitch).min().unwrap_or(48);
            let hi = in_bounds.iter().map(|n| n.pitch).max().unwrap_or(72);
            top_pitch = hi.saturating_add(2).min(127);
            bottom_pitch = lo.saturating_sub(2);
        }
        let pitch_range = (top_pitch as i32 - bottom_pitch as i32).max(12) as f32;
        let ppb = pixels_per_second * seconds_per_beat;

        let preview_count = in_bounds.len();
        for note in &in_bounds {
            let visible_end = (note.start + note.duration).min(clip_len);
            let visible_start = note.start.max(0.0);
            if visible_end <= visible_start {
                continue;
            }
            let note_left = visible_start * ppb;
            let note_width = ((visible_end - visible_start) * ppb).max(2.0);
            let norm_pitch = (note.pitch as i32 - bottom_pitch as i32) as f32 / pitch_range;
            let note_top = (1.0 - norm_pitch) * (note_h - 4.0) + 1.0;

            note_elements.push(
                div()
                    .absolute()
                    .left(px(note_left))
                    .top(px(note_top))
                    .w(px(note_width))
                    .h(px(2.0))
                    .bg({
                        let mut c = track_color;
                        c.a = 0.8;
                        c
                    }),
            );
        }

        if midi_debug_enabled() {
            eprintln!(
                "[midi] preview clip={} notes={}/{} len={:.2}",
                clip.id,
                preview_count,
                notes.len(),
                clip_len
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
    let erase_cb = on_erase_clip.clone();
    let ctx_cb = on_context_menu.clone();
    let clip_for_erase = clip.id.clone();

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
        .border_color(if erase_target {
            Colors::status_error()
        } else if selected {
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
        .when_some(on_erase_clip.clone(), |this, _erase| {
            this.on_mouse_down(
                gpui::MouseButton::Right,
                move |event: &gpui::MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    if let Some(erase) = erase_cb.as_ref() {
                        erase(&clip_for_erase, window, cx);
                    } else if let Some(cb) = ctx_cb.as_ref() {
                        let x: f32 = event.position.x.into();
                        let y: f32 = event.position.y.into();
                        cb(&(context_clip_id.clone(), x, y), window, cx);
                    }
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
