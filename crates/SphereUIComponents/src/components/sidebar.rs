use gpui::{div, px, IntoElement, ParentElement, Styled};
use crate::theme::Colors;

pub fn sidebar() -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .w(px(200.0))
        .h_full()
        .bg(Colors::surface_panel())
        .child(
            div()
                .px(px(8.0))
                .py(px(6.0))
                .text_color(Colors::text_dim())
                .text_xs()
                .child("Browser"),
        )
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .text_color(Colors::text_dim())
                .text_xs()
                .child("File browser"),
        )
}
