use gpui::{div, px, InteractiveElement, IntoElement, ParentElement, Styled};

use crate::theme::Colors;

#[derive(Debug, Clone)]
pub struct PerformanceOverlaySnapshot {
    pub renderer: String,
    pub display_sync: String,
    pub fps: f32,
    pub frame_ms: f32,
    pub peak_ms: f32,
    pub has_sample: bool,
    pub repaint_reason: String,
    pub audio: String,
}

pub fn performance_overlay(snapshot: &PerformanceOverlaySnapshot) -> impl IntoElement {
    let fps = if snapshot.has_sample {
        format!("{:.1}", snapshot.fps)
    } else {
        "—".to_string()
    };
    let frame = if snapshot.has_sample {
        format!("{:.2} ms", snapshot.frame_ms)
    } else {
        "—".to_string()
    };
    let peak = if snapshot.has_sample {
        format!("{:.2} ms", snapshot.peak_ms)
    } else {
        "—".to_string()
    };

    div()
        .absolute()
        .top(px(36.0))
        .right(px(12.0))
        .w(px(280.0))
        .flex()
        .flex_col()
        .gap(px(2.0))
        .p(px(10.0))
        .rounded_lg()
        .border(px(1.0))
        .border_color(Colors::border_subtle())
        .bg(Colors::surface_panel())
        .shadow_lg()
        .occlude()
        .child(overlay_title("Profiler"))
        .children([
            overlay_line("Audio", &snapshot.audio),
            overlay_line("Renderer", &snapshot.renderer),
            overlay_line("Display Sync", &snapshot.display_sync),
            overlay_line("FPS", &fps),
            overlay_line("Frame", &frame),
            overlay_line("Peak", &peak),
            overlay_line("Repaint", &snapshot.repaint_reason),
        ])
}

fn overlay_title(text: &'static str) -> impl IntoElement {
    div()
        .pb(px(4.0))
        .text_size(px(11.0))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(Colors::text_primary())
        .child(text)
}

fn overlay_line(label: &'static str, value: &str) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_start()
        .justify_between()
        .gap(px(8.0))
        .child(
            div()
                .w(px(88.0))
                .text_size(px(10.0))
                .text_color(Colors::text_muted())
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_size(px(10.0))
                .text_color(Colors::text_secondary())
                .child(value.to_string()),
        )
}
