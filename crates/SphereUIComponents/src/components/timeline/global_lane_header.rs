use std::sync::Arc;

use gpui::{
    div, px, svg, AnyView, App, AppContext, Context, CursorStyle, ElementId, InteractiveElement,
    IntoElement, ParentElement, Render, StatefulInteractiveElement, Styled, Window,
};

use crate::assets;
use crate::components::timeline::timeline_state::HEADER_WIDTH;
use crate::theme::Colors;

const LANE_BTN: f32 = 20.0;
const LANE_ICON: f32 = 11.0;

pub type GlobalLaneVoidCb = Arc<dyn Fn(&(), &mut Window, &mut App) + 'static>;
pub type GlobalLaneMenuCb = Arc<dyn Fn(&(f32, f32), &mut Window, &mut App) + 'static>;

#[derive(Clone, Default)]
pub struct GlobalLaneHeaderActions {
    pub on_add: Option<GlobalLaneVoidCb>,
    pub on_menu: Option<GlobalLaneMenuCb>,
    pub on_hide: Option<GlobalLaneVoidCb>,
    pub on_toggle_collapsed: Option<GlobalLaneVoidCb>,
}

struct LaneTooltipText(&'static str);

impl Render for LaneTooltipText {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(8.0))
            .py(px(4.0))
            .rounded_sm()
            .bg(Colors::surface_raised())
            .border(px(1.0))
            .border_color(Colors::border_subtle())
            .text_size(px(10.0))
            .text_color(Colors::text_secondary())
            .child(self.0)
    }
}

fn tooltip_view(text: &'static str) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    move |_window, cx| cx.new(|_| LaneTooltipText(text)).into()
}

fn lane_icon_button(
    id: impl Into<ElementId>,
    icon: &'static str,
    tooltip: &'static str,
    accent: bool,
    on_mouse_down: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let color = if accent {
        Colors::accent_primary()
    } else {
        Colors::text_muted()
    };
    div()
        .id(id)
        .flex_shrink_0()
        .w(px(LANE_BTN))
        .h(px(LANE_BTN))
        .flex()
        .items_center()
        .justify_center()
        .rounded_sm()
        .border(px(1.0))
        .border_color(Colors::with_alpha(Colors::accent_primary(), if accent { 0.35 } else { 0.0 }))
        .bg(Colors::with_alpha(Colors::surface_raised(), 0.35))
        .cursor(CursorStyle::PointingHand)
        .hover(|s| {
            s.bg(Colors::with_alpha(Colors::accent_primary(), 0.12))
                .border_color(Colors::with_alpha(Colors::accent_primary(), 0.45))
        })
        .tooltip(tooltip_view(tooltip))
        .on_mouse_down(gpui::MouseButton::Left, move |event, window, cx| {
            cx.stop_propagation();
            on_mouse_down(event, window, cx);
        })
        .child(
            svg()
                .path(icon)
                .w(px(LANE_ICON))
                .h(px(LANE_ICON))
                .text_color(color),
        )
}

/// Compact conductor-lane header shared by Tempo and Time Signature tracks.
pub fn global_lane_header(
    lane_id: &'static str,
    title: &'static str,
    subtitle: String,
    collapsed: bool,
    hide_tooltip: &'static str,
    actions: GlobalLaneHeaderActions,
) -> impl IntoElement {
    let collapse_icon = if collapsed {
        assets::ICON_CHEVRON_RIGHT_PATH
    } else {
        assets::ICON_CHEVRON_DOWN_PATH
    };

    let mut action_row = div().flex().items_center().gap(px(2.0));

    if let Some(on_add) = actions.on_add {
        let add = on_add.clone();
        action_row = action_row.child(lane_icon_button(
            format!("global-lane-add-{lane_id}"),
            assets::ICON_PLUS_PATH,
            if lane_id == "tempo" {
                "Add tempo point at playhead"
            } else {
                "Add time signature marker at playhead"
            },
            true,
            move |_event, window, cx| add(&(), window, cx),
        ));
    }

    if let Some(on_menu) = actions.on_menu {
        action_row = action_row.child(lane_icon_button(
            format!("global-lane-menu-{lane_id}"),
            assets::ICON_MENU_PATH,
            "Lane menu",
            false,
            move |event, window, cx| {
                let x: f32 = event.position.x.into();
                let y: f32 = event.position.y.into();
                on_menu(&(x, y), window, cx);
            },
        ));
    }

    if let Some(on_toggle) = actions.on_toggle_collapsed {
        let toggle = on_toggle.clone();
        action_row = action_row.child(lane_icon_button(
            format!("global-lane-collapse-{lane_id}"),
            collapse_icon,
            if collapsed {
                "Expand lane"
            } else {
                "Collapse lane"
            },
            false,
            move |_event, window, cx| toggle(&(), window, cx),
        ));
    }

    if let Some(on_hide) = actions.on_hide {
        let hide = on_hide.clone();
        action_row = action_row.child(lane_icon_button(
            format!("global-lane-hide-{lane_id}"),
            assets::ICON_X_PATH,
            hide_tooltip,
            false,
            move |_event, window, cx| hide(&(), window, cx),
        ));
    }

    div()
        .flex_shrink_0()
        .w(px(HEADER_WIDTH))
        .h_full()
        .border_r(px(1.0))
        .border_color(Colors::border_subtle())
        .bg(Colors::with_alpha(Colors::accent_primary(), 0.05))
        .border_l(px(2.0))
        .border_color(Colors::with_alpha(Colors::accent_primary(), 0.4))
        .flex()
        .flex_col()
        .justify_center()
        .px(px(8.0))
        .gap(px(4.0))
        .child(
            div()
                .flex()
                .items_start()
                .justify_between()
                .gap(px(4.0))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .min_w_0()
                        .flex_1()
                        .child(
                            div()
                                .text_size(px(10.0))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(Colors::text_primary())
                                .child(title),
                        )
                        .child(
                            div()
                                .text_size(px(8.0))
                                .text_color(Colors::text_muted())
                                .overflow_hidden()
                                .child(subtitle),
                        ),
                )
                .child(action_row),
        )
}
