//! Beat/time ruler for the audio editor waveform view.

use gpui::{div, px, IntoElement, ParentElement, Styled};

use crate::AudioEditorTheme;

const RULER_H: f32 = 20.0;

pub fn ruler_height() -> f32 {
    RULER_H
}

/// Horizontal beat ruler aligned with the waveform scroll/zoom.
pub fn audio_ruler(
    clip_duration_beats: f32,
    clip_start_beat: f32,
    beats_per_bar: f32,
    pixels_per_beat: f32,
    scroll_x: f32,
    viewport_width: f32,
    theme: &AudioEditorTheme,
) -> impl IntoElement {
    let bpb = beats_per_bar.max(1.0);
    let visible_start_beat = (scroll_x / pixels_per_beat.max(0.0001)).floor();
    let visible_end_beat =
        visible_start_beat + (viewport_width / pixels_per_beat.max(0.0001)).ceil() + 1.0;
    let first_bar = (visible_start_beat / bpb).floor() as i32;
    let last_bar = (visible_end_beat / bpb).ceil() as i32;

    let mut ticks = Vec::new();
    for bar in first_bar..=last_bar {
        let beat = bar as f32 * bpb;
        if beat > clip_duration_beats + 0.001 {
            continue;
        }
        let x = beat * pixels_per_beat - scroll_x;
        if x < -4.0 || x > viewport_width + 4.0 {
            continue;
        }
        let abs_bar = ((clip_start_beat + beat) / bpb).floor() as i32 + 1;
        ticks.push(
            div()
                .absolute()
                .left(px(x))
                .top(px(0.0))
                .h_full()
                .flex()
                .flex_col()
                .child(
                    div()
                        .w(px(1.0))
                        .h(px(6.0))
                        .bg(theme.border_subtle),
                )
                .child(
                    div()
                        .ml(px(3.0))
                        .mt(px(1.0))
                        .text_size(px(9.0))
                        .text_color(theme.text_muted)
                        .child(format!("{abs_bar}")),
                ),
        );
    }

    div()
        .relative()
        .w_full()
        .h(px(RULER_H))
        .flex_none()
        .border_b(px(1.0))
        .border_color(theme.border_subtle)
        .bg(theme.surface_panel)
        .children(ticks)
}
