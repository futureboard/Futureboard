use gpui::{div, px, IntoElement, ParentElement, Styled};
use crate::theme::Colors;

pub fn title_bar() -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .h(px(32.0))
        .px(px(12.0))
        .bg(Colors::bg_base())
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_color(Colors::text_secondary())
                        .text_sm()
                        .child("Futureboard Studio"),
                ),
        )
        .child(div().flex_1())
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .text_color(Colors::text_muted())
                .text_xs()
                .child("- [] X"),
        )
}
