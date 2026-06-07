use gpui::{
    div, px, svg, App, Div, InteractiveElement, IntoElement, ParentElement, Rgba,
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
    // Active state is shown with a stroked/accent icon + a subtle 1px accent
    // ring — never a filled background (pro-DAW toolbar feel). A transparent
    // border on inactive buttons keeps the box size identical so toggling
    // active never shifts layout. The `color` argument already carries the
    // accent/status color when `active`, so the icon itself reads as accent.
    let border_color = if active {
        Colors::with_alpha(color, 0.45)
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
        .border_1()
        .border_color(border_color)
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
/// Drag uses an explicit flex spacer (same pattern as the main studio chrome) so
/// interactive children do not swallow window-move hit testing.
pub fn external_window_titlebar(
    title: impl Into<String>,
    close_id: impl Into<gpui::ElementId>,
    on_close: impl Fn(&mut Window, &mut App) + 'static + Clone,
) -> impl IntoElement {
    external_window_titlebar_with_icon(None, title, close_id, on_close)
}

/// Optional leading icon (e.g. Add Tracks dialog).
pub fn external_window_titlebar_with_icon(
    icon_path: Option<&'static str>,
    title: impl Into<String>,
    close_id: impl Into<gpui::ElementId>,
    on_close: impl Fn(&mut Window, &mut App) + 'static + Clone,
) -> impl IntoElement {
    let policy = PlatformChromePolicy::external_dialog();
    let on_close = on_close.clone();
    let title_text = title.into();

    let mut title_row = div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .h_full()
        .flex_shrink_0();
    if let Some(path) = icon_path {
        title_row = title_row.child(
            svg()
                .path(path)
                .w(px(13.0))
                .h(px(13.0))
                .text_color(Colors::accent_primary()),
        );
    }
    title_row = title_row.child(
        div()
            .text_size(px(CHROME_TITLE_SIZE))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(Colors::text_primary())
            .child(title_text),
    );

    let mut bar = div()
        .flex()
        .flex_row()
        .items_center()
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
        .child(title_row)
        .child(draggable_spacer());

    if policy.show_window_controls {
        bar = bar
            .child(external_window_control_button(
                WindowControlArea::Min,
                "external-window-minimize",
                assets::ICON_MINIMIZE_PATH,
                move |window, _cx| window.minimize_window(),
            ))
            .child(external_window_control_button(
                WindowControlArea::Max,
                "external-window-maximize",
                assets::ICON_MAXIMIZE_PATH,
                move |window, _cx| window.zoom_window(),
            ))
            .child(external_window_control_button(
                WindowControlArea::Close,
                close_id,
                assets::ICON_X_PATH,
                move |window, cx| on_close(window, cx),
            ));
    } else {
        bar = bar.child(
            div()
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

fn external_window_control_button(
    area: WindowControlArea,
    id: impl Into<gpui::ElementId>,
    icon_path: &'static str,
    on_click: impl Fn(&mut Window, &mut App) + 'static + Clone,
) -> impl IntoElement {
    div()
        .window_control_area(area)
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .w(px(TITLEBAR_HEIGHT))
        .h(px(TITLEBAR_HEIGHT))
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|s| s.bg(Colors::surface_control_hover()))
        .occlude()
        .on_click(move |_, window, cx| on_click(window, cx))
        .child(
            svg()
                .path(icon_path)
                .w(px(12.0))
                .h(px(12.0))
                .text_color(Colors::text_faint()),
        )
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
