use std::sync::Arc;

use gpui::prelude::FluentBuilder;
use gpui::{
    div, px, rgba, svg, App, InteractiveElement, IntoElement, MouseButton, ParentElement,
    StatefulInteractiveElement, Styled, Window, WindowControlArea,
};

use crate::assets;
use crate::components::icon_button;
use crate::menu::MenuManifest;
use crate::theme::Colors;

/// Click handler for top-level menu buttons. Receives `(menu_id, anchor_x)`
/// — anchor_x is the click X position which the dropdown overlay uses to
/// align itself under the clicked label.
pub type MenuOpenCb = Arc<dyn Fn(&(String, f32), &mut Window, &mut App) + 'static>;
pub type ChromeActionCb = Arc<dyn Fn(&(), &mut Window, &mut App) + 'static>;
pub type ProjectOpenCb = Arc<dyn Fn(&f32, &mut Window, &mut App) + 'static>;

#[derive(Clone)]
pub struct ProjectChromeState {
    pub name: String,
    pub is_dirty: bool,
    pub on_open_project_menu: ProjectOpenCb,
}

#[derive(Clone)]
pub struct TransportChromeState {
    pub playing: bool,
    pub recording: bool,
    pub loop_enabled: bool,
    pub metronome_enabled: bool,
    pub position_label: String,
    pub bpm_label: String,
    pub time_signature_label: String,
    pub on_return_to_start: ChromeActionCb,
    pub on_play_toggle: ChromeActionCb,
    pub on_stop: ChromeActionCb,
    pub on_loop_toggle: ChromeActionCb,
    pub on_metronome_toggle: ChromeActionCb,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn divider() -> impl IntoElement {
    div()
        .w(px(1.0))
        .h(px(28.0))
        .bg(Colors::border_subtle())
        .mx(px(2.0))
}

// ── Left section ──────────────────────────────────────────────────────────────

fn menu_area(open_menu_id: Option<&str>, on_open_menu: MenuOpenCb) -> impl IntoElement {
    // Top-level labels are sourced from the generated menu manifest (which
    // is itself derived from `packages/shared/src/menu/menuItems.ts`). The
    // fallback inside `MenuManifest::load` keeps the strip populated even
    // when the JSON fails to parse, so this function never produces an
    // empty menu bar.
    let manifest = MenuManifest::load();
    let open_id_owned = open_menu_id.map(|s| s.to_string());

    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(1.0))
        .px(px(4.0))
        .children(manifest.menus.iter().enumerate().map(|(i, menu)| {
            let is_open = open_id_owned.as_deref() == Some(menu.id.as_str());
            let menu_id = menu.id.clone();
            let hover_menu_id = menu.id.clone();
            let cb = on_open_menu.clone();
            let hover_cb = on_open_menu.clone();
            let can_hover_switch = open_id_owned.is_some() && !is_open;
            let (bg, fg) = if is_open {
                (Colors::surface_hover(), Colors::text_primary())
            } else {
                // Transparent background; hover style supplies the bg.
                (gpui::transparent_black().into(), Colors::text_muted())
            };

            div()
                .px(px(7.0))
                .py(px(3.0))
                .rounded_md()
                .text_color(fg)
                .text_size(px(11.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .bg(bg)
                .hover(|s| {
                    s.bg(Colors::surface_hover())
                        .text_color(Colors::text_primary())
                })
                .id(("top-menu", i))
                .cursor(gpui::CursorStyle::PointingHand)
                .on_mouse_down(gpui::MouseButton::Left, move |event, w, cx| {
                    let click_x: f32 = event.position.x.into();
                    cb(&(menu_id.clone(), click_x), w, cx);
                })
                .when(can_hover_switch, |this| {
                    this.on_hover(move |hovered, window, cx| {
                        if *hovered {
                            let x: f32 = window.mouse_position().x.into();
                            hover_cb(&(hover_menu_id.clone(), x), window, cx);
                        }
                    })
                })
                .occlude()
                .child(menu.label.clone())
        }))
}

fn project_title(state: ProjectChromeState) -> impl IntoElement {
    let on_open = state.on_open_project_menu.clone();
    let status = if state.is_dirty { "Unsaved" } else { "Saved" };
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(4.0))
        .h(px(28.0))
        .px(px(7.0))
        .rounded_md()
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|s| s.bg(Colors::surface_hover()))
        .on_mouse_down(gpui::MouseButton::Left, move |event, window, cx| {
            let x: f32 = event.position.x.into();
            on_open(&x, window, cx);
        })
        .occlude()
        .child(
            div()
                .text_color(Colors::text_secondary())
                .text_size(px(12.0))
                .font_weight(gpui::FontWeight::BOLD)
                .child(state.name),
        )
        .child(
            div()
                .text_color(Colors::text_muted())
                .text_size(px(8.0))
                .font_weight(gpui::FontWeight::MEDIUM)
                .child(status),
        )
}

