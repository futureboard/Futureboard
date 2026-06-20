use gpui::CursorStyle;

use crate::components::timeline::timeline_state::TimelineTool;

pub fn arrow() -> CursorStyle {
    CursorStyle::FutureboardArrow
}

pub fn select() -> CursorStyle {
    CursorStyle::FutureboardSelect
}

pub fn marquee() -> CursorStyle {
    CursorStyle::FutureboardMarquee
}

pub fn move_clip() -> CursorStyle {
    CursorStyle::FutureboardMove
}

pub fn fade_in() -> CursorStyle {
    CursorStyle::FutureboardFadeIn
}

pub fn fade_out() -> CursorStyle {
    CursorStyle::FutureboardFadeOut
}

pub fn resize_horizon() -> CursorStyle {
    CursorStyle::FutureboardResizeHorizon
}

pub fn resize_left() -> CursorStyle {
    CursorStyle::FutureboardResizeLeft
}

pub fn resize_right() -> CursorStyle {
    CursorStyle::FutureboardResizeRight
}

pub fn timeline_tool(tool: TimelineTool) -> CursorStyle {
    match tool {
        TimelineTool::Pointer => select(),
        TimelineTool::Pen => marquee(),
        TimelineTool::Cut
        | TimelineTool::Glue
        | TimelineTool::Mute
        | TimelineTool::Time
        | TimelineTool::Automation => select(),
    }
}
