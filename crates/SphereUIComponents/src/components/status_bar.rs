use crate::components::title_bar::{status_item, STATUSBAR_HEIGHT};
use crate::components::{
    background_task_button, background_task_panel, BackgroundTaskCancelCb, BackgroundTaskStore,
    BackgroundTaskToggleCb,
};
use crate::theme::Colors;
use gpui::{div, px, IntoElement, ParentElement, Styled};

pub fn status_bar(left: impl Into<String>, right: impl Into<String>) -> impl IntoElement {
    status_bar_inner(left, right, None, None, None)
}

pub fn status_bar_with_background_tasks(
    left: impl Into<String>,
    right: impl Into<String>,
    tasks: &BackgroundTaskStore,
    on_toggle_tasks: BackgroundTaskToggleCb,
    on_cancel_task: BackgroundTaskCancelCb,
) -> impl IntoElement {
    status_bar_inner(
        left,
        right,
        Some(tasks),
        Some(on_toggle_tasks),
        Some(on_cancel_task),
    )
}

fn status_bar_inner(
    left: impl Into<String>,
    right: impl Into<String>,
    tasks: Option<&BackgroundTaskStore>,
    on_toggle_tasks: Option<BackgroundTaskToggleCb>,
    on_cancel_task: Option<BackgroundTaskCancelCb>,
) -> impl IntoElement {
    let left = left.into();
    let right = right.into();
    let task_button = match (tasks, on_toggle_tasks.clone()) {
        (Some(tasks), Some(on_toggle)) => {
            Some(background_task_button(tasks, on_toggle).into_any_element())
        }
        _ => None,
    };
    let task_panel = match (tasks, on_toggle_tasks, on_cancel_task) {
        (Some(tasks), Some(on_close), Some(on_cancel)) if tasks.panel_open => {
            Some(background_task_panel(tasks, on_close, on_cancel).into_any_element())
        }
        _ => None,
    };
    div()
        .relative()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .h(px(STATUSBAR_HEIGHT))
        .px(px(6.0))
        .gap(px(8.0))
        .bg(Colors::statusbar_bg())
        .border_t(px(1.0))
        .border_color(Colors::panel_border())
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .overflow_hidden()
                .child(status_item(left, true)),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .overflow_hidden()
                .children(task_button)
                .child(status_item(right, false)),
        )
        .children(task_panel)
}
