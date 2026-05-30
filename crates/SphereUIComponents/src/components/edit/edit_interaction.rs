//! Shared edit-tool semantics for arrangement and piano-roll surfaces.

use gpui::MouseButton;

/// High-level editing tools shared across editors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditTool {
    #[default]
    Select,
    Draw,
    Erase,
    RangeSelect,
    Move,
    Resize,
}

/// Resolved pointer intent after hit-testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerEditIntent {
    Create,
    Delete,
    Select,
    RangeSelect,
    Move,
    Resize,
}

/// Small drag threshold (px) before a press becomes a drag gesture.
pub const EDIT_DRAG_THRESHOLD_PX: f32 = 4.0;

/// Map mouse button + active tool to the primary edit intent on empty space.
pub fn pointer_intent_on_empty(tool: EditTool, button: MouseButton) -> PointerEditIntent {
    match button {
        MouseButton::Right => PointerEditIntent::Delete,
        MouseButton::Left => match tool {
            EditTool::Draw => PointerEditIntent::Create,
            EditTool::RangeSelect => PointerEditIntent::RangeSelect,
            EditTool::Select => PointerEditIntent::Select,
            EditTool::Erase => PointerEditIntent::Delete,
            EditTool::Move | EditTool::Resize => PointerEditIntent::Select,
        },
        _ => PointerEditIntent::Select,
    }
}

/// Normalize a 1-D range so `start <= end`.
#[inline]
pub fn normalize_range(start: f32, end: f32) -> (f32, f32) {
    if start <= end {
        (start, end)
    } else {
        (end, start)
    }
}

/// Axis-aligned rectangle intersection (left, top, right, bottom).
#[inline]
pub fn rects_intersect(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> bool {
    a.0 < b.2 && a.2 > b.0 && a.1 < b.3 && a.3 > b.1
}
