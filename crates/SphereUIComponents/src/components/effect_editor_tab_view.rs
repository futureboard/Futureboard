//! Effect Editor bottom-tab content — cheap static surface, entity-isolated repaint.

use gpui::{canvas, div, px, Context, IntoElement, ParentElement, Render, Styled, Window};

use crate::theme::Colors;

pub struct EffectEditorTabView;

impl EffectEditorTabView {
    pub fn new() -> Self {
        Self
    }
}

impl Render for EffectEditorTabView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let _scope = crate::perf::PerfScope::enter("BottomPanelEffectEditor");

        div()
            .flex()
            .flex_row()
            .items_start()
            .size_full()
            .relative()
            .child(effect_editor_background())
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .size_full()
                    .px(px(12.0))
                    .gap_3()
                    .child(effect_card("Equalizer", true))
                    .child(effect_card("Compressor", true))
                    .child(effect_card("Delay Unit", false))
                    .child(empty_effect_slot()),
            )
    }
}

fn effect_editor_background() -> impl IntoElement {
    canvas(
        |_bounds, _window, _cx| {},
        |bounds, (), window, _cx| {
            window.paint_quad(gpui::fill(bounds, Colors::surface_base()));
        },
    )
    .absolute()
    .inset_0()
}

fn effect_card(name: &'static str, active: bool) -> impl IntoElement {
    div()
        .w(px(130.0))
        .h(px(80.0))
        .rounded_md()
        .bg(Colors::surface_panel())
        .border(px(1.0))
        .border_color(if active {
            Colors::accent_primary()
        } else {
            Colors::border_subtle()
        })
        .px(px(8.0))
        .py(px(6.0))
        .flex_col()
        .justify_between()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_color(Colors::text_primary())
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(name),
                )
                .child(div().w(px(8.0)).h(px(8.0)).rounded_full().bg(if active {
                    Colors::accent_primary()
                } else {
                    Colors::text_muted()
                })),
        )
        .child(
            div()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_color(Colors::text_muted())
                        .text_size(px(8.5))
                        .child("Mix: 80%"),
                )
                .child(
                    div()
                        .w_full()
                        .h(px(3.0))
                        .bg(Colors::surface_input())
                        .rounded_full()
                        .relative()
                        .child(
                            div()
                                .absolute()
                                .left(px(0.0))
                                .w(px(80.0))
                                .h_full()
                                .bg(Colors::accent_primary())
                                .rounded_full(),
                        ),
                ),
        )
}

fn empty_effect_slot() -> impl IntoElement {
    div()
        .w(px(130.0))
        .h(px(80.0))
        .rounded_md()
        .border(px(1.0))
        .border_color(Colors::panel_border())
        .border_dashed()
        .flex()
        .items_center()
        .justify_center()
        .text_color(Colors::text_muted())
        .text_size(px(10.0))
        .child("+ Add Effect")
}
