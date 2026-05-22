use gpui::{div, px, IntoElement, ParentElement, Styled};
use crate::theme::Colors;

pub fn status_bar() -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .h(px(22.0))
        .px(px(8.0))
        .bg(Colors::surface_panel())
        .child(
            div()
                .flex_1()
                .text_color(Colors::text_dim())
                .text_xs()
                .child("Ready"),
        )
        .child(
            div()
                .text_color(Colors::text_dim())
                .text_xs()
                .child("44100 Hz  32-bit"),
        )
}
