use gpui::{div, IntoElement, ParentElement, Styled};
use crate::theme::Colors;

pub fn timeline_shell() -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .h_full()
        .bg(Colors::bg_base())
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .text_color(Colors::text_dim())
                .text_xs()
                .child("Timeline"),
        )
}