// ── Right section — transport + panel toggles + utility ───────────────────────

fn transport_controls(state: TransportChromeState) -> impl IntoElement {
    let play_color = if state.playing {
        Colors::accent_primary()
    } else {
        Colors::text_muted()
    };
    let record_color = if state.recording {
        Colors::status_error()
    } else {
        Colors::text_faint()
    };
    let loop_color = if state.loop_enabled {
        Colors::accent_primary()
    } else {
        Colors::text_muted()
    };
    let metronome_color = if state.metronome_enabled {
        Colors::accent_primary()
    } else {
        Colors::text_muted()
    };
    let on_return = state.on_return_to_start.clone();
    let on_play = state.on_play_toggle.clone();
    let on_stop = state.on_stop.clone();
    let on_loop = state.on_loop_toggle.clone();
    let on_metronome = state.on_metronome_toggle.clone();

    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(1.0))
        // Skip back
        .child(
            icon_button(
                Some(assets::ICON_SKIP_BACK_PATH),
                "<<",
                px(28.0),
                px(28.0),
                px(14.0),
                Colors::text_muted(),
            )
            .cursor(gpui::CursorStyle::PointingHand)
            .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                on_return(&(), window, cx);
            })
            .occlude(),
        )
        // Play
        .child(
            icon_button(
                Some(assets::ICON_PLAY_PATH),
                ">",
                px(28.0),
                px(28.0),
                px(14.0),
                play_color,
            )
            .cursor(gpui::CursorStyle::PointingHand)
            .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                on_play(&(), window, cx);
            })
            .occlude(),
        )
        // Stop
        .child(
            icon_button(
                Some(assets::ICON_SQUARE_PATH),
                "[]",
                px(28.0),
                px(28.0),
                px(14.0),
                Colors::text_muted(),
            )
            .cursor(gpui::CursorStyle::PointingHand)
            .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                on_stop(&(), window, cx);
            })
            .occlude(),
        )
        // Record
        .child(
            icon_button(
                Some(assets::ICON_CIRCLE_PATH),
                "REC",
                px(28.0),
                px(28.0),
                px(14.0),
                record_color,
            )
            .opacity(0.38),
        )
        // Loop
        .child(
            icon_button(
                Some(assets::ICON_REPEAT2_PATH),
                "LOOP",
                px(28.0),
                px(28.0),
                px(14.0),
                loop_color,
            )
            .cursor(gpui::CursorStyle::PointingHand)
            .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                on_loop(&(), window, cx);
            })
            .occlude(),
        )
        // Metronome
        .child(
            icon_button(
                Some(assets::ICON_TIMER_PATH),
                "MET",
                px(28.0),
                px(28.0),
                px(14.0),
                metronome_color,
            )
            .cursor(gpui::CursorStyle::PointingHand)
            .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                on_metronome(&(), window, cx);
            })
            .occlude(),
        )
        .child(divider())
        // Position display
        .child(
            div()
                .w(px(84.0))
                .h(px(28.0))
                .flex()
                .items_center()
                .justify_center()
                .text_color(Colors::text_primary())
                .text_size(px(13.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(state.position_label),
        )
        .child(divider())
        // BPM
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(4.0))
                .px(px(4.0))
                .child(
                    div()
                        .text_color(Colors::text_muted())
                        .text_size(px(8.0))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child("BPM"),
                )
                .child(
                    div()
                        .w(px(32.0))
                        .h(px(20.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_md()
                        .bg(Colors::surface_raised())
                        .text_color(Colors::text_primary())
                        .text_size(px(11.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(state.bpm_label),
                ),
        )
        // Time signature
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(2.0))
                .px(px(4.0))
                .child(
                    div()
                        .w(px(18.0))
                        .h(px(20.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_md()
                        .bg(Colors::surface_raised())
                        .text_color(Colors::text_primary())
                        .text_size(px(11.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(
                            state
                                .time_signature_label
                                .split_once('/')
                                .map(|(num, _)| num.to_string())
                                .unwrap_or_else(|| "4".to_string()),
                        ),
                )
                .child(
                    div()
                        .text_color(Colors::text_muted())
                        .text_size(px(10.0))
                        .child("/"),
                )
                .child(
                    div()
                        .w(px(18.0))
                        .h(px(20.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_md()
                        .bg(Colors::surface_raised())
                        .text_color(Colors::text_primary())
                        .text_size(px(11.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(
                            state
                                .time_signature_label
                                .split_once('/')
                                .map(|(_, den)| den.to_string())
                                .unwrap_or_else(|| "4".to_string()),
                        ),
                ),
        )
}

fn panel_toggles() -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(2.0))
        .px(px(2.0))
        // Browser
        .child(icon_button(
            Some(assets::ICON_FOLDER_OPEN_PATH),
            "BROWSER",
            px(28.0),
            px(28.0),
            px(14.0),
            Colors::text_muted(),
        ))
        // Mixer
        .child(icon_button(
            Some(assets::ICON_PANEL_BOTTOM_PATH),
            "MIXER",
            px(28.0),
            px(28.0),
            px(14.0),
            Colors::text_muted(),
        ))
        // Inspector
        .child(icon_button(
            Some(assets::ICON_PANEL_RIGHT_PATH),
            "INSPECT",
            px(28.0),
            px(28.0),
            px(14.0),
            Colors::text_muted(),
        ))
}

fn utility_buttons() -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(2.0))
        .px(px(2.0))
        // Import audio
        .child(icon_button(
            Some(assets::ICON_FOLDER_PATH),
            "IMPORT",
            px(28.0),
            px(28.0),
            px(14.0),
            Colors::text_muted(),
        ))
        // Save
        .child(icon_button(
            Some(assets::ICON_SAVE_PATH),
            "SAVE",
            px(28.0),
            px(28.0),
            px(14.0),
            Colors::text_muted(),
        ))
        // Share
        .child(icon_button(
            Some(assets::ICON_SHARE_PATH),
            "SHARE",
            px(28.0),
            px(28.0),
            px(14.0),
            Colors::text_muted(),
        ))
}

fn report_bug_button() -> impl IntoElement {
    let amber_bg = rgba(0xFBBF2412_u32); // rgba(251,191,36, 0.07)
    let amber_text = rgba(0xFBBF24B3_u32); // rgba(251,191,36, 0.70)
    let amber_border = rgba(0xFBBF2438_u32); // rgba(251,191,36, 0.22)

    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(4.0))
        .h(px(28.0))
        .px(px(8.0))
        .rounded_md()
        .bg(amber_bg)
        .border_1()
        .border_color(amber_border)
        .hover(|s| {
            s.bg(rgba(0xFBBF2424_u32))
                .border_color(rgba(0xFBBF2466_u32))
        })
        .child(
            svg()
                .path(assets::ICON_BUG_PATH)
                .w(px(11.0))
                .h(px(11.0))
                .text_color(amber_text),
        )
        .child(
            div()
                .text_color(amber_text)
                .text_size(px(10.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child("Report bug"),
        )
        .occlude()
}

fn window_controls(window: &gpui::Window) -> impl IntoElement {
    let is_maximized = window.is_maximized();
    let (max_path, max_fallback) = if is_maximized {
        (assets::ICON_RESTORE_PATH, "RESTORE")
    } else {
        (assets::ICON_MAXIMIZE_PATH, "MAX")
    };

    div()
        .flex()
        .flex_row()
        .items_center()
        .h_full()
        .child(
            icon_button(
                Some(assets::ICON_MINIMIZE_PATH),
                "-",
                px(32.0),
                px(32.0),
                px(16.0),
                Colors::text_muted(),
            )
            .window_control_area(WindowControlArea::Min)
            .occlude(),
        )
        .child(
            icon_button(
                Some(max_path),
                max_fallback,
                px(32.0),
                px(32.0),
                px(16.0),
                Colors::text_muted(),
            )
            .window_control_area(WindowControlArea::Max)
            .occlude(),
        )
        .child(
            icon_button(
                Some(assets::ICON_X_PATH),
                "X",
                px(32.0),
                px(32.0),
                px(16.0),
                Colors::text_muted(),
            )
            .window_control_area(WindowControlArea::Close)
            .occlude(),
        )
}

// ── Public entry point ────────────────────────────────────────────────────────

pub fn app_chrome(
    window: &gpui::Window,
    open_menu_id: Option<&str>,
    on_open_menu: MenuOpenCb,
    project: ProjectChromeState,
    transport: TransportChromeState,
) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .h(px(36.0))
        .w_full()
        .bg(Colors::surface_panel())
        .border_b_1()
        .border_color(Colors::border_subtle())
        // Windows: NCHITTEST callback returns `HTCAPTION` for hitboxes
        // tagged Drag, letting DefWindowProc start the system move.
        .window_control_area(WindowControlArea::Drag)
        // Linux (Wayland / X11) and macOS: `start_window_move` is the
        // implemented drag API there; the WindowControlArea path is a
        // no-op on those platforms. Safe to attach here because every
        // interactive child below (menu buttons, transport buttons,
        // window controls, report-bug) calls `.occlude()`. Occlude is
        // `HitboxBehavior::BlockMouse`, which breaks the `hit_test`
        // iteration at that child — the chrome's id is then NOT in
        // `mouse_hit_test.ids`, so this on_mouse_down does NOT fire
        // for clicks on those buttons.
        .on_mouse_down(MouseButton::Left, |_, window, _cx| {
            window.start_window_move();
        })
        // ── Left: menus + project ─────────────────────────────────────────────
        .child(menu_area(open_menu_id, on_open_menu))
        .child(divider())
        .child(project_title(project))
        // ── Drag region spacer ────────────────────────────────────────────────
        // Carry both drag mechanisms on the spacer too — Windows reads
        // the WindowControlArea, Linux/macOS reads the on_mouse_down.
        // Redundant with the chrome root, but a child hitbox in the
        // exact same band makes resolution deterministic even if a
        // future sibling adds an `occlude()` that drifts into the
        // central spacer.
        .child(
            div()
                .flex_1()
                .h_full()
                .window_control_area(WindowControlArea::Drag)
                .on_mouse_down(MouseButton::Left, |_, window, _cx| {
                    window.start_window_move();
                }),
        )
        // ── Right: transport controls ─────────────────────────────────────────
        .child(transport_controls(transport))
        .child(divider())
        // Panel toggles: Browser | Mixer | Inspector
        .child(panel_toggles())
        .child(divider())
        // Utility: Import | Save | Share
        .child(utility_buttons())
        .child(divider())
        // Report bug
        .child(
            div()
                .flex()
                .items_center()
                .px(px(4.0))
                .child(report_bug_button()),
        )
        .child(divider())
        // Window controls (min / max / close)
        .child(window_controls(window))
}
