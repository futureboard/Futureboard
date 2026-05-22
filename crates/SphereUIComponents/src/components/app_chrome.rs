use gpui::{div, px, InteractiveElement, IntoElement, ParentElement, Styled, WindowControlArea};
use crate::theme::Colors;
use crate::assets;
use crate::components::icon_button;

const MENU_ITEMS: &[&str] = &[
    "File", "Edit", "MIDI", "Project", "Audio", "Automation", "Window", "Tools", "Help",
];

fn menu_area() -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .px(px(6.0))
        .children(MENU_ITEMS.iter().map(|label| {
            div()
                .px(px(7.0))
                .py(px(2.0))
                .rounded_md()
                .text_color(Colors::text_muted())
                .text_size(px(11.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(*label)
        }))
}

fn project_title() -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .px(px(8.0))
        .child(
            div()
                .text_color(Colors::text_secondary())
                .text_size(px(12.0))
                .font_weight(gpui::FontWeight::BOLD)
                .child("Untitled Project"),
        )
        .child(
            div()
                .text_color(Colors::text_muted())
                .text_xs()
                .child("·"),
        )
        .child(
            div()
                .text_color(Colors::text_muted())
                .text_xs()
                .child("Saved"),
        )
}

fn transport_controls() -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_px()
        .px(px(6.0))
        // Skip back
        .child(
            icon_button(
                Some(assets::ICON_SKIP_BACK_PATH),
                "⏮",
                px(24.0),
                px(24.0),
                px(12.0),
                Colors::text_secondary(),
            )
        )
        // Play
        .child(
            icon_button(
                Some(assets::ICON_PLAY_PATH),
                "▶",
                px(24.0),
                px(24.0),
                px(12.0),
                Colors::text_secondary(),
            )
        )
        // Stop
        .child(
            icon_button(
                Some(assets::ICON_SQUARE_PATH),
                "■",
                px(24.0),
                px(24.0),
                px(12.0),
                Colors::text_secondary(),
            )
        )
        // Record
        .child(
            icon_button(
                Some(assets::ICON_CIRCLE_PATH),
                "⏺",
                px(24.0),
                px(24.0),
                px(12.0),
                Colors::status_error(),
            )
        )
        // Divider
        .child(div().w(px(1.0)).h(px(16.0)).bg(Colors::border_subtle()).mx(px(4.0)))
        // Position display
        .child(
            div()
                .w(px(80.0))
                .flex() 
                .items_center()
                .justify_center()
                .text_color(Colors::text_primary())
                .text_xs()
                .child("1.1.1"),
        )
        // Divider
        .child(div().w(px(1.0)).h(px(16.0)).bg(Colors::border_subtle()).mx(px(4.0)))
        // BPM
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .text_color(Colors::text_muted())
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_xs()
                        .child("BPM"),
                )
                .child(
                    div()
                        .text_color(Colors::text_primary())
                        .text_xs()
                        .child("120"),
                ),
        )
        // Time sig
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .px(px(4.0))
                .text_color(Colors::text_secondary())
                .text_xs()
                .child("4/4"),
        )
}

fn utility_buttons() -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_px()
        .px(px(4.0))
        .child(
            icon_button(
                None,
                "⤓",
                px(24.0),
                px(24.0),
                px(12.0),
                Colors::text_muted(),
            )
        )
        .child(
            icon_button(
                Some(assets::ICON_PANEL_BOTTOM_PATH),
                "⊟",
                px(24.0),
                px(24.0),
                px(12.0),
                Colors::text_muted(),
            )
        )
}

fn window_controls(window: &gpui::Window) -> impl IntoElement {
    let is_maximized = window.is_maximized();
    let max_restore_icon = if is_maximized {
        assets::ICON_RESTORE_PATH
    } else {
        assets::ICON_MAXIMIZE_PATH
    };
    let max_restore_fallback = if is_maximized { "❐" } else { "□" };

    div()
        .flex()
        .flex_row()
        .items_center()
        .h_full()
        // Minimize
        .child(
            icon_button(
                Some(assets::ICON_MINIMIZE_PATH),
                "−",
                px(32.0),
                px(32.0),
                px(16.0),
                Colors::text_muted(),
            )
            .window_control_area(WindowControlArea::Min)
            .occlude(),
        )
        // Maximize / Restore
        .child(
            icon_button(
                Some(max_restore_icon),
                max_restore_fallback,
                px(32.0),
                px(32.0),
                px(16.0),
                Colors::text_muted(),
            )
            .window_control_area(WindowControlArea::Max)
            .occlude(),
        )
        // Close
        .child(
            icon_button(
                Some(assets::ICON_X_PATH),
                "×",
                px(32.0),
                px(32.0),
                px(16.0),
                Colors::text_muted(),
            )
            .window_control_area(WindowControlArea::Close)
            .occlude(),
        )
}

pub fn app_chrome(window: &gpui::Window) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .h(px(36.0))
        .w_full()
        .bg(Colors::surface_panel())
        // Entire chrome is draggable except where overridden by control areas
        .window_control_area(WindowControlArea::Drag)
        // Left: menus
        .child(menu_area())
        // Divider
        .child(div().w(px(1.0)).h(px(16.0)).bg(Colors::border_subtle()).mx(px(2.0)))
        // Project title
        .child(project_title())
        // Flexible spacer — also draggable (inherits parent Drag region)
        .child(div().flex_1())
        // Center: transport controls
        .child(transport_controls())
        // Flexible spacer
        .child(div().flex_1())
        // Utility panel toggle buttons
        .child(utility_buttons())
        // Divider
        .child(div().w(px(1.0)).h(px(16.0)).bg(Colors::border_subtle()).mx(px(2.0)))
        // Window controls (min / max / close) — override drag with specific hit areas
        .child(window_controls(window))
}
