use gpui::prelude::FluentBuilder;
use gpui::{
    div, px, App, AppContext, DragMoveEvent, Empty, InteractiveElement, IntoElement, ParentElement,
    Render, StatefulInteractiveElement, Styled, Window,
};

use crate::components::combo_box::combo_box_trigger;
use crate::components::controls::fb_checkbox;
use crate::theme::Colors;

#[derive(Clone, Copy)]
pub struct InspectorSelectOption<T: Copy + PartialEq + 'static> {
    pub label: &'static str,
    pub value: T,
}

/// Short-lived payload for an Inspector value scrub. The display owns the
/// gesture; the parent still owns the actual setting and undo transaction.
#[derive(Clone, Debug)]
struct InspectorNumericDrag {
    start_value: f64,
}

impl Render for InspectorNumericDrag {
    fn render(&mut self, _window: &mut Window, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        Empty
    }
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
    const SCRUB_PIXELS_PER_STEP: f32 = 5.0;
    let drag_start_y = std::sync::Arc::new(std::sync::Mutex::new(None::<f32>));
    let drag_start_y_move = drag_start_y.clone();
    let drag_change = on_change.clone();
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_end()
        .opacity(if disabled { 0.48 } else { 1.0 })
        .child({
            let display = display.into();
            let field = div()
                .id((id, 0usize))
                .w(px(108.0))
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
                .child(display);
            if disabled {
                field.into_any_element()
            } else {
                let start_y = drag_start_y.clone();
                field
                    .cursor(gpui::CursorStyle::ResizeUpDown)
                    .hover(|style| {
                        style
                            .bg(Colors::surface_control_hover())
                            .border_color(Colors::border_strong())
                    })
                    .on_drag(
                        InspectorNumericDrag { start_value: value },
                        move |drag, _offset, _window, cx| {
                            *start_y.lock().expect("inspector scrub mutex poisoned") = None;
                            cx.new(|_| InspectorNumericDrag {
                                start_value: drag.start_value,
                            })
                        },
                    )
                    .on_drag_move::<InspectorNumericDrag>(
                        move |event: &DragMoveEvent<InspectorNumericDrag>, window, cx| {
                            let current_y: f32 = event.event.position.y.into();
                            let mut anchor = drag_start_y_move
                                .lock()
                                .expect("inspector scrub mutex poisoned");
                            let start_y = *anchor.get_or_insert(current_y);
                            let steps =
                                ((start_y - current_y) / SCRUB_PIXELS_PER_STEP).round() as f64;
                            let next = (event.drag(cx).start_value + steps * step).clamp(min, max);
                            drop(anchor);
                            drag_change(next, window, cx);
                        },
                    )
                    .into_any_element()
            }
        })
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
