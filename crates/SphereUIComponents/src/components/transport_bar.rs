use gpui::{div, px, IntoElement, ParentElement, Styled};
use crate::theme::Colors;

pub fn transport_bar() -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .h(px(36.0))
        .px(px(8.0))
        .bg(Colors::surface_panel())
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .flex_1()
                .child(
                    div()
                        .text_color(Colors::text_dim())
                        .text_xs()
                        .child("File  Edit  View  Transport  Window  Help"),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_color(Colors::text_secondary())
                        .text_xs()
                        .child("<<  >  []  REC"),
                )
                .child(
                    div()
                        .text_color(Colors::text_primary())
                        .text_sm()
                        .child("1.1.1"),
                )
                .child(
                    div()
                        .text_color(Colors::text_secondary())
                        .text_xs()
                        .child("120 BPM  4/4"),
                ),
        )
        .child(div().w(px(8.0)))
}
