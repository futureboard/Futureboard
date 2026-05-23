//! Native GPUI port of the WebUI top-menu dropdown panel.
//!
//! Source of truth for the visuals is `apps/web/src/components/TransportBar.tsx`'s
//! `MenuPanel`. Matching tokens:
//!
//! * panel background  = `daw-surface`
//! * panel border      = `daw-border`
//! * panel shadow      = `0 12px 36px rgba(0,0,0,0.52)`
//! * panel padding     = 4 px (Tailwind `p-1`)
//! * panel width       = 256 px (fixed, vs web `min-w-[13rem]`)
//! * item height       = 24 px (Tailwind `h-6`)
//! * item radius       = 4 px (Tailwind `rounded`)
//! * item text         = 11 px, `daw-text` (or `daw-red` when `danger`)
//! * item hover        = `daw-surface-high`
//! * item disabled     = ~35 % opacity
//! * shortcut          = 10 px, `daw-faint`, right aligned
//! * separator         = 1 px horizontal rule, 2 px vertical margin
//!
//! Submenus are click-to-toggle (the web version uses hover, but GPUI's
//! callback model makes click semantics simpler and click also matches what
//! every native DAW does). The open submenu path lives in `MenuBarUiState`
//! and threads through the same dropdown render.

use std::sync::Arc;

use gpui::{
    div, px, rgba, svg, App, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window,
};

use crate::assets;
use crate::menu::{Menu, MenuItem, MenuItemKind};
use crate::theme::Colors;

pub const MENU_PANEL_WIDTH: f32 = 256.0;
pub const TOP_CHROME_HEIGHT: f32 = 36.0;

/// Horizontal padding *inside* each row, matching the WebUI's `px-2`
/// (Tailwind 8 px) tightened slightly so the label sits closer to the
/// rounded panel edge without losing the hover highlight inset.
const ROW_PX: f32 = 6.0;
/// Pixel space reserved on the left of every row in a panel that contains
/// at least one checkbox item, so the check icon doesn't shift the label
/// of neighbouring non-checkbox items. Panels with no checkboxes don't
/// reserve this space at all.
const CHECK_SLOT_W: f32 = 14.0;
/// Y nudge applied to nested submenu panels so their top edge lines up
/// with the parent row instead of the row's text baseline.
const SUBMENU_OFFSET: f32 = -4.0;

/// Vertical room each item type occupies inside a panel. Matches the
/// `flex flex-col gap-px p-1` layout used by [`panel_view`].
const PANEL_PAD_TOP: f32 = 4.0;
const ROW_HEIGHT: f32 = 24.0;
const SEPARATOR_HEIGHT: f32 = 5.0; // my(2) + 1 px line + my(2)
const ITEM_GAP: f32 = 1.0;

pub type CommandCb = Arc<dyn Fn(&String, &mut Window, &mut App) + 'static>;
pub type CloseCb = Arc<dyn Fn(&(), &mut Window, &mut App) + 'static>;
/// `(depth, submenu_id)` — depth is the index in the open path that the
/// caller wants to toggle. Setting a fresh id at `depth` truncates the
/// path beyond `depth`; toggling the same id closes the submenu.
pub type ToggleSubmenuCb = Arc<dyn Fn(&(usize, String), &mut Window, &mut App) + 'static>;

/// Render the dropdown overlay rooted at `menu`, anchored under the clicked
/// top-level label. `submenu_path` is the current open path *below* the
/// top level — i.e. `submenu_path[0]` is the id of the submenu open in the
/// root panel, `submenu_path[1]` the id of the submenu open in *that*
/// panel, etc.
pub fn menu_dropdown(
    menu: &Menu,
    anchor_x: f32,
    viewport_width: f32,
    submenu_path: &[String],
    on_toggle_submenu: ToggleSubmenuCb,
    on_command: CommandCb,
    on_close: CloseCb,
) -> impl IntoElement {
    let root_top = TOP_CHROME_HEIGHT;
    let root_left = clamp_left(anchor_x, viewport_width);

    // Build the chain of panels: root, then for each open submenu in the
    // path, the nested panel positioned to the right of its parent.
    let mut panels: Vec<gpui::AnyElement> = Vec::new();
    let mut items_ref: &[MenuItem] = &menu.items;
    let mut panel_left = root_left;
    let mut panel_top = root_top;

    for depth in 0..=submenu_path.len() {
        panels.push(
            panel_view(
                depth,
                items_ref,
                panel_left,
                panel_top,
                submenu_path.get(depth).cloned(),
                on_toggle_submenu.clone(),
                on_command.clone(),
                on_close.clone(),
            )
            .into_any_element(),
        );

        // Walk into the next submenu level if the path requests it.
        let Some(next_id) = submenu_path.get(depth) else {
            break;
        };
        let Some(trigger_index) = items_ref
            .iter()
            .position(|it| it.kind == MenuItemKind::Submenu && &it.id == next_id)
        else {
            break;
        };
        let submenu = &items_ref[trigger_index];

        // Align the child panel's top edge with the trigger row's top in
        // the parent panel, then nudge by SUBMENU_OFFSET so the borders
        // visually overlap by ~1 row of breathing room.
        let trigger_y = trigger_top_offset(items_ref, trigger_index);
        panel_top += trigger_y + SUBMENU_OFFSET;
        panel_left = clamp_left(panel_left + MENU_PANEL_WIDTH + 2.0, viewport_width);
        items_ref = &submenu.children;
    }

    // Click-blocking backdrop behind every panel.
    let backdrop_close = on_close.clone();
    let backdrop = div()
        .absolute()
        .inset_0()
        .id("menu-dropdown-backdrop")
        .on_mouse_down(gpui::MouseButton::Left, move |_, w, cx| {
            backdrop_close(&(), w, cx);
        });

    div()
        .absolute()
        .inset_0()
        .child(backdrop)
        .children(panels)
}

