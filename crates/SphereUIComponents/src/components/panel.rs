use gpui::{div, px, IntoElement, ParentElement, Styled};

use crate::components::timeline::timeline_state::{volume, TrackState, TrackType};
use crate::theme::Colors;

/// Lightweight projection of the currently selected clip, built by the layout
/// from `TimelineState`. The inspector only needs a read-only summary.
pub struct SelectedClipSummary<'a> {
    pub name: &'a str,
    pub start_beat: f32,
    pub duration_beats: f32,
    pub kind: &'static str,
    pub track_name: &'a str,
}

/// Legacy entry point — kept so any existing call sites still compile. Returns
/// an empty placeholder identical to the pre-state version.
pub fn right_panel() -> impl IntoElement {
    inspector_shell().child(no_selection())
}

/// Inspector driven by the live selection. Renders one of:
/// 1. Clip details when a clip is selected.
/// 2. Track details when only a track is selected.
/// 3. "No Selection" placeholder otherwise.
pub fn inspector_panel<'a>(
    tracks: &'a [TrackState],
    selected_track_id: Option<&str>,
    selected_clip_id: Option<&str>,
    clip_summary: Option<SelectedClipSummary<'a>>,
) -> impl IntoElement {
    let body: gpui::AnyElement = if let Some(clip) = clip_summary {
        clip_inspector(clip).into_any_element()
    } else if let Some(tid) = selected_track_id {
        match tracks.iter().find(|t| t.id == tid) {
            Some(t) => track_inspector(t).into_any_element(),
            None => no_selection().into_any_element(),
        }
    } else {
        let _ = selected_clip_id; // currently only used via clip_summary
        no_selection().into_any_element()
    };

    inspector_shell().child(body)
}

fn inspector_shell() -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .w(px(292.0))
        .h_full()
        .bg(Colors::surface_panel())
        .border_l(px(1.0))
        .border_color(Colors::border_subtle())
        .child(
            div()
                .px(px(10.0))
                .py(px(8.0))
                .border_b(px(1.0))
                .border_color(Colors::border_subtle())
                .child(
                    div()
                        .text_color(Colors::text_primary())
                        .text_xs()
                        .font_weight(gpui::FontWeight::BOLD)
                        .child("Inspector"),
                ),
        )
}

fn no_selection() -> impl IntoElement {
    div()
        .flex_1()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_1()
        .child(
            div()
                .text_color(Colors::text_muted())
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child("No Selection"),
        )
        .child(
            div()
                .text_color(Colors::text_muted())
                .text_size(px(10.5))
                .child("Select a track or clip"),
        )
}

fn section_label(text: &'static str) -> impl IntoElement {
    div()
        .text_size(px(8.5))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(Colors::text_faint())
        .child(text)
}

fn kv_row(key: &'static str, value: String) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .justify_between()
        .items_center()
        .py(px(3.0))
        .child(
            div()
                .text_size(px(10.0))
                .text_color(Colors::text_muted())
                .child(key),
        )
        .child(
            div()
                .text_size(px(10.5))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(Colors::text_primary())
                .child(value),
        )
}

fn bool_badge(label: &'static str, active: bool, accent: gpui::Rgba) -> impl IntoElement {
    let (bg, fg) = if active {
        (accent, Colors::text_inverse().into())
    } else {
        (Colors::with_alpha(Colors::text_primary(), 0.05), Colors::text_secondary())
    };
    div()
        .flex()
        .items_center()
        .justify_center()
        .px(px(6.0))
        .py(px(2.0))
        .rounded_sm()
        .bg(bg)
        .text_color(fg)
        .text_size(px(9.0))
        .font_weight(gpui::FontWeight::BOLD)
        .child(label)
}

fn track_inspector(track: &TrackState) -> impl IntoElement {
    let kind_label = match track.track_type {
        TrackType::Audio => "Audio",
        TrackType::Midi => "MIDI",
        TrackType::Instrument => "Instrument",
        TrackType::Bus => "Bus",
        TrackType::Return => "Return",
        TrackType::Master => "Master",
    };
    let pan = if track.pan.abs() < 0.01 {
        "Center".to_string()
    } else if track.pan < 0.0 {
        format!(
            "L {}",
            (track.pan * -100.0).round().clamp(1.0, 100.0) as i32
        )
    } else {
        format!("R {}", (track.pan * 100.0).round().clamp(1.0, 100.0) as i32)
    };

    div()
        .flex_1()
        .flex()
        .flex_col()
        .px(px(10.0))
        .py(px(10.0))
        .gap(px(10.0))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(div().w(px(4.0)).h(px(18.0)).rounded_sm().bg(track.color))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(Colors::text_primary())
                        .child(track.name.clone()),
                ),
        )
        .child(section_label("TRACK"))
        .child(kv_row("Type", kind_label.to_string()))
        .child(kv_row(
            "Volume",
            format!("{} dB", volume::format_db(track.volume)),
        ))
        .child(kv_row("Pan", pan))
        .child(kv_row("Clips", format!("{}", track.clips.len())))
        .child(section_label("STATE"))
        .child(
            div()
                .flex()
                .flex_row()
                .gap(px(4.0))
                .pt(px(2.0))
                .child(bool_badge("M", track.muted, Colors::accent_warning()))
                .child(bool_badge("S", track.solo, Colors::accent_success()))
                .child(bool_badge("R", track.armed, Colors::accent_danger()))
                .child(bool_badge(
                    "I",
                    track.input_monitor,
                    Colors::accent_primary(),
                )),
        )
}

fn clip_inspector<'a>(clip: SelectedClipSummary<'a>) -> impl IntoElement {
    div()
        .flex_1()
        .flex()
        .flex_col()
        .px(px(10.0))
        .py(px(10.0))
        .gap(px(10.0))
        .child(
            div()
                .text_size(px(12.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(Colors::text_primary())
                .child(clip.name.to_string()),
        )
        .child(section_label("CLIP"))
        .child(kv_row("Type", clip.kind.to_string()))
        .child(kv_row("Track", clip.track_name.to_string()))
        .child(kv_row("Start", format!("{:.2} bt", clip.start_beat)))
        .child(kv_row("Length", format!("{:.2} bt", clip.duration_beats)))
}
