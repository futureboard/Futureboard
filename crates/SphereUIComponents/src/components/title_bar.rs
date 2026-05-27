use gpui::{
    div, px, svg, App, Div, InteractiveElement, IntoElement, MouseButton, ParentElement, Rgba,
    StatefulInteractiveElement, Styled, Window, WindowControlArea,
};

use crate::assets;
use crate::platform_chrome::{PlatformChromePolicy, TITLEBAR_HEIGHT_PX};
use crate::theme::Colors;

pub const TITLEBAR_HEIGHT: f32 = TITLEBAR_HEIGHT_PX;
pub const STATUSBAR_HEIGHT: f32 = 22.0;
pub const CHROME_ICON_BUTTON_SIZE: f32 = 26.0;
pub const WINDOW_CONTROL_WIDTH: f32 = 34.0;
pub const CHROME_PAD_X: f32 = 6.0;
pub const CHROME_TEXT_SIZE: f32 = 10.5;
pub const CHROME_TITLE_SIZE: f32 = 11.5;

pub fn section_separator() -> impl gpui::IntoElement {
    div()
        .w(px(1.0))
        .h(px(18.0))
        .mx(px(3.0))
        .bg(Colors::panel_border())
}

pub fn chrome_button(
    icon_path: Option<&'static str>,
    fallback_text: &'static str,
    active: bool,
    color: Rgba,
) -> Div {
    let bg = if active {
        Colors::accent_muted()
    } else {
        gpui::transparent_black().into()
    };

    let mut button = div()
        .w(px(CHROME_ICON_BUTTON_SIZE))
        .h(px(CHROME_ICON_BUTTON_SIZE))
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .bg(bg)
        .text_color(color)
        .hover(|style| {
            style
                .bg(Colors::surface_control_hover())
                .text_color(Colors::text_primary())
        });

    if let Some(path) = icon_path {
        button = button.child(svg().path(path).w(px(13.0)).h(px(13.0)).text_color(color));
    } else {
        button = button.child(fallback_text);
    }

    button
}

pub fn window_control_button(
    area: WindowControlArea,
    icon_path: &'static str,
    fallback_text: &'static str,
) -> Div {
    chrome_button(Some(icon_path), fallback_text, false, Colors::text_muted())
        .w(px(WINDOW_CONTROL_WIDTH))
        .h(px(TITLEBAR_HEIGHT))
        .rounded_none()
        .window_control_area(area)
        .occlude()
}

pub fn draggable_spacer() -> Div {
    div()
        .flex_1()
        .h_full()
        .window_control_area(WindowControlArea::Drag)
        .on_mouse_down(gpui::MouseButton::Left, |_, window, _cx| {
            window.start_window_move();
        })
}

/// Compact title bar for external floating dialogs (Project Wizard, Preferences).
/// Drag on the bar; close button uses `.occlude()` so it is not swallowed by drag.
pub fn external_window_titlebar(
    title: impl Into<String>,
    close_id: impl Into<gpui::ElementId>,
    on_close: impl Fn(&mut Window, &mut App) + 'static + Clone,
) -> impl IntoElement {
    let policy = PlatformChromePolicy::external_dialog();
    let on_close = on_close.clone();
    let title_text = title.into();

    let mut bar = div()
        .window_control_area(WindowControlArea::Drag)
        .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
        .flex()
        .items_center()
        .justify_between()
        .h(px(policy.titlebar_height_px))
        .pl(policy.external_titlebar_left_padding())
        .pr(px(if policy.show_window_controls {
            0.0
        } else {
            CHROME_PAD_X
        }))
        .border_b(px(1.0))
        .border_color(Colors::border_subtle())
        .bg(Colors::surface_titlebar())
        .child(
            div()
                .flex()
                .items_center()
                .h_full()
                .text_size(px(CHROME_TITLE_SIZE))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(Colors::text_primary())
                .child(title_text),
        );

    if policy.show_window_controls {
        bar = bar.child(
            div()
                .window_control_area(WindowControlArea::Close)
                .id(close_id)
                .flex()
                .items_center()
                .justify_center()
                .w(px(TITLEBAR_HEIGHT))
                .h(px(TITLEBAR_HEIGHT))
                .cursor(gpui::CursorStyle::PointingHand)
                .hover(|s| s.bg(Colors::surface_control_hover()))
                .occlude()
                .on_click(move |_, window, cx| on_close(window, cx))
                .child(
                    svg()
                        .path(assets::ICON_X_PATH)
                        .w(px(12.0))
                        .h(px(12.0))
                        .text_color(Colors::text_faint()),
                ),
        );
    }

    bar
}

pub fn status_item(text: impl Into<String>, strong: bool) -> impl gpui::IntoElement {
    div()
        .h(px(18.0))
        .flex()
        .items_center()
        .px(px(6.0))
        .rounded_sm()
        .text_size(px(10.0))
        .font_weight(if strong {
            gpui::FontWeight::MEDIUM
        } else {
            gpui::FontWeight::NORMAL
        })
        .text_color(if strong {
            Colors::statusbar_text()
        } else {
            Colors::statusbar_text_muted()
        })
        .child(text.into())
}
