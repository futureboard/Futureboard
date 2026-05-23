use gpui::{div, px, svg, IntoElement, ParentElement, Styled, InteractiveElement, StatefulInteractiveElement};
use crate::components::timeline::timeline_state::TimelineTool;
use crate::assets;

pub fn floating_tools_bar(
    active_tool: TimelineTool,
    on_select_tool: std::sync::Arc<dyn Fn(&TimelineTool, &mut gpui::Window, &mut gpui::App) + 'static>,
) -> impl IntoElement {
    let tools = [
        (TimelineTool::Pointer, "Select [V]", assets::ICON_MOUSE_POINTER_PATH, true),
        (TimelineTool::Pen, "Draw [P]", assets::ICON_PENCIL_PATH, false),
        (TimelineTool::Cut, "Cut [C]", assets::ICON_SCISSORS_PATH, false),
        (TimelineTool::Glue, "Glue [G]", assets::ICON_LINK_PATH, true),
        (TimelineTool::Mute, "Mute [U]", assets::ICON_VOLUME_X_PATH, false),
        (TimelineTool::Time, "Stretch [T]", assets::ICON_CLOCK_PATH, false),
        (TimelineTool::Automation, "Automation [A]", assets::ICON_AUTOMATION_PATH, false),
    ];

    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(1.0))
        .rounded_lg()
        .border(px(1.0))
        .border_color(gpui::rgba(0xFFFFFF1A))
        .bg(gpui::rgb(0x171b22))
        .px(px(4.0))
        .py(px(4.0))
        .shadow_xl()
        .children(tools.into_iter().enumerate().map(move |(i, (tool, label, icon, separator))| {
            let active = active_tool == tool;
            let on_select = on_select_tool.clone();
            
            div()
                .flex()
                .flex_row()
                .items_center()
                .child(
                    div()
                        .relative()
                        .flex()
                        .items_center()
                        .justify_center()
                        .w(px(28.0))
                        .h(px(28.0))
                        .rounded_md()
                        .id(label)
                        .cursor(gpui::CursorStyle::PointingHand)
                        .text_size(px(14.0))
                        .text_color(if active { gpui::rgb(0x56c7c9) } else { gpui::rgba(0xB4C0CC8C) })
                        .bg(if active { gpui::rgba(0x56c7c926) } else { gpui::rgba(0x00000000) })
                        .hover(|style| if !active { style.bg(gpui::rgba(0xFFFFFF0D)) } else { style })
                        .on_click(move |_, window, cx| {
                            on_select(&tool, window, cx);
                        })
                        .child(
                            svg()
                                .path(icon)
                                .w(px(14.0))
                                .h(px(14.0))
                                .text_color(if active { gpui::rgb(0x56c7c9) } else { gpui::rgba(0xB4C0CC8C) })
                        )
                        .children(
                            if active {
                                Some(
                                    div()
                                        .absolute()
                                        .bottom(px(1.0))
                                        .h(px(1.5))
                                        .w(px(16.0))
                                        .rounded_full()
                                        .bg(gpui::rgb(0x56c7c9))
                                )
                            } else {
                                None
                            }
                        )
                )
                .children(
                    if separator && i < 6 {
                        Some(
                            div()
                                .mx(px(4.0))
                                .h(px(16.0))
                                .w(px(1.0))
                                .bg(gpui::rgba(0xFFFFFF14))
                        )
                    } else {
                        None
                    }
                )
        }))
}
