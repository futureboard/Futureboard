use gpui::{div, px, IntoElement, ParentElement, Styled};
use crate::theme::Colors;

pub fn status_bar() -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .h(px(22.0))
        .px(px(10.0))
        .bg(Colors::surface_panel())
        .border_t(px(1.0))
        .border_color(Colors::border_subtle())
        .child(
            div()
                .flex_1()
                .text_color(Colors::text_muted())
                .text_size(px(10.5))
                .child("Ready"),
        )
        .child(
            div()
                .text_color(Colors::text_muted())
                .text_size(px(10.5))
                .child("44100 Hz  32-bit  -  DSP: 0.0%"),
        )
}

