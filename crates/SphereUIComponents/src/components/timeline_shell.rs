use gpui::{div, px, IntoElement, ParentElement, Styled};
use crate::theme::Colors;

fn ruler_marker(label: &'static str) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .items_start()
        .justify_between()
        .h_full()
        .w(px(80.0))
        .border_l(px(1.0))
        .border_color(Colors::border_subtle())
        .child(
            div()
                .px(px(4.0))
                .py(px(2.0))
                .text_color(Colors::text_muted())
                .text_size(px(9.5))
                .child(label),
        )
}

fn track_header(name: &'static str, color: gpui::Rgba) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .h(px(76.0))
        .w_full()
        .px(px(8.0))
        .border_b(px(1.0))
        .border_color(Colors::border_subtle())
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .w(px(4.0))
                        .h(px(40.0))
                        .rounded_full()
                        .bg(color),
                )
                .child(
                    div()
                        .flex_col()
                        .child(
                            div()
                                .text_color(Colors::text_primary())
                                .text_xs()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(name),
                        )
                        .child(
                            div()
                                .text_color(Colors::text_muted())
                                .text_size(px(9.0))
                                .child("Stereo Audio"),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .gap_1()
                .child(
                    div()
                        .px(px(4.0))
                        .py(px(1.0))
                        .rounded_md()
                        .bg(Colors::surface_raised())
                        .text_color(Colors::text_muted())
                        .text_size(px(8.5))
                        .child("M"),
                )
                .child(
                    div()
                        .px(px(4.0))
                        .py(px(1.0))
                        .rounded_md()
                        .bg(Colors::surface_raised())
                        .text_color(Colors::text_muted())
                        .text_size(px(8.5))
                        .child("S"),
                ),
        )
}

fn clip_item(name: &'static str, width: f32, delay: f32) -> impl IntoElement {
    div()
        .h(px(60.0))
        .w(px(width))
        .ml(px(delay))
        .rounded_md()
        .bg(Colors::accent_soft())
        .border(px(1.0))
        .border_color(Colors::accent_primary())
        .px(px(6.0))
        .py(px(4.0))
        .child(
            div()
                .text_color(Colors::text_primary())
                .text_size(px(10.5))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(name),
        )
}

pub fn timeline_shell() -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .h_full()
        .bg(Colors::surface_base())
        .child(
            // Timeline Ruler
            div()
                .flex()
                .flex_row()
                .h(px(30.0))
                .w_full()
                .bg(Colors::surface_panel())
                .border_b(px(1.0))
                .border_color(Colors::border_subtle())
                // Ruler track headers space
                .child(
                    div()
                        .w(px(150.0))
                        .h_full()
                        .border_r(px(1.0))
                        .border_color(Colors::border_subtle()),
                )
                // Ruler time markings
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_1()
                        .child(ruler_marker("1.1.1"))
                        .child(ruler_marker("2.1.1"))
                        .child(ruler_marker("3.1.1"))
                        .child(ruler_marker("4.1.1"))
                        .child(ruler_marker("5.1.1"))
                        .child(ruler_marker("6.1.1"))
                        .child(ruler_marker("7.1.1")),
                ),
        )
        .child(
            // Main Tracks Area
            div()
                .flex()
                .flex_row()
                .flex_1()
                .relative()
                // Left track headers
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .w(px(150.0))
                        .h_full()
                        .bg(Colors::surface_panel())
                        .border_r(px(1.0))
                        .border_color(Colors::border_subtle())
                        .child(track_header("Audio 1", gpui::rgb(0x56C7C9)))
                        .child(track_header("Audio 2", gpui::rgb(0x7EDB9A)))
                        .child(track_header("Synth 3", gpui::rgb(0xF2C96D))),
                )
                // Right grid lanes
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .h_full()
                        .relative()
                        // Grid background ticks
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .size_full()
                                .py(px(0.0))
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .h(px(76.0))
                                        .w_full()
                                        .border_b(px(1.0))
                                        .border_color(Colors::border_subtle())
                                        .child(clip_item("vocals_dry.wav", 180.0, 20.0))
                                        .child(clip_item("vocals_harmony.wav", 120.0, 40.0)),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .h(px(76.0))
                                        .w_full()
                                        .border_b(px(1.0))
                                        .border_color(Colors::border_subtle())
                                        .child(clip_item("drums_loop_120.wav", 300.0, 0.0)),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .h(px(76.0))
                                        .w_full()
                                        .border_b(px(1.0))
                                        .border_color(Colors::border_subtle())
                                        .child(clip_item("synth_lead.mid", 220.0, 100.0)),
                                ),
                        )
                        // Playhead line placeholder (drawn over clips)
                        .child(
                            div()
                                .absolute()
                                .top(px(0.0))
                                .bottom(px(0.0))
                                .left(px(160.0))
                                .w(px(2.0))
                                .bg(Colors::accent_primary()),
                        ),
                ),
        )
}

