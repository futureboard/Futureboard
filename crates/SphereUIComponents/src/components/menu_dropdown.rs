//! Native GPUI port of the WebUI top-menu dropdown panel.
//!
//! Source of truth for the visuals is `apps/web/src/components/TransportBar.tsx`'s
//! `MenuPanel`. Matching tokens:
//!
//! * panel background  = `daw-surface`
//! * panel border      = `daw-border`
//! * panel shadow      = `0 12px 36px rgba(0,0,0,0.52)`
//! * panel padding     = 4 px (Tailwind `p-1`)
//! * panel min-width   = 13 rem ≈ 208 px
//! * item height       = 24 px (Tailwind `h-6`)
//! * item radius       = 4 px (Tailwind `rounded`)
//! * item text         = 11 px, `daw-text` (or `daw-red` when `danger`)
//! * item hover        = `daw-surface-high`
//! * item disabled     = ~35 % opacity
//! * shortcut          = 10 px, `daw-faint`, right aligned
//! * separator         = 1 px horizontal rule, 2 px vertical margin
//!
//! Submenu support emits a chevron indicator and renders nested children when
//! the user hovers/clicks; for this pass we render only the first level — the
//! chevron is still present so the affordance is visible.

use std::sync::Arc;

use gpui::{
    div, px, rgba, svg, App, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window,
};

use crate::assets;
use crate::menu::{Menu, MenuItem, MenuItemKind};
use crate::theme::Colors;

/// Fixed panel width. The WebUI uses `min-w-[13rem]` and grows from there;
/// GPUI's flex resolver doesn't make rows stretch to a min-width container,
/// so we lock the panel to a single width and let long labels truncate.
/// 256 px comfortably fits every label + shortcut in the shared manifest.
pub const MENU_PANEL_WIDTH: f32 = 256.0;
pub const TOP_CHROME_HEIGHT: f32 = 36.0;

pub type CommandCb = Arc<dyn Fn(&String, &mut Window, &mut App) + 'static>;
pub type CloseCb = Arc<dyn Fn(&(), &mut Window, &mut App) + 'static>;

/// Render the dropdown panel for `menu`, anchored at content-space x =
/// `anchor_x`, top = chrome-height. The caller is responsible for
/// positioning this element absolutely inside the studio root.
pub fn menu_dropdown(
    menu: &Menu,
    anchor_x: f32,
    viewport_width: f32,
    on_command: CommandCb,
    on_close: CloseCb,
) -> impl IntoElement {
    let mut panel_left = anchor_x;
    // Clamp so the panel never overflows the right edge.
    let max_left = (viewport_width - MENU_PANEL_WIDTH - 8.0).max(0.0);
    if panel_left > max_left {
        panel_left = max_left;
    }

    let mut items_els: Vec<gpui::AnyElement> = Vec::with_capacity(menu.items.len());
    for (i, item) in menu.items.iter().enumerate() {
        if !item.visible {
            continue;
        }
        match item.kind {
            MenuItemKind::Separator => items_els.push(separator().into_any_element()),
            _ => items_els.push(
                menu_item_row(i, item, on_command.clone(), on_close.clone())
                    .into_any_element(),
            ),
        }
    }

    // Click-blocking backdrop behind the panel so clicking outside closes
    // the menu. Transparent and covers the full window.
    let backdrop_close = on_close.clone();
    let backdrop = div()
        .absolute()
        .inset_0()
        .id("menu-dropdown-backdrop")
        .on_mouse_down(gpui::MouseButton::Left, move |_, w, cx| {
            backdrop_close(&(), w, cx);
        });

    // Web shadow: `0 12px 36px rgba(0,0,0,0.52)`. GPUI takes a list of
    // shadow specs; this matches that single, deep, soft drop-shadow.
    let panel_shadow = vec![gpui::BoxShadow {
        color: rgba(0x00000085_u32).into(),
        offset: gpui::point(px(0.0), px(12.0)),
        blur_radius: px(36.0),
        spread_radius: px(0.0),
    }];

    let panel = div()
        .absolute()
        .top(px(TOP_CHROME_HEIGHT))
        .left(px(panel_left))
        .w(px(MENU_PANEL_WIDTH))
        .max_h(px(560.0))
        .id(("menu-dropdown-panel", panel_left as usize))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap(px(1.0))
        .p(px(4.0))
        .rounded_md()
        .bg(Colors::surface_panel())
        .border(px(1.0))
        .border_color(Colors::border_subtle())
        .shadow(panel_shadow)
        // Soaks up clicks so they don't bubble to the backdrop.
        .occlude()
        .children(items_els);

    div()
        .absolute()
        .inset_0()
        .child(backdrop)
        .child(panel)
}

fn separator() -> impl IntoElement {
    div()
        .my(px(2.0))
        .h(px(1.0))
        .bg(Colors::border_subtle())
}

fn menu_item_row(
    index: usize,
    item: &MenuItem,
    on_command: CommandCb,
    on_close: CloseCb,
) -> impl IntoElement {
    let enabled = item.enabled;
    let label = item.label.clone().unwrap_or_default();
    let shortcut = item.shortcut.clone();
    let is_submenu = item.kind == MenuItemKind::Submenu;
    let is_checked = item.kind == MenuItemKind::Checkbox && item.checked;
    let command = item.command.clone();

    let text_color = if !enabled {
        // ~35% of the regular text color, matching the web's disabled style.
        let mut c = Colors::text_secondary();
        c.a = 0.35;
        c
    } else if item.danger {
        Colors::status_error()
    } else {
        Colors::text_secondary()
    };
    let shortcut_color = {
        let mut c = Colors::text_faint();
        if !enabled {
            c.a = 0.35;
        }
        c
    };

    let on_close_click = on_close.clone();

    // Left cluster: check slot + label. Stretches to fill row width.
    let check_slot = div()
        .flex()
        .items_center()
        .justify_center()
        .w(px(14.0))
        .h(px(14.0))
        .flex_none()
        .child(if is_checked {
            svg()
                .path(assets::ICON_CHECK_PATH)
                .w(px(11.0))
                .h(px(11.0))
                .text_color(Colors::accent_primary())
                .into_any_element()
        } else {
            div().into_any_element()
        });

    let left = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .flex_1()
        .min_w(px(0.0))
        .child(check_slot)
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_color(text_color)
                .child(label),
        );

    // Right cluster: shortcut text and/or submenu chevron, justified to
    // the panel's right edge by the row's `justify_between`.
    let mut right = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .flex_none()
        .pl(px(16.0));
    if let Some(sc) = shortcut {
        right = right.child(
            div()
                .text_size(px(10.0))
                .text_color(shortcut_color)
                .child(sc),
        );
    }
    if is_submenu {
        right = right.child(
            svg()
                .path(assets::ICON_CHEVRON_RIGHT_PATH)
                .w(px(11.0))
                .h(px(11.0))
                .text_color(Colors::text_faint()),
        );
    }

    let mut row = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .h(px(24.0))
        .w_full()
        .px(px(8.0))
        .rounded_sm()
        .text_size(px(11.0))
        .id(("menu-item", index))
        .child(left)
        .child(right);

    if enabled {
        row = row
            .cursor(gpui::CursorStyle::PointingHand)
            .hover(|s| s.bg(Colors::surface_hover()));
        row = row.on_click(move |_, w, cx| {
            if let Some(cmd) = command.as_ref() {
                on_command(cmd, w, cx);
            }
            on_close_click(&(), w, cx);
        });
    }

    row
}
