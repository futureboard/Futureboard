use gpui::prelude::FluentBuilder;
use gpui::{
    div, px, App, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement,
    Styled, Window,
};

use crate::components::combo_box::combo_box_trigger;
use crate::components::controls::fb_checkbox;
use crate::theme::Colors;

#[derive(Clone, Copy)]
pub struct InspectorSelectOption<T: Copy + PartialEq + 'static> {
    pub label: &'static str,
    pub value: T,
}

pub fn inspector_section(
    title: impl Into<String>,
    subtitle: Option<impl Into<String>>,
    children: impl IntoElement,
) -> impl IntoElement {
    let title = title.into();
    let subtitle = subtitle.map(Into::into);
    div()
        .flex()
        .flex_col()
        .gap(px(5.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(1.0))
                .child(
                    div()
                        .h(px(18.0))
                        .flex()
                        .items_center()
                        .text_size(px(9.5))
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(Colors::text_faint())
                        .child(title),
                )
                .children(subtitle.map(|text| {
                    div()
                        .min_w(px(0.0))
                        .text_size(px(10.0))
                        .text_color(Colors::text_faint())
                        .child(text)
                })),
        )
        .child(div().flex().flex_col().gap(px(3.0)).child(children))
}

pub fn inspector_row(
    label: impl Into<String>,
    disabled: bool,
    control: impl IntoElement,
) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .min_h(px(24.0))
        .opacity(if disabled { 0.48 } else { 1.0 })
        .child(
            div()
                .w(px(106.0))
                .flex_shrink_0()
                .truncate()
                .text_size(px(10.5))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(Colors::text_muted())
                .child(label.into()),
        )
        .child(div().flex_1().min_w_0().child(control))
}

pub fn inspector_value(text: impl Into<String>) -> impl IntoElement {
    div()
        .min_w(px(0.0))
        .h(px(24.0))
        .flex()
        .items_center()
        .justify_end()
        .truncate()
        .text_size(px(11.0))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(Colors::text_secondary())
        .child(text.into())
}

pub fn inspector_select<T: Copy + PartialEq + 'static>(
    id: impl Into<gpui::ElementId>,
    selected: T,
    options: &'static [InspectorSelectOption<T>],
    disabled: bool,
    on_change: impl Fn(T, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let selected_index = options
        .iter()
        .position(|option| option.value == selected)
        .unwrap_or(0);
    let label = options
        .get(selected_index)
        .map(|option| option.label)
        .unwrap_or("-");
    let next_value = options
        .get((selected_index + 1).min(options.len().saturating_sub(1)))
        .map(|option| option.value)
        .unwrap_or(selected);
    div()
        .h(px(24.0))
        .when(disabled, |this| this.opacity(0.48))
        .child(combo_box_trigger(id, label, false, move |_, window, cx| {
            if !disabled && !options.is_empty() {
                let next = if selected_index + 1 >= options.len() {
                    options[0].value
                } else {
                    next_value
                };
                on_change(next, window, cx);
            }
        }))
}

pub fn inspector_checkbox(
    id: impl Into<gpui::ElementId>,
    checked: bool,
    disabled: bool,
    label: impl Into<String>,
    on_change: impl Fn(bool, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    fb_checkbox(id, label, checked, !disabled, move |_, window, cx| {
        if !disabled {
            on_change(!checked, window, cx);
        }
    })
}

pub fn inspector_numeric_stepper(
    id: &'static str,
    value: f64,
    display: impl Into<String>,
    min: f64,
    max: f64,
    step: f64,
    disabled: bool,
    on_change: impl Fn(f64, &mut Window, &mut App) + Clone + 'static,
) -> impl IntoElement {
    let down_enabled = !disabled && value > min + f64::EPSILON;
    let up_enabled = !disabled && value < max - f64::EPSILON;
    let on_down = on_change.clone();
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_end()
        .gap(px(4.0))
        .opacity(if disabled { 0.48 } else { 1.0 })
        .child(
            div()
                .w(px(84.0))
                .h(px(24.0))
                .flex()
                .items_center()
                .justify_end()
                .rounded_md()
                .border(px(1.0))
                .border_color(Colors::border_subtle())
                .bg(Colors::surface_input())
                .px(px(7.0))
                .text_size(px(11.0))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(Colors::text_primary())
                .child(display.into()),
        )
        .child(inspector_mini_button(
            (id, 0usize),
            "-",
            down_enabled,
            move |_, window, cx| on_down((value - step).clamp(min, max), window, cx),
        ))
        .child(inspector_mini_button(
            (id, 1usize),
            "+",
            up_enabled,
            move |_, window, cx| on_change((value + step).clamp(min, max), window, cx),
        ))
}

pub fn inspector_mini_button(
    id: impl Into<gpui::ElementId>,
    label: impl Into<String>,
    enabled: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let mut button = div()
        .id(id)
        .h(px(24.0))
        .min_w(px(26.0))
        .px(px(7.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .border(px(1.0))
        .border_color(Colors::border_subtle())
        .bg(Colors::surface_input())
        .opacity(if enabled { 1.0 } else { 0.45 })
        .text_size(px(10.5))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(Colors::text_secondary())
        .child(label.into());
    if enabled {
        button = button
            .cursor(gpui::CursorStyle::PointingHand)
            .hover(|s| {
                s.bg(Colors::surface_control_hover())
                    .border_color(Colors::border_strong())
            })
            .on_click(on_click);
    }
    button
}

pub fn inspector_hint_text(text: impl Into<String>) -> impl IntoElement {
    div()
        .min_w(px(0.0))
        .pt(px(1.0))
        .text_size(px(10.0))
        .text_color(Colors::text_faint())
        .child(text.into())
}
