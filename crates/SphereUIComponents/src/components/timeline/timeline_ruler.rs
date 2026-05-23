use gpui::{div, px, svg, InteractiveElement, StatefulInteractiveElement, IntoElement, ParentElement, Styled, Window, App};
use crate::theme::Colors;
use crate::assets;
use crate::components::timeline::timeline_state::{TimelineState, GridLineLevel, HEADER_WIDTH, RULER_HEIGHT};

pub fn timeline_ruler(
    state: &TimelineState,
    on_add_track: std::sync::Arc<dyn Fn(&(), &mut gpui::Window, &mut gpui::App) + 'static>,
    on_toggle_snap: std::sync::Arc<dyn Fn(&(), &mut gpui::Window, &mut gpui::App) + 'static>,
    on_cycle_grid: std::sync::Arc<dyn Fn(&(), &mut gpui::Window, &mut gpui::App) + 'static>,
    on_seek: std::sync::Arc<dyn Fn(&f32, &mut gpui::Window, &mut gpui::App) + 'static>,
) -> impl IntoElement {
    let on_toggle_snap_clone = on_toggle_snap.clone();
    let on_cycle_grid_clone = on_cycle_grid.clone();
    let on_add_track_clone = on_add_track.clone();
    
    // Width of the ruler grid area (e.g. window size 1400 - sidebar 272 - header HEADER_WIDTH)
    let ruler_grid_width = 5000.0;
    let lines = state.get_arrangement_grid_lines(ruler_grid_width);
    
    let on_seek_clone = on_seek.clone();

    div()
        .flex()
        .flex_row()
        .h(px(RULER_HEIGHT))
        .w_full()
        .bg(Colors::surface_panel())
        .border_b(px(1.0))
        .border_color(Colors::border_subtle())
        .child(
            // Left Ruler Header Area
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .w(px(HEADER_WIDTH))
                .h_full()
                .px(px(8.0))
                .border_r(px(1.0))
                .border_color(Colors::border_subtle())
                .child(
                    div()
                        .text_color(Colors::text_primary())
                        .text_size(px(11.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child("Arrangement"),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(4.0))
                        // Add Track Button
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .h(px(20.0))
                                .px(px(5.0))
                                .rounded_md()
                                .bg(Colors::surface_raised())
                                .border(px(1.0))
                                .border_color(Colors::border_subtle())
                                .cursor(gpui::CursorStyle::PointingHand)
                                .text_color(Colors::text_secondary())
                                .text_size(px(10.0))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .id("ruler-add-track-btn")
                                .hover(|style| style.bg(Colors::surface_hover()))
                                .on_click(move |_, window, cx| {
                                    on_add_track_clone(&(), window, cx);
                                })
                                .child("+ Add"),
                        )
                        // Snap Toggle Button
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .h(px(20.0))
                                .w(px(20.0))
                                .rounded_md()
                                .bg(if state.snap_to_grid { Colors::accent_primary() } else { Colors::surface_raised() })
                                .border(px(1.0))
                                .border_color(Colors::border_subtle())
                                .cursor(gpui::CursorStyle::PointingHand)
                                .id("ruler-snap-toggle-btn")
                                .on_click(move |_, window, cx| {
                                    on_toggle_snap_clone(&(), window, cx);
                                })
                                .child(
                                    svg()
                                        .path(assets::ICON_MAGNET_PATH)
                                        .w(px(12.0))
                                        .h(px(12.0))
                                        .text_color(if state.snap_to_grid { gpui::rgb(0x101216) } else { Colors::text_secondary() })
                                ),
                        )
                        // Grid Resolution Button
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .h(px(20.0))
                                .px(px(4.0))
                                .rounded_md()
                                .bg(Colors::surface_raised())
                                .border(px(1.0))
                                .border_color(Colors::border_subtle())
                                .cursor(gpui::CursorStyle::PointingHand)
                                .text_color(Colors::text_muted())
                                .text_size(px(9.0))
                                .id("ruler-grid-res-btn")
                                .on_click(move |_, window, cx| {
                                    on_cycle_grid_clone(&(), window, cx);
                                })
                                .child(state.grid_division.label()),
                        ),
                ),
        )
        .child(
            // Right Ruler Markings Area
            div()
                .flex_1()
                .h_full()
                .relative()
                .cursor(gpui::CursorStyle::Crosshair)
                .id("ruler-markings-area")
                // Seek timeline position on click
                .on_mouse_down(gpui::MouseButton::Left, move |event: &gpui::MouseDownEvent, window, cx| {
                    // event.position.x is absolute screen position.
                    // Sidebar is 272px, Track Header is HEADER_WIDTH.
                    let x: f32 = event.position.x.into();
                    let click_x = x - 272.0 - HEADER_WIDTH;
                    on_seek_clone(&click_x, window, cx);
                })
                .children(
                    if state.transport.loop_enabled {
                        let lx = state.beats_to_x(state.transport.loop_start_beats);
                        let rx = state.beats_to_x(state.transport.loop_end_beats);
                        let width = (rx - lx).max(0.0);
                        Some(
                            div()
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .left(px(lx))
                                .w(px(width))
                                .bg(gpui::rgba(0x7bd88f14))
                                .border_l(px(1.0))
                                .border_r(px(1.0))
                                .border_color(gpui::rgb(0x7bd88f))
                        )
                    } else {
                        None
                    }
                )
                .children(lines.into_iter().map(|line| {
                    let tick_h = match line.level {
                        GridLineLevel::Bar => RULER_HEIGHT - 2.0,
                        GridLineLevel::Beat => RULER_HEIGHT * 0.46,
                        GridLineLevel::Sub => RULER_HEIGHT * 0.18,
                    };
                    
                    let tick_alpha = match line.level {
                        GridLineLevel::Bar => 0.28,
                        GridLineLevel::Beat => 0.18,
                        GridLineLevel::Sub => 0.10,
                    };

                    div()
                        .absolute()
                        .left(px(line.x))
                        .bottom_0()
                        .w(px(1.0))
                        .h(px(tick_h))
                        .bg(gpui::Rgba { r: 1.0, g: 1.0, b: 1.0, a: tick_alpha })
                        .children(
                            if line.show_label {
                                let label = state.format_bar_beat(line.beat);
                                let font_weight = match line.level {
                                    GridLineLevel::Bar => gpui::FontWeight::BOLD,
                                    _ => gpui::FontWeight::NORMAL,
                                };
                                let text_color = match line.level {
                                    GridLineLevel::Bar => Colors::text_secondary(),
                                    _ => Colors::text_muted(),
                                };
                                Some(
                                    div()
                                        .absolute()
                                        .left(px(4.0))
                                        .top(px(4.0))
                                        .text_size(px(9.5))
                                        .font_weight(font_weight)
                                        .text_color(text_color)
                                        .child(label)
                                )
                            } else {
                                None
                            }
                        )
                }))
        )
}