fn clamp_left(left: f32, viewport_width: f32) -> f32 {
    let max_left = (viewport_width - MENU_PANEL_WIDTH - 8.0).max(0.0);
    left.clamp(0.0, max_left)
}

/// Y offset (in px) of the row at `trigger_index` relative to the panel's
/// top edge, accounting for panel padding, item gaps, and the smaller
/// height of separator rows.
fn trigger_top_offset(items: &[MenuItem], trigger_index: usize) -> f32 {
    let mut y = PANEL_PAD_TOP;
    for (i, item) in items.iter().enumerate() {
        if i == trigger_index {
            break;
        }
        if !item.visible {
            continue;
        }
        let h = match item.kind {
            MenuItemKind::Separator => SEPARATOR_HEIGHT,
            _ => ROW_HEIGHT,
        };
        y += h + ITEM_GAP;
    }
    y
}

fn panel_shadow() -> Vec<gpui::BoxShadow> {
    vec![gpui::BoxShadow {
        color: rgba(0x00000085_u32).into(),
        offset: gpui::point(px(0.0), px(12.0)),
        blur_radius: px(36.0),
        spread_radius: px(0.0),
    }]
}

fn panel_view(
    depth: usize,
    items: &[MenuItem],
    left: f32,
    top: f32,
    open_child_id: Option<String>,
    on_toggle_submenu: ToggleSubmenuCb,
    on_command: CommandCb,
    on_close: CloseCb,
) -> impl IntoElement {
    // Per-panel decision: only reserve a left check-slot if the panel
    // actually contains a checkbox item. That removes the empty gutter
    // from menus like File / Project that are pure command lists.
    let has_check = items
        .iter()
        .any(|it| it.kind == MenuItemKind::Checkbox);

    let mut item_els: Vec<gpui::AnyElement> = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        if !item.visible {
            continue;
        }
        match item.kind {
            MenuItemKind::Separator => item_els.push(separator().into_any_element()),
            _ => item_els.push(
                menu_item_row(
                    depth,
                    i,
                    item,
                    has_check,
                    open_child_id.as_deref(),
                    on_toggle_submenu.clone(),
                    on_command.clone(),
                    on_close.clone(),
                )
                .into_any_element(),
            ),
        }
    }

    div()
        .absolute()
        .top(px(top))
        .left(px(left))
        .w(px(MENU_PANEL_WIDTH))
        .max_h(px(560.0))
        .id(("menu-dropdown-panel", (depth + 1) * 1000 + left as usize))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap(px(1.0))
        .p(px(4.0))
        .rounded_md()
        .bg(Colors::surface_panel())
        .border(px(1.0))
        .border_color(Colors::border_subtle())
        .shadow(panel_shadow())
        // Soaks up clicks so they don't bubble to the backdrop.
        .occlude()
        .children(item_els)
}

fn separator() -> impl IntoElement {
    div()
        .my(px(2.0))
        .h(px(1.0))
        .bg(Colors::border_subtle())
}

fn menu_item_row(
    depth: usize,
    index: usize,
    item: &MenuItem,
    panel_has_check: bool,
    open_child_id: Option<&str>,
    on_toggle_submenu: ToggleSubmenuCb,
    on_command: CommandCb,
    on_close: CloseCb,
) -> impl IntoElement {
    let enabled = item.enabled;
    let label = item.label.clone().unwrap_or_default();
    let shortcut = item.shortcut.clone();
    let is_submenu = item.kind == MenuItemKind::Submenu;
    let is_checkbox = item.kind == MenuItemKind::Checkbox;
    let is_checked = is_checkbox && item.checked;
    let is_open_submenu = is_submenu && open_child_id == Some(item.id.as_str());
    let command = item.command.clone();
    let item_id = item.id.clone();

    let text_color = if !enabled {
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
    let on_toggle_click = on_toggle_submenu.clone();

    // ── Left cluster: optional check slot + label ───────────────────────
    let check_slot: Option<gpui::Div> = if panel_has_check {
        Some(
            div()
                .flex()
                .items_center()
                .justify_center()
                .w(px(CHECK_SLOT_W))
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
                }),
        )
    } else {
        None
    };

    let mut left = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .flex_1()
        .min_w(px(0.0));
    if let Some(slot) = check_slot {
        left = left.child(slot);
    }
    left = left.child(
        div()
            .flex_1()
            .min_w(px(0.0))
            .text_size(px(11.0))
            .text_color(text_color)
            .child(label),
    );

    // ── Right cluster: shortcut / chevron ────────────────────────────────
    let mut right = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .flex_none()
        .pl(px(12.0));
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

    // ── Row container ────────────────────────────────────────────────────
    let mut row = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .h(px(24.0))
        .w_full()
        .px(px(ROW_PX))
        .rounded_sm()
        .text_size(px(11.0))
        .id(("menu-item", depth * 10_000 + index))
        .child(left)
        .child(right);

    if is_open_submenu {
        row = row.bg(Colors::surface_hover());
    }

    if enabled {
        row = row
            .cursor(gpui::CursorStyle::PointingHand)
            .hover(|s| s.bg(Colors::surface_hover()));
        if is_submenu {
            // Click toggles this submenu open/closed at `depth`.
            row = row.on_click(move |_, w, cx| {
                on_toggle_click(&(depth, item_id.clone()), w, cx);
            });
        } else {
            row = row.on_click(move |_, w, cx| {
                if let Some(cmd) = command.as_ref() {
                    on_command(cmd, w, cx);
                }
                on_close_click(&(), w, cx);
            });
        }
    }

    row
}
