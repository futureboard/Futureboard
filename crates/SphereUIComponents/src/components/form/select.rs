use std::sync::Arc;

use gpui::prelude::FluentBuilder;
use gpui::{
    div, px, svg, App, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement,
    Styled, Window,
};

use crate::assets;
use crate::theme::Colors;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectOption {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
    pub disabled: bool,
}

impl SelectOption {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            description: None,
            disabled: false,
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

pub fn select(
    id: &'static str,
    selected_id: Option<&str>,
    placeholder: impl Into<String>,
    options: Vec<SelectOption>,
    open: bool,
    disabled: bool,
    on_toggle: Arc<dyn Fn(&(), &mut Window, &mut App) + 'static>,
    on_change: Arc<dyn Fn(&String, &mut Window, &mut App) + 'static>,
) -> impl IntoElement {
    let placeholder = placeholder.into();
    let selected_label = selected_id
        .and_then(|selected| options.iter().find(|option| option.id == selected))
        .map(|option| option.label.clone());
    let label = selected_label.unwrap_or(placeholder);
    let toggle = on_toggle.clone();

    div()
        .relative()
        .w_full()
        .child(
            div()
                .id(id)
                .h(px(28.0))
                .w_full()
                .min_w(px(0.0))
                .rounded_md()
                .border(px(1.0))
                .border_color(if open {
                    Colors::border_focus()
                } else {
                    Colors::border_subtle()
                })
                .bg(if open {
                    Colors::surface_card()
                } else {
                    Colors::surface_input()
                })
                .px(px(8.0))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .opacity(if disabled { 0.48 } else { 1.0 })
                .cursor(if disabled {
                    gpui::CursorStyle::Arrow
                } else {
                    gpui::CursorStyle::PointingHand
                })
                .hover(|s| {
                    if disabled {
                        s
                    } else {
                        s.bg(Colors::surface_control_hover())
                            .border_color(Colors::border_strong())
                    }
                })
                .on_mouse_down(gpui::MouseButton::Left, move |_event, window, cx| {
                    cx.stop_propagation();
                    if !disabled {
                        toggle(&(), window, cx);
                    }
                    window.prevent_default();
                })
                .child(
                    div()
                        .min_w(px(0.0))
                        .flex_1()
                        .truncate()
                        .text_size(px(11.0))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(Colors::text_primary())
                        .child(label),
                )
                .child(
                    svg()
                        .path(assets::ICON_CHEVRON_DOWN_PATH)
                        .w(px(10.0))
                        .h(px(10.0))
                        .flex_shrink_0()
                        .text_color(Colors::text_faint()),
                ),
        )
        .when(open && !disabled, move |root| {
            root.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(31.0))
                    .max_h(px(180.0))
                    .rounded_md()
                    .border(px(1.0))
                    .border_color(Colors::border_subtle())
                    .bg(Colors::surface_card())
                    .shadow(vec![gpui::BoxShadow {
                        color: Colors::surface_overlay().into(),
                        offset: gpui::point(px(0.0), px(8.0)),
                        blur_radius: px(20.0),
                        spread_radius: px(0.0),
                    }])
                    .p(px(4.0))
                    .id("select-menu")
                    .overflow_y_scroll()
                    .occlude()
                    .children(options.into_iter().enumerate().map(move |(index, option)| {
                        let active = selected_id == Some(option.id.as_str());
                        let disabled = option.disabled;
                        let value = option.id.clone();
                        let on_change = on_change.clone();
                        div()
                            .id(("select-option", index))
                            .min_h(px(if option.description.is_some() {
                                34.0
                            } else {
                                24.0
                            }))
                            .w_full()
                            .rounded_md()
                            .px(px(8.0))
                            .py(px(4.0))
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap(px(8.0))
                            .bg(if active {
                                Colors::accent_muted()
                            } else {
                                gpui::transparent_black().into()
                            })
                            .opacity(if disabled { 0.45 } else { 1.0 })
                            .cursor(if disabled {
                                gpui::CursorStyle::Arrow
                            } else {
                                gpui::CursorStyle::PointingHand
                            })
                            .hover(|s| {
                                if disabled {
                                    s
                                } else {
                                    s.bg(Colors::surface_control_hover())
                                }
                            })
                            .on_click(move |_, window, cx| {
                                if !disabled {
                                    on_change(&value, window, cx);
                                }
                            })
                            .child(
                                div()
                                    .min_w(px(0.0))
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap(px(1.0))
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(px(10.5))
                                            .font_weight(if active {
                                                gpui::FontWeight::SEMIBOLD
                                            } else {
                                                gpui::FontWeight::NORMAL
                                            })
                                            .text_color(if active {
                                                Colors::text_primary()
                                            } else {
                                                Colors::text_secondary()
                                            })
                                            .child(option.label),
                                    )
                                    .children(option.description.map(|description| {
                                        div()
                                            .truncate()
                                            .text_size(px(9.0))
                                            .text_color(Colors::text_faint())
                                            .child(description)
                                    })),
                            )
                            .children(active.then(|| {
                                svg()
                                    .path(assets::ICON_CHECK_PATH)
                                    .w(px(11.0))
                                    .h(px(11.0))
                                    .flex_shrink_0()
                                    .text_color(Colors::accent_primary())
                            }))
                    })),
            )
        })
}
