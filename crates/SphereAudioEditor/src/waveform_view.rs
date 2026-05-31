//! Waveform peak drawing — min/max columns only, no PCM buffers.

use gpui::{IntoElement, ParentElement, Styled, div, px};

use crate::AudioEditorTheme;

#[derive(Debug, Clone, Copy, Default)]
pub struct WaveformColumn {
    pub x: f32,
    pub min: f32,
    pub max: f32,
}

#[derive(Debug, Clone)]
pub struct WaveformViewModel {
    pub columns: Vec<WaveformColumn>,
    pub ready: bool,
    pub status_label: String,
    pub is_error: bool,
    pub show_progress: bool,
}

impl WaveformViewModel {
    pub fn loading(label: impl Into<String>) -> Self {
        Self {
            columns: Vec::new(),
            ready: false,
            status_label: label.into(),
            is_error: false,
            show_progress: true,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            columns: Vec::new(),
            ready: false,
            status_label: message.into(),
            is_error: true,
            show_progress: false,
        }
    }
}

pub fn waveform_view(
    view_h: f32,
    clip_width_px: f32,
    waveform: &WaveformViewModel,
    theme: &AudioEditorTheme,
    waveform_color: gpui::Rgba,
) -> impl IntoElement {
    let center = view_h / 2.0;
    let mut color = waveform_color;
    color.a = 0.78;

    let zero_line = div()
        .absolute()
        .left_0()
        .right_0()
        .top(px(center - 0.5))
        .h(px(1.0))
        .bg(theme.border_subtle);

    let bars: Vec<_> = waveform
        .columns
        .iter()
        .filter_map(|col| {
            if col.min == 0.0 && col.max == 0.0 {
                return None;
            }
            let mn = col.min.clamp(-1.0, 1.0);
            let mx = col.max.clamp(-1.0, 1.0);
            let top = center - mx * center;
            let bottom = center - mn * center;
            let bar_h = (bottom - top).max(1.0);
            Some(
                div()
                    .absolute()
                    .left(px(col.x.round()))
                    .top(px(top))
                    .w(px(1.0))
                    .h(px(bar_h))
                    .bg(color),
            )
        })
        .collect();

    let status_overlay = if !waveform.ready {
        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .rounded_sm()
                        .border(px(1.0))
                        .border_color(if waveform.is_error {
                            theme.error
                        } else {
                            theme.border_subtle
                        })
                        .bg(gpui::Rgba {
                            a: 0.72,
                            ..theme.surface_base
                        })
                        .px(px(8.0))
                        .py(px(3.0))
                        .text_size(px(10.0))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(if waveform.is_error {
                            theme.error
                        } else {
                            theme.text_muted
                        })
                        .child(waveform.status_label.clone()),
                ),
        )
    } else {
        None
    };

    let progress_stripe = waveform.show_progress.then(|| {
        div()
            .absolute()
            .left_0()
            .right_0()
            .top(px(0.0))
            .h(px(2.0))
            .bg(gpui::Rgba {
                a: 0.55,
                ..theme.accent
            })
    });

    div()
        .relative()
        .w(px(clip_width_px.max(1.0)))
        .h(px(view_h))
        .overflow_hidden()
        .bg(gpui::Rgba {
            a: 0.35,
            ..theme.surface_base
        })
        .child(zero_line)
        .children(bars)
        .children(progress_stripe)
        .children(status_overlay)
}
