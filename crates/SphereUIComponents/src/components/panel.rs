use gpui::{div, px, IntoElement, ParentElement, Styled};
use crate::theme::Colors;

pub fn right_panel() -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .w(px(220.0))
        .h_full()
        .bg(Colors::surface_panel())
        .border_l(px(1.0))
        .border_color(Colors::border_subtle())
        .child(
            // Inspector header
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
        .child(
            // No selection placeholder
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
                ),
        )
}

