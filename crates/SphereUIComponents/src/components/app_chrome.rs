use std::sync::Arc;

use gpui::{
    div, px, svg, App, InteractiveElement, IntoElement, MouseButton, ParentElement, Styled,
    Window, WindowControlArea,
};

use crate::assets;
use crate::components::menu_bar;
use crate::components::title_bar::{
    chrome_button, draggable_spacer, section_separator, window_control_button, CHROME_PAD_X,
    CHROME_TITLE_SIZE,
};
use crate::platform_chrome::PlatformChromePolicy;
use crate::theme::Colors;

/// Click handler for top-level menu buttons. Receives `(menu_id, anchor_x)`
/// — anchor_x is the click X position which the dropdown overlay uses to
/// align itself under the clicked label.
pub type MenuOpenCb = menu_bar::MenuOpenCb;
pub type ChromeActionCb = Arc<dyn Fn(&(), &mut Window, &mut App) + 'static>;
pub type ProjectOpenCb = Arc<dyn Fn(&f32, &mut Window, &mut App) + 'static>;

#[derive(Clone)]
pub struct ProjectChromeState {
    pub name: String,
    pub is_dirty: bool,
    pub on_open_project_menu: ProjectOpenCb,
}

#[derive(Clone)]
pub struct PanelChromeState {
    pub browser_visible: bool,
    pub inspector_visible: bool,
    pub mixer_visible: bool,
    pub on_toggle_browser: ChromeActionCb,
    pub on_toggle_mixer: ChromeActionCb,
    pub on_toggle_inspector: ChromeActionCb,
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

fn menu_area(
    open_menu_id: Option<&str>,
    on_open_menu: MenuOpenCb,
    viewport_width: f32,
) -> impl IntoElement {
    menu_bar::menu_bar(open_menu_id, on_open_menu, viewport_width)
}

fn project_title(state: ProjectChromeState) -> impl IntoElement {
    let on_open = state.on_open_project_menu.clone();
    let status = if state.is_dirty { "Unsaved" } else { "Saved" };
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .h(px(24.0))
        .px(px(8.0))
        .rounded_md()
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|s| s.bg(Colors::surface_control_hover()))
        .on_mouse_down(gpui::MouseButton::Left, move |event, window, cx| {
            let x: f32 = event.position.x.into();
            on_open(&x, window, cx);
        })
        .occlude()
        .child(
            div()
                .text_color(Colors::text_secondary())
                .text_size(px(CHROME_TITLE_SIZE))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .truncate()
                .child(state.name),
        )
        .child(
            div()
                .flex_none()
                .text_color(if state.is_dirty {
                    Colors::status_warning()
                } else {
                    Colors::text_faint()
                })
                .text_size(px(9.0))
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
            chrome_button(
                Some(assets::ICON_SKIP_BACK_PATH),
                "<<",
                false,
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
            chrome_button(Some(assets::ICON_PLAY_PATH), ">", state.playing, play_color)
                .cursor(gpui::CursorStyle::PointingHand)
                .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                    on_play(&(), window, cx);
                })
                .occlude(),
        )
        // Stop
        .child(
            chrome_button(
                Some(assets::ICON_SQUARE_PATH),
                "[]",
                false,
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
            chrome_button(
                Some(assets::ICON_CIRCLE_PATH),
                "REC",
                state.recording,
                record_color,
            )
            .opacity(0.38),
        )
        // Loop
        .child(
            chrome_button(
                Some(assets::ICON_REPEAT2_PATH),
                "LOOP",
                state.loop_enabled,
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
            chrome_button(
                Some(assets::ICON_TIMER_PATH),
                "MET",
                state.metronome_enabled,
                metronome_color,
            )
            .cursor(gpui::CursorStyle::PointingHand)
            .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                on_metronome(&(), window, cx);
            })
            .occlude(),
        )
        .child(section_separator())
        // Position display
        .child(
            div()
                .w(px(78.0))
                .h(px(24.0))
                .flex()
                .items_center()
                .justify_center()
                .text_color(Colors::text_primary())
                .text_size(px(12.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(state.position_label),
        )
        .child(section_separator())
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
                        .text_size(px(9.0))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child("BPM"),
                )
                .child(
                    div()
                        .w(px(32.0))
                        .h(px(19.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_md()
                        .bg(Colors::surface_input())
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
                        .h(px(19.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_md()
                        .bg(Colors::surface_input())
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
                        .h(px(19.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_md()
                        .bg(Colors::surface_input())
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

fn panel_toggle_button(
    icon_path: &'static str,
    fallback: &'static str,
    active: bool,
    on_click: ChromeActionCb,
) -> impl IntoElement {
    let color = if active {
        Colors::accent_primary()
    } else {
        Colors::text_muted()
    };
    let on_click = on_click.clone();
    chrome_button(Some(icon_path), fallback, active, color)
        .cursor(gpui::CursorStyle::PointingHand)
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            on_click(&(), window, cx);
        })
        .occlude()
}

fn panel_toggles(state: PanelChromeState) -> impl IntoElement {
    let on_browser = state.on_toggle_browser.clone();
    let on_mixer = state.on_toggle_mixer.clone();
    let on_inspector = state.on_toggle_inspector.clone();
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(2.0))
        .px(px(2.0))
        .child(panel_toggle_button(
            assets::ICON_FOLDER_OPEN_PATH,
            "BROWSER",
            state.browser_visible,
            on_browser,
        ))
        .child(panel_toggle_button(
            assets::ICON_PANEL_BOTTOM_PATH,
            "MIXER",
            state.mixer_visible,
            on_mixer,
        ))
        .child(panel_toggle_button(
            assets::ICON_PANEL_RIGHT_PATH,
            "INSPECT",
            state.inspector_visible,
            on_inspector,
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
        .child(chrome_button(
            Some(assets::ICON_FOLDER_PATH),
            "IMPORT",
            false,
            Colors::text_muted(),
        ))
        // Save
        .child(chrome_button(
            Some(assets::ICON_SAVE_PATH),
            "SAVE",
            false,
            Colors::text_muted(),
        ))
        // Share
        .child(chrome_button(
            Some(assets::ICON_SHARE_PATH),
            "SHARE",
            false,
            Colors::text_muted(),
        ))
}

fn report_bug_button() -> impl IntoElement {
    let amber_bg = Colors::with_alpha(Colors::status_warning(), 0.07);
    let amber_text = Colors::with_alpha(Colors::status_warning(), 0.70);
    let amber_border = Colors::with_alpha(Colors::status_warning(), 0.22);

    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(4.0))
        .h(px(24.0))
        .px(px(8.0))
        .rounded_md()
        .bg(amber_bg)
        .border_1()
        .border_color(amber_border)
        .hover(|s| {
            s.bg(Colors::with_alpha(Colors::status_warning(), 0.14))
                .border_color(Colors::with_alpha(Colors::status_warning(), 0.40))
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
        .child(window_control_button(
            WindowControlArea::Min,
            assets::ICON_MINIMIZE_PATH,
            "-",
        ))
        .child(window_control_button(
            WindowControlArea::Max,
            max_path,
            max_fallback,
        ))
        .child(window_control_button(
            WindowControlArea::Close,
            assets::ICON_X_PATH,
            "X",
        ))
}

// ── Public entry point ────────────────────────────────────────────────────────

pub fn app_chrome(
    window: &gpui::Window,
    open_menu_id: Option<&str>,
    on_open_menu: MenuOpenCb,
    project: ProjectChromeState,
    transport: TransportChromeState,
    panels: PanelChromeState,
) -> impl IntoElement {
    let policy = PlatformChromePolicy::current();
    let viewport_width: f32 = window.bounds().size.width.into();

    let mut chrome = div()
        .flex()
        .flex_row()
        .items_center()
        .h(px(policy.titlebar_height_px))
        .w_full()
        .bg(Colors::surface_titlebar())
        .border_b_1()
        .border_color(Colors::border_subtle())
        .pl(policy.traffic_light_left_padding())
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
        });

    if policy.show_in_window_menubar {
        chrome = chrome
            .child(menu_area(
                open_menu_id,
                on_open_menu,
                viewport_width,
            ))
            .child(section_separator());
    }

    chrome = chrome
        .child(project_title(project))
        .child(draggable_spacer())
        .child(transport_controls(transport))
        .child(section_separator())
        .child(panel_toggles(panels))
        .child(section_separator())
        .child(utility_buttons())
        .child(section_separator())
        .child(
            div()
                .flex()
                .items_center()
                .px(px(CHROME_PAD_X))
                .child(report_bug_button()),
        )
        .child(section_separator());

    if policy.show_window_controls {
        chrome = chrome.child(window_controls(window));
    }

    chrome
}
