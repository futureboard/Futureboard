//! Shared anchor-based overlay positioning for dropdowns, menus, and popovers.

use gpui::{bounds, point, px, size, Bounds, MouseDownEvent, Pixels, Window};

use crate::components::title_bar::TITLEBAR_HEIGHT;

pub const OVERLAY_WINDOW_MARGIN: f32 = 8.0;
pub const COMBO_TRIGGER_HEIGHT: f32 = 30.0;
pub const MENU_LABEL_ESTIMATE_WIDTH: f32 = 72.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayPlacement {
    BottomStart,
    BottomEnd,
    TopStart,
    TopEnd,
    RightStart,
    LeftStart,
    Pointer,
}

#[derive(Debug, Clone, Copy)]
pub struct OverlayAnchor {
    pub bounds: Bounds<Pixels>,
}

impl Default for OverlayAnchor {
    fn default() -> Self {
        Self {
            bounds: bounds(point(px(0.0), px(0.0)), size(px(0.0), px(0.0))),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct OverlayPosition {
    pub x: Pixels,
    pub y: Pixels,
    pub width: Option<Pixels>,
    pub max_height: Option<Pixels>,
}

#[derive(Debug, Clone, Copy)]
pub struct OverlaySize {
    pub width: f32,
    pub height: f32,
}

/// Layout of the value column in settings form rows.
#[derive(Debug, Clone, Copy)]
pub struct FormColumnLayout {
    pub value_left: f32,
    pub value_width: f32,
}

pub fn settings_form_column(window: &Window) -> FormColumnLayout {
    const SIDEBAR: f32 = crate::components::settings_layout::SETTINGS_SIDEBAR_WIDTH;
    const CONTENT_PAD: f32 = crate::components::settings_layout::SETTINGS_CONTENT_PAD;
    const LABEL: f32 = crate::components::settings_layout::SETTINGS_LABEL_WIDTH;
    const GAP: f32 = crate::components::settings_layout::SETTINGS_ROW_GAP;
    let w: f32 = window.bounds().size.width.into();
    let left = SIDEBAR + CONTENT_PAD + LABEL + GAP;
    let width = (w - left - CONTENT_PAD).max(120.0);
    FormColumnLayout {
        value_left: left,
        value_width: width,
    }
}

/// Build trigger bounds from a form-row combo click using the current window layout.
pub fn form_combo_trigger_bounds(
    layout: FormColumnLayout,
    event: &MouseDownEvent,
    trigger_height: f32,
) -> Bounds<Pixels> {
    let click_y: f32 = event.position.y.into();
    let top = click_y - trigger_height * 0.5;
    bounds(
        point(px(layout.value_left), px(top)),
        size(px(layout.value_width), px(trigger_height)),
    )
}

/// Refresh horizontal geometry on resize while preserving vertical anchor.
pub fn refresh_form_anchor(anchor: OverlayAnchor, layout: FormColumnLayout) -> OverlayAnchor {
    let top: f32 = anchor.bounds.origin.y.into();
    let height: f32 = anchor.bounds.size.height.into();
    OverlayAnchor {
        bounds: bounds(
            point(px(layout.value_left), px(top)),
            size(px(layout.value_width), px(height.max(COMBO_TRIGGER_HEIGHT))),
        ),
    }
}

/// Anchor for a top-menu label (click x ≈ label origin).
pub fn titlebar_label_anchor(click_x: f32) -> OverlayAnchor {
    OverlayAnchor {
        bounds: bounds(
            point(px(click_x), px(0.0)),
            size(px(MENU_LABEL_ESTIMATE_WIDTH), px(TITLEBAR_HEIGHT)),
        ),
    }
}

/// Anchor for the project title button in the title bar.
pub fn project_title_anchor(left_x: f32) -> OverlayAnchor {
    OverlayAnchor {
        bounds: bounds(
            point(px(left_x), px(0.0)),
            size(px(288.0), px(TITLEBAR_HEIGHT)),
        ),
    }
}

/// Anchor at pointer position for context menus.
pub fn pointer_anchor(x: f32, y: f32) -> OverlayAnchor {
    OverlayAnchor {
        bounds: bounds(point(px(x), px(y)), size(px(0.0), px(0.0))),
    }
}

/// Width of the Inspector value column (right-aligned panel).
pub fn inspector_value_width(inspector_width: f32) -> f32 {
    const PAD: f32 = 10.0;
    const LABEL: f32 = 86.0;
    const GAP: f32 = 10.0;
    (inspector_width - PAD * 2.0 - LABEL - GAP).max(80.0)
}

/// Build trigger bounds for an Inspector form-row ComboBox from a click on the
/// trigger. The menu width matches the value column, not the pointer.
pub fn inspector_combo_trigger_bounds(
    window: &Window,
    inspector_width: f32,
    event: &MouseDownEvent,
) -> Bounds<Pixels> {
    const PAD: f32 = 10.0;
    const LABEL: f32 = 86.0;
    const GAP: f32 = 10.0;
    let value_w = inspector_value_width(inspector_width);
    let win_w: f32 = window.bounds().size.width.into();
    let value_left = win_w - inspector_width + PAD + LABEL + GAP;
    let click_y: f32 = event.position.y.into();
    let top = click_y - COMBO_TRIGGER_HEIGHT * 0.5;
    bounds(
        point(px(value_left), px(top)),
        size(px(value_w), px(COMBO_TRIGGER_HEIGHT)),
    )
}

pub fn inspector_combo_menu_position(
    anchor: OverlayAnchor,
    inspector_width: f32,
    menu_height: f32,
    window: &Window,
) -> OverlayPosition {
    let width = inspector_value_width(inspector_width);
    let refreshed = OverlayAnchor {
        bounds: bounds(
            anchor.bounds.origin,
            size(px(width), anchor.bounds.size.height),
        ),
    };
    compute_overlay_position(
        refreshed.bounds,
        OverlaySize {
            width,
            height: menu_height,
        },
        window.bounds(),
        OverlayPlacement::BottomStart,
        4.0,
    )
}

pub fn window_content_bounds(window: &Window) -> Bounds<Pixels> {
    window.bounds()
}

/// Drawable client area below the external dialog title bar (window-local coordinates).
pub fn external_dialog_overlay_bounds(window: &Window) -> Bounds<Pixels> {
    let full = window.bounds();
    let titlebar = TITLEBAR_HEIGHT;
    let full_w: f32 = full.size.width.into();
    let full_h: f32 = full.size.height.into();
    let content_h = (full_h - titlebar).max(0.0);
    bounds(
        point(px(0.0), px(titlebar)),
        size(px(full_w), px(content_h)),
    )
}

/// True when `anchor` lies inside the overlay coordinate space for this window.
pub fn anchor_visible_in_window(anchor: OverlayAnchor, window: &Window) -> bool {
    let content = external_dialog_overlay_bounds(window);
    let anchor_left: f32 = anchor.bounds.origin.x.into();
    let anchor_top: f32 = anchor.bounds.origin.y.into();
    let anchor_w: f32 = anchor.bounds.size.width.into();
    let anchor_h: f32 = anchor.bounds.size.height.into();
    let anchor_right = anchor_left + anchor_w;
    let anchor_bottom = anchor_top + anchor_h;
    let content_left: f32 = content.origin.x.into();
    let content_top: f32 = content.origin.y.into();
    let content_right = content_left + f32::from(content.size.width);
    let content_bottom = content_top + f32::from(content.size.height);
    anchor_left >= content_left - 2.0
        && anchor_top >= content_top - 2.0
        && anchor_right <= content_right + 2.0
        && anchor_bottom <= content_bottom + 2.0
}

pub fn compute_overlay_position(
    anchor: Bounds<Pixels>,
    overlay_size: OverlaySize,
    window_bounds: Bounds<Pixels>,
    placement: OverlayPlacement,
    margin: f32,
) -> OverlayPosition {
    let win_w: f32 = window_bounds.size.width.into();
    let win_h: f32 = window_bounds.size.height.into();
    let anchor_left: f32 = anchor.origin.x.into();
    let anchor_top: f32 = anchor.origin.y.into();
    let anchor_w: f32 = f32::from(anchor.size.width);
    let anchor_h: f32 = f32::from(anchor.size.height);
    let anchor_bottom = anchor_top + anchor_h;
    let anchor_right = anchor_left + anchor_w;

    let width = overlay_size.width.max(anchor_w);
    let mut height = overlay_size.height;

    let (mut x, mut y, mut flipped) = match placement {
        OverlayPlacement::BottomStart => (anchor_left, anchor_bottom + margin, false),
        OverlayPlacement::BottomEnd => (anchor_right - width, anchor_bottom + margin, false),
        OverlayPlacement::TopStart => (anchor_left, anchor_top - height - margin, false),
        OverlayPlacement::TopEnd => (anchor_right - width, anchor_top - height - margin, false),
        OverlayPlacement::RightStart => (anchor_right + margin, anchor_top, false),
        OverlayPlacement::LeftStart => (anchor_left - width - margin, anchor_top, false),
        OverlayPlacement::Pointer => {
            let px_pos: f32 = anchor.origin.x.into();
            let py_pos: f32 = anchor.origin.y.into();
            (px_pos, py_pos, false)
        }
    };

    if matches!(
        placement,
        OverlayPlacement::BottomStart | OverlayPlacement::BottomEnd
    ) && y + height + OVERLAY_WINDOW_MARGIN > win_h
    {
        let top_y = anchor_top - height - margin;
        if top_y >= OVERLAY_WINDOW_MARGIN {
            y = top_y;
            flipped = true;
            overlay_debug(&format!(
                "flip bottom->top anchor=({anchor_left:.0},{anchor_top:.0})"
            ));
        }
    }

    if x + width + OVERLAY_WINDOW_MARGIN > win_w {
        x = (win_w - width - OVERLAY_WINDOW_MARGIN).max(OVERLAY_WINDOW_MARGIN);
        overlay_debug(&format!("shift left x={x:.0} win_w={win_w:.0}"));
    }
    if x < OVERLAY_WINDOW_MARGIN {
        x = OVERLAY_WINDOW_MARGIN;
    }
    if y < OVERLAY_WINDOW_MARGIN {
        y = OVERLAY_WINDOW_MARGIN;
    }

    let available_below = (win_h - OVERLAY_WINDOW_MARGIN - y).max(0.0);
    let available_above = (anchor_top - OVERLAY_WINDOW_MARGIN).max(0.0);
    let max_height = if flipped {
        available_above.min(height)
    } else {
        available_below.min(height)
    };
    height = max_height.max(0.0);

    if y + height + OVERLAY_WINDOW_MARGIN > win_h {
        y = (win_h - height - OVERLAY_WINDOW_MARGIN).max(OVERLAY_WINDOW_MARGIN);
    }

    overlay_debug(&format!(
        "type=compute platform={} placement={placement:?} anchor=({anchor_left:.0},{anchor_top:.0},{anchor_w:.0},{anchor_h:.0}) pos=({x:.0},{y:.0}) size=({width:.0},{height:.0}) window=({win_w:.0},{win_h:.0}) flip={flipped}",
        overlay_platform()
    ));

    OverlayPosition {
        x: px(x),
        y: px(y),
        width: Some(px(width)),
        max_height: Some(px(height.max(0.0))),
    }
}

fn overlay_debug(message: &str) {
    if std::env::var_os("FUTUREBOARD_OVERLAY_DEBUG").is_some()
        || std::env::var_os("FUTUREBOARD_COMBOBOX_DEBUG").is_some()
    {
        eprintln!("[overlay] {message}");
    }
}

#[cfg(target_os = "windows")]
fn overlay_platform() -> &'static str {
    "windows"
}

#[cfg(not(target_os = "windows"))]
fn overlay_platform() -> &'static str {
    "other"
}
