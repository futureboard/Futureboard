use gpui::{div, px, InteractiveElement, IntoElement, ParentElement, Styled};
use crate::theme::Colors;

fn browser_item(label: &'static str, is_folder: bool) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px(px(8.0))
        .py(px(4.0))
        .rounded_md()
        .hover(|style| style.bg(Colors::surface_hover()))
        .child(
            div()
                .text_color(Colors::accent_primary())
                .text_xs()
                .child(if is_folder { "📁" } else { "📄" }),
        )
        .child(
            div()
                .text_color(Colors::text_secondary())
                .text_xs()
                .child(label),
        )
}

pub fn sidebar() -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .w(px(200.0))
        .h_full()
        .bg(Colors::surface_panel())
        .border_r(px(1.0))
        .border_color(Colors::border_subtle())
        .child(
            // Browser header
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
                        .child("Browser"),
                ),
        )
        .child(
            // Tree view container
            div()
                .flex_1()
                .flex_col()
                .px(px(6.0))
                .py(px(6.0))
                .gap_px()
                .child(browser_item("Audio Files", true))
                .child(browser_item("Plug-ins (VST3/CLAP)", true))
                .child(browser_item("Instruments", true))
                .child(browser_item("Projects", true))
                .child(browser_item("Samples", true))
                .child(browser_item("User Library", true))
                .child(browser_item("demo_loop_120bpm.wav", false))
                .child(browser_item("synth_lead_c3.wav", false))
        )
}

