//! Main audio clip editor panel — waveform, ruler, metadata, playhead.

use gpui::{IntoElement, ParentElement, ScrollWheelEvent, Styled, div, px};

use crate::audio_editor_state::AudioEditorState;
use crate::audio_ruler::{audio_ruler, ruler_height};
use crate::waveform_view::{WaveformViewModel, waveform_view};

/// Theme tokens passed from the host shell (Futureboard dark DAW palette).
#[derive(Debug, Clone, Copy)]
pub struct AudioEditorTheme {
    pub surface_base: gpui::Rgba,
    pub surface_panel: gpui::Rgba,
    pub text_primary: gpui::Rgba,
    pub text_secondary: gpui::Rgba,
    pub text_muted: gpui::Rgba,
    pub border_subtle: gpui::Rgba,
    pub accent: gpui::Rgba,
    pub playhead: gpui::Rgba,
    pub error: gpui::Rgba,
    pub selection: gpui::Rgba,
}

/// Read-only view model built from project/timeline state each frame.
#[derive(Debug, Clone)]
pub struct AudioEditorViewModel {
    pub clip_id: String,
    pub clip_name: String,
    pub file_label: Option<String>,
    pub start_beat: f32,
    pub duration_beats: f32,
    pub offset_beats: f32,
    pub beats_per_bar: f32,
    pub bpm: f32,
    pub track_color: gpui::Rgba,
    pub waveform: WaveformViewModel,
    /// Playhead position relative to clip start, if visible.
    pub playhead_in_clip: Option<f32>,
    /// Selection overlay relative to clip start (beats).
    pub selection_range: Option<(f32, f32)>,
    pub theme: AudioEditorTheme,
}

const WAVEFORM_H: f32 = 120.0;
const GRID_SUBDIV: f32 = 0.25;

fn meta_chip(label: &'static str, value: String, theme: &AudioEditorTheme) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(4.0))
        .child(
            div()
                .text_size(px(9.0))
                .text_color(theme.text_muted)
                .child(label),
        )
        .child(
            div()
                .text_size(px(9.0))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(theme.text_secondary)
                .child(value),
        )
}

fn build_grid_lines(
    duration_beats: f32,
    pixels_per_beat: f32,
    scroll_x: f32,
    viewport_width: f32,
    view_h: f32,
    theme: &AudioEditorTheme,
) -> Vec<gpui::Div> {
    let mut lines = Vec::new();
    let step = GRID_SUBDIV;
    let start = (scroll_x / pixels_per_beat.max(0.0001)).floor();
    let end = start + (viewport_width / pixels_per_beat.max(0.0001)).ceil() + 1.0;
    let mut beat = (start / step).floor() * step;
    while beat <= end.min(duration_beats + step) {
        let x = beat * pixels_per_beat - scroll_x;
        if x >= 0.0 && x <= viewport_width {
            let is_bar = (beat.fract()).abs() < 0.001 || ((beat % 1.0).abs() < 0.001 && beat > 0.0);
            lines.push(
                div()
                    .absolute()
                    .left(px(x))
                    .top(px(0.0))
                    .w(px(1.0))
                    .h(px(view_h))
                    .bg(if is_bar {
                        theme.border_subtle
                    } else {
                        gpui::Rgba {
                            a: theme.border_subtle.a * 0.45,
                            ..theme.border_subtle
                        }
                    }),
            );
        }
        beat += step;
    }
    lines
}

fn playhead_overlay(x: f32, view_h: f32, theme: &AudioEditorTheme) -> impl IntoElement {
    div()
        .absolute()
        .left(px(x - 0.5))
        .top(px(0.0))
        .w(px(1.0))
        .h(px(view_h + ruler_height()))
        .bg(theme.playhead)
}

fn selection_overlay(x0: f32, x1: f32, view_h: f32, theme: &AudioEditorTheme) -> impl IntoElement {
    let left = x0.min(x1);
    let w = (x1 - x0).abs().max(1.0);
    div()
        .absolute()
        .left(px(left))
        .top(px(0.0))
        .w(px(w))
        .h(px(view_h))
        .bg(gpui::Rgba {
            a: 0.18,
            ..theme.selection
        })
        .border_l(px(1.0))
        .border_r(px(1.0))
        .border_color(gpui::Rgba {
            a: 0.55,
            ..theme.selection
        })
}

/// Render the audio editor body for `vm`. Uses `state` for scroll/zoom layout.
pub fn audio_editor_panel(
    vm: &AudioEditorViewModel,
    state: &AudioEditorState,
    viewport_width: f32,
) -> impl IntoElement {
    let ppb = state.pixels_per_beat;
    let scroll_x = state.scroll_x;
    let clip_width_px = vm.duration_beats * ppb;
    let view_h = WAVEFORM_H;

    let grid = build_grid_lines(
        vm.duration_beats,
        ppb,
        scroll_x,
        viewport_width,
        view_h,
        &vm.theme,
    );

    let waveform_el = waveform_view(
        view_h,
        clip_width_px,
        &vm.waveform,
        &vm.theme,
        vm.track_color,
    );

    let playhead = vm.playhead_in_clip.map(|rel| {
        let x = rel * ppb - scroll_x;
        playhead_overlay(x, view_h, &vm.theme)
    });

    let selection = vm.selection_range.map(|(a, b)| {
        let x0 = a * ppb - scroll_x;
        let x1 = b * ppb - scroll_x;
        selection_overlay(x0, x1, view_h, &vm.theme)
    });

    let offset_label = format!("{:.2} bt", vm.offset_beats);
    let length_label = format!("{:.2} bt", vm.duration_beats);
    let start_label = format!("{:.2} bt", vm.start_beat);
    let file_label = vm.file_label.clone().unwrap_or_else(|| "—".to_string());

    div()
        .flex()
        .flex_col()
        .size_full()
        .bg(vm.theme.surface_base)
        .child(
            div()
                .flex_none()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .h(px(28.0))
                .px(px(10.0))
                .border_b(px(1.0))
                .border_color(vm.theme.border_subtle)
                .bg(vm.theme.surface_panel)
                .child(
                    div()
                        .text_size(px(11.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(vm.theme.text_primary)
                        .child(vm.clip_name.clone()),
                )
                .child(
                    div()
                        .text_size(px(9.0))
                        .text_color(vm.theme.text_muted)
                        .truncate()
                        .max_w(px(280.0))
                        .child(file_label),
                )
                .child(meta_chip("Start", start_label, &vm.theme))
                .child(meta_chip("Length", length_label, &vm.theme))
                .child(meta_chip("Offset", offset_label, &vm.theme)),
        )
        .child(audio_ruler(
            vm.duration_beats,
            vm.start_beat,
            vm.beats_per_bar,
            ppb,
            scroll_x,
            viewport_width,
            &vm.theme,
        ))
        .child(
            div().flex_1().min_h_0().relative().overflow_hidden().child(
                div().absolute().top(px(0.0)).left(px(-scroll_x)).child(
                    div()
                        .relative()
                        .w(px(clip_width_px.max(viewport_width)))
                        .h(px(view_h))
                        .children(grid)
                        .child(waveform_el)
                        .children(selection)
                        .children(playhead),
                ),
            ),
        )
}

/// Empty state when no audio clip is selected.
pub fn empty_audio_editor(theme: &AudioEditorTheme) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_center()
        .size_full()
        .bg(theme.surface_base)
        .text_size(px(11.0))
        .text_color(theme.text_muted)
        .child("Select an audio clip to edit")
}

/// Default scroll/zoom handler for the audio editor waveform view.
pub fn default_wheel_handler(event: &ScrollWheelEvent, state: &mut AudioEditorState) {
    let (dx, dy) = match event.delta {
        gpui::ScrollDelta::Pixels(p) => (f32::from(p.x), f32::from(p.y)),
        gpui::ScrollDelta::Lines(p) => (p.x * 36.0, p.y * 36.0),
    };
    if event.modifiers.shift {
        state.scroll_x = (state.scroll_x - dy - dx).max(0.0);
    } else if event.modifiers.control || event.modifiers.platform {
        let factor = (1.0015_f32).powf(-dy);
        state.pixels_per_beat = (state.pixels_per_beat * factor).clamp(8.0, 512.0);
    } else {
        state.scroll_x = (state.scroll_x - dx).max(0.0);
    }
}
