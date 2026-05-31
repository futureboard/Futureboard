use std::path::PathBuf;
use std::sync::Arc;

use gpui::prelude::FluentBuilder;
use gpui::{
    div, img, px, svg, App, Context, InteractiveElement, IntoElement, KeyDownEvent, ParentElement,
    Render, SharedString, Styled, Window, WindowControlArea,
};

use crate::assets;
use crate::components::title_bar::{
    draggable_spacer, section_separator, window_control_button, CHROME_PAD_X, CHROME_TITLE_SIZE,
    STATUSBAR_HEIGHT,
};
use crate::embedded_assets::APP_LOGO_PATH;
use crate::platform_chrome::PlatformChromePolicy;
use crate::project::{RecentProject, RecentProjectsStore};
use crate::theme::{self, Colors};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupRoute {
    Splash,
    Welcome,
    Workspace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WelcomeAction {
    EmptyProject,
    MidiComposer,
    AudioSession,
    MixTemplate,
    OpenProject,
    OpenRecent(PathBuf),
    /// Open the workspace shell directly with a blank, unsaved project.
    OpenEmptyWorkspace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StartupNav {
    Welcome,
    NewProject,
    RecentProjects,
    OpenProject,
    AudioSetup,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum WelcomeSelection {
    Start(usize),
    Recent(usize),
    Continue,
}

#[derive(Clone)]
pub struct WelcomeCallbacks {
    pub on_action: Arc<dyn Fn(WelcomeAction, &mut Window, &mut App) + 'static>,
}

pub struct WelcomeWindow {
    version: SharedString,
    route: StartupRoute,
    loading_status: SharedString,
    active_nav: StartupNav,
    recent_projects: Vec<RecentProject>,
    selected: Option<WelcomeSelection>,
    callbacks: WelcomeCallbacks,
}

impl WelcomeWindow {
    pub fn new(version: impl Into<SharedString>, callbacks: WelcomeCallbacks) -> Self {
        let mut recent = RecentProjectsStore::load();
        recent.refresh_missing();
        Self {
            version: version.into(),
            route: StartupRoute::Splash,
            loading_status: SharedString::from("Loading Futureboard Studio"),
            active_nav: StartupNav::Welcome,
            recent_projects: recent.entries().iter().take(7).cloned().collect(),
            selected: Some(WelcomeSelection::Start(0)),
            callbacks,
        }
    }

    pub fn set_loading_status(&mut self, status: impl Into<SharedString>) {
        self.loading_status = status.into();
    }

    pub fn show_welcome(&mut self) {
        self.route = StartupRoute::Welcome;
        self.loading_status = SharedString::from("Ready");
    }

    fn handle_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.is_held {
            return;
        }
        match event.keystroke.key.as_str() {
            "enter" | "numpad_enter" => {
                if let Some(action) = self.selected_action() {
                    (self.callbacks.on_action)(action, window, cx);
                }
            }
            "escape" => window.remove_window(),
            _ => {}
        }
    }

    fn selected_action(&self) -> Option<WelcomeAction> {
        match self.selected {
            Some(WelcomeSelection::Start(index)) => {
                start_rows().get(index).map(|row| row.action.clone())
            }
            Some(WelcomeSelection::Recent(index)) => self
                .recent_projects
                .get(index)
                .filter(|recent| !recent.missing)
                .map(|recent| WelcomeAction::OpenRecent(recent.path.clone())),
            Some(WelcomeSelection::Continue) => Some(WelcomeAction::OpenEmptyWorkspace),
            None => None,
        }
    }
}

impl Render for WelcomeWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let target = cx.entity().clone();
        let content = match self.route {
            StartupRoute::Splash => self.render_splash(),
            StartupRoute::Welcome | StartupRoute::Workspace => self.render_welcome(cx),
        };

        div()
            .key_context("WelcomeWindow")
            .capture_key_down(move |event, window, cx| {
                let _ = target.update(cx, |this, cx| this.handle_key(event, window, cx));
            })
            .flex()
            .flex_col()
            .size_full()
            .font_family(theme::FONT_FAMILY)
            .bg(Colors::surface_startup_bg())
            .child(startup_titlebar(window, self.route.clone()))
            .child(content)
    }
}

impl WelcomeWindow {
    fn render_splash(&self) -> gpui::AnyElement {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .items_center()
            .justify_center()
            .bg(Colors::surface_startup_bg())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(18.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .w(px(84.0))
                            .h(px(84.0))
                            .rounded_md()
                            .border(px(1.0))
                            .border_color(Colors::border_startup())
                            .bg(Colors::surface_startup_window())
                            .overflow_hidden()
                            .child(
                                img(SharedString::from(APP_LOGO_PATH))
                                    .w(px(84.0))
                                    .h(px(84.0)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(5.0))
                            .child(
                                div()
                                    .text_size(px(20.0))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(Colors::text_startup_strong())
                                    .child("Futureboard Studio"),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(Colors::text_startup_muted())
                                    .child(format!("v{}", self.version)),
                            ),
                    )
                    .child(loading_rows(self.loading_status.clone())),
            )
            .child(status_bar(
                "Startup",
                self.loading_status.clone(),
                "Native GPUI",
            ))
            .into_any_element()
    }

    fn render_welcome(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .bg(Colors::surface_startup_window())
            .child(welcome_header(self.version.clone()))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .child(left_rail(cx, &self.active_nav, &self.callbacks))
                    .child(center_actions(cx, &self.selected, &self.callbacks))
                    .child(recent_sidebar(
                        cx,
                        &self.recent_projects,
                        &self.selected,
                        &self.callbacks,
                    )),
            )
            .child(status_bar(
                "Ready",
                "No project loaded",
                "Open Empty Workspace enters the studio with a blank project",
            ))
            .into_any_element()
    }
}

#[derive(Clone)]
struct StartRow {
    title: &'static str,
    description: &'static str,
    shortcut: String,
    icon: &'static str,
    action: WelcomeAction,
}

fn start_rows() -> Vec<StartRow> {
    let modifier = if cfg!(target_os = "macos") {
        "Cmd"
    } else {
        "Ctrl"
    };
    vec![
        StartRow {
            title: "Empty Project",
            description: "Start with a blank arrangement",
            shortcut: format!("{modifier} + N"),
            icon: assets::ICON_PLUS_PATH,
            action: WelcomeAction::EmptyProject,
        },
        StartRow {
            title: "MIDI Composer",
            description: "Piano roll, instruments, and routing",
            shortcut: format!("{modifier} + Shift + M"),
            icon: assets::ICON_MUSIC_PATH,
            action: WelcomeAction::MidiComposer,
        },
        StartRow {
            title: "Audio Session",
            description: "Recording, editing, and mix-ready tracks",
            shortcut: format!("{modifier} + Shift + A"),
            icon: assets::ICON_MIC_PATH,
            action: WelcomeAction::AudioSession,
        },
        StartRow {
            title: "Mix Template",
            description: "Buses, sends, groups, and master chain",
            shortcut: format!("{modifier} + Shift + T"),
            icon: assets::ICON_SLIDERS_HORIZONTAL_PATH,
            action: WelcomeAction::MixTemplate,
        },
        StartRow {
            title: "Open Project...",
            description: "Browse for an existing .fbs file",
            shortcut: format!("{modifier} + O"),
            icon: assets::ICON_FOLDER_OPEN_PATH,
            action: WelcomeAction::OpenProject,
        },
    ]
}

fn startup_titlebar(window: &Window, route: StartupRoute) -> impl IntoElement {
    let policy = PlatformChromePolicy::current();
    let route_label = match route {
        StartupRoute::Splash => "Startup",
        StartupRoute::Welcome => "Welcome",
        StartupRoute::Workspace => "Workspace",
    };

    let mut chrome = div()
        .flex()
        .flex_row()
        .items_center()
        .h(px(policy.titlebar_height_px))
        .w_full()
        .pl(policy.traffic_light_left_padding())
        .bg(Colors::surface_titlebar())
        .border_b(px(1.0))
        .border_color(Colors::border_subtle())
        .window_control_area(WindowControlArea::Drag)
        .on_mouse_down(gpui::MouseButton::Left, |_, window, _cx| {
            window.start_window_move();
        })
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .h_full()
                .px(px(CHROME_PAD_X))
                .occlude()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .w(px(18.0))
                        .h(px(18.0))
                        .rounded_sm()
                        .overflow_hidden()
                        .child(
                            img(SharedString::from(APP_LOGO_PATH))
                                .w(px(18.0))
                                .h(px(18.0)),
                        ),
                )
                .child(
                    div()
                        .text_size(px(CHROME_TITLE_SIZE))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(Colors::text_primary())
                        .child("Futureboard Studio"),
                )
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(Colors::text_faint())
                        .child(route_label),
                ),
        )
        .child(draggable_spacer());

    if policy.show_window_controls {
        let is_maximized = window.is_maximized();
        let (max_path, max_fallback) = if is_maximized {
            (assets::ICON_RESTORE_PATH, "RESTORE")
        } else {
            (assets::ICON_MAXIMIZE_PATH, "MAX")
        };
        chrome = chrome
            .child(section_separator())
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
            ));
    }
    chrome
}

fn welcome_header(version: SharedString) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(16.0))
        .h(px(86.0))
        .px(px(18.0))
        .border_b(px(1.0))
        .border_color(Colors::border_startup_soft())
        .bg(Colors::surface_startup_window())
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(12.0))
                .min_w_0()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .w(px(42.0))
                        .h(px(42.0))
                        .rounded_md()
                        .overflow_hidden()
                        .border(px(1.0))
                        .border_color(Colors::border_startup())
                        .child(img(SharedString::from(APP_LOGO_PATH)).w(px(42.0)).h(px(42.0))),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .min_w_0()
                        .child(
                            div()
                                .truncate()
                                .text_size(px(17.0))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(Colors::text_startup_strong())
                                .child("Welcome to Futureboard Studio"),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(px(11.0))
                                .text_color(Colors::text_startup_muted())
                                .child(
                                    "Start a session, open a project, or build a workspace from scratch.",
                                ),
                        ),
                ),
        )
        .child(
            div()
                .flex_none()
                .text_size(px(10.5))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(Colors::text_startup_muted())
                .child(format!("v{version}")),
        )
}

fn left_rail(
    cx: &mut Context<WelcomeWindow>,
    active: &StartupNav,
    callbacks: &WelcomeCallbacks,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .w(px(190.0))
        .flex_none()
        .border_r(px(1.0))
        .border_color(Colors::border_startup_soft())
        .bg(Colors::surface_startup_panel())
        .p(px(8.0))
        .gap(px(2.0))
        .child(rail_item(
            cx,
            StartupNav::Welcome,
            "Welcome",
            assets::ICON_STAR_PATH,
            active,
            None,
        ))
        .child(rail_item(
            cx,
            StartupNav::NewProject,
            "New Project",
            assets::ICON_PLUS_PATH,
            active,
            None,
        ))
        .child(rail_item(
            cx,
            StartupNav::RecentProjects,
            "Recent Projects",
            assets::ICON_CLOCK_PATH,
            active,
            None,
        ))
        .child(rail_item(
            cx,
            StartupNav::OpenProject,
            "Open Project",
            assets::ICON_FOLDER_OPEN_PATH,
            active,
            Some((callbacks.on_action.clone(), WelcomeAction::OpenProject)),
        ))
        .child(rail_item(
            cx,
            StartupNav::AudioSetup,
            "Audio Setup",
            assets::ICON_VOLUME_2_PATH,
            active,
            None,
        ))
        .child(div().flex_1())
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .px(px(8.0))
                .py(px(8.0))
                .border_t(px(1.0))
                .border_color(Colors::border_startup_soft())
                .text_size(px(10.0))
                .text_color(Colors::text_startup_faint())
                .child("Native GPUI")
                .child("No WebView"),
        )
}

fn rail_item(
    cx: &mut Context<WelcomeWindow>,
    nav: StartupNav,
    label: &'static str,
    icon: &'static str,
    active: &StartupNav,
    action: Option<(
        Arc<dyn Fn(WelcomeAction, &mut Window, &mut App) + 'static>,
        WelcomeAction,
    )>,
) -> impl IntoElement {
    let is_active = active == &nav;
    let target = cx.entity().clone();
    div()
        .id(label)
        .flex()
        .items_center()
        .gap(px(8.0))
        .h(px(30.0))
        .px(px(8.0))
        .rounded_md()
        .bg(if is_active {
            Colors::surface_startup_elevated()
        } else {
            gpui::transparent_black().into()
        })
        .border_l(px(if is_active { 3.0 } else { 1.0 }))
        .border_color(if is_active {
            Colors::accent_startup()
        } else {
            gpui::transparent_black().into()
        })
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|style| style.bg(Colors::surface_startup_elevated()))
        .on_mouse_down(gpui::MouseButton::Left, move |_event, window, cx| {
            let _ = target.update(cx, |this, cx| {
                this.active_nav = nav.clone();
                cx.notify();
            });
            if let Some((callback, action)) = &action {
                callback(action.clone(), window, cx);
            }
        })
        .child(
            svg()
                .path(icon)
                .w(px(13.0))
                .h(px(13.0))
                .text_color(if is_active {
                    Colors::text_startup_strong()
                } else {
                    Colors::text_startup_muted()
                }),
        )
        .child(
            div()
                .text_size(px(11.0))
                .font_weight(if is_active {
                    gpui::FontWeight::SEMIBOLD
                } else {
                    gpui::FontWeight::NORMAL
                })
                .text_color(if is_active {
                    Colors::text_startup_strong()
                } else {
                    Colors::text_startup_muted()
                })
                .child(label),
        )
}

fn center_actions(
    cx: &mut Context<WelcomeWindow>,
    selected: &Option<WelcomeSelection>,
    callbacks: &WelcomeCallbacks,
) -> impl IntoElement {
    let mut rows = div()
        .flex()
        .flex_col()
        .gap(px(7.0))
        .max_w(px(560.0))
        .w_full();
    for (index, row) in start_rows().into_iter().enumerate() {
        let target = cx.entity().clone();
        let is_selected = selected == &Some(WelcomeSelection::Start(index));
        let action = row.action.clone();
        let on_action = callbacks.on_action.clone();
        rows = rows.child(start_row(index, row, is_selected, move |window, cx| {
            let _ = target.update(cx, |this, cx| {
                this.selected = Some(WelcomeSelection::Start(index));
                this.active_nav = StartupNav::NewProject;
                cx.notify();
            });
            on_action(action.clone(), window, cx);
        }));
    }

    let continue_selected = selected == &Some(WelcomeSelection::Continue);
    let target = cx.entity().clone();
    let on_continue = callbacks.on_action.clone();
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .p(px(16.0))
        .gap(px(12.0))
        .bg(Colors::surface_startup_window())
        .child(section_label("Start a session"))
        .child(rows)
        .child(
            div()
                .mt(px(4.0))
                .max_w(px(560.0))
                .w_full()
                .child(continue_row(continue_selected, move |window, cx| {
                    let _ = target.update(cx, |this, cx| {
                        this.selected = Some(WelcomeSelection::Continue);
                        this.active_nav = StartupNav::Welcome;
                        cx.notify();
                    });
                    on_continue(WelcomeAction::OpenEmptyWorkspace, window, cx);
                })),
        )
}

fn start_row(
    index: usize,
    row: StartRow,
    selected: bool,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(("welcome-start-row", index))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .min_h(px(58.0))
        .border(px(1.0))
        .border_l(px(if selected { 3.0 } else { 1.0 }))
        .border_color(if selected {
            Colors::accent_startup()
        } else {
            Colors::border_startup_soft()
        })
        .rounded_md()
        .bg(if selected {
            Colors::surface_startup_elevated()
        } else {
            Colors::surface_startup_panel()
        })
        .px(px(11.0))
        .py(px(8.0))
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|style| style.bg(Colors::surface_startup_elevated()))
        .on_mouse_down(gpui::MouseButton::Left, move |_event, window, cx| {
            on_click(window, cx);
        })
        .child(row_icon(row.icon))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .min_w_0()
                .flex_1()
                .child(
                    div()
                        .truncate()
                        .text_size(px(12.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(Colors::text_startup_strong())
                        .child(row.title),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(px(10.5))
                        .text_color(Colors::text_startup_muted())
                        .child(row.description),
                ),
        )
        .child(shortcut_badge(row.shortcut))
}

fn continue_row(
    selected: bool,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id("welcome-continue-row")
        .flex()
        .items_center()
        .gap(px(10.0))
        .min_h(px(50.0))
        .rounded_md()
        .border(px(1.0))
        .border_l(px(if selected { 3.0 } else { 1.0 }))
        .border_color(if selected {
            Colors::accent_startup()
        } else {
            Colors::border_startup_soft()
        })
        .bg(if selected {
            Colors::surface_startup_elevated()
        } else {
            Colors::surface_startup_panel()
        })
        .px(px(11.0))
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|style| style.bg(Colors::surface_startup_elevated()))
        .on_mouse_down(gpui::MouseButton::Left, move |_event, window, cx| {
            on_click(window, cx);
        })
        .child(row_icon(assets::ICON_CORNER_DOWN_LEFT_PATH))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .min_w_0()
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(Colors::text_startup_strong())
                        .child("Open Empty Workspace"),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(px(10.5))
                        .text_color(Colors::text_startup_muted())
                        .child("Enter the studio with a blank, unsaved project"),
                ),
        )
}

fn recent_sidebar(
    cx: &mut Context<WelcomeWindow>,
    recent: &[RecentProject],
    selected: &Option<WelcomeSelection>,
    callbacks: &WelcomeCallbacks,
) -> impl IntoElement {
    let content = if recent.is_empty() {
        div()
            .flex()
            .items_center()
            .justify_center()
            .flex_1()
            .min_h(px(180.0))
            .text_size(px(11.0))
            .text_color(Colors::text_startup_faint())
            .child("No recent projects yet")
            .into_any_element()
    } else {
        let mut list = div().flex().flex_col().gap(px(6.0));
        for (index, item) in recent.iter().cloned().enumerate() {
            let target = cx.entity().clone();
            let on_action = callbacks.on_action.clone();
            let is_selected = selected == &Some(WelcomeSelection::Recent(index));
            list = list.child(recent_row(
                index,
                item,
                is_selected,
                move |path, window, cx| {
                    let _ = target.update(cx, |this, cx| {
                        this.selected = Some(WelcomeSelection::Recent(index));
                        this.active_nav = StartupNav::RecentProjects;
                        cx.notify();
                    });
                    on_action(WelcomeAction::OpenRecent(path), window, cx);
                },
            ));
        }
        list.into_any_element()
    };

    div()
        .flex()
        .flex_col()
        .w(px(340.0))
        .flex_none()
        .border_l(px(1.0))
        .border_color(Colors::border_startup_soft())
        .bg(Colors::surface_startup_panel())
        .p(px(12.0))
        .gap(px(10.0))
        .child(section_label("Recent"))
        .child(content)
        .child(
            div()
                .mt(px(4.0))
                .pt(px(10.0))
                .border_t(px(1.0))
                .border_color(Colors::border_startup_soft())
                .text_size(px(10.0))
                .text_color(Colors::text_startup_faint())
                .child("Recent projects are read from the native recent project store."),
        )
}

fn recent_row(
    index: usize,
    recent: RecentProject,
    selected: bool,
    on_click: impl Fn(PathBuf, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let path = recent.path.clone();
    let path_label = recent.path.to_string_lossy().to_string();
    let missing = recent.missing;
    div()
        .id(("welcome-recent-row", index))
        .flex()
        .flex_col()
        .gap(px(3.0))
        .min_h(px(54.0))
        .rounded_md()
        .border(px(1.0))
        .border_l(px(if selected { 3.0 } else { 1.0 }))
        .border_color(if selected {
            Colors::accent_startup()
        } else {
            Colors::border_startup_soft()
        })
        .bg(if selected {
            Colors::surface_startup_elevated()
        } else {
            Colors::surface_startup_window()
        })
        .opacity(if missing { 0.48 } else { 1.0 })
        .px(px(10.0))
        .py(px(8.0))
        .cursor(if missing {
            gpui::CursorStyle::Arrow
        } else {
            gpui::CursorStyle::PointingHand
        })
        .hover(|style| {
            if missing {
                style
            } else {
                style.bg(Colors::surface_startup_elevated())
            }
        })
        .when(!missing, |row| {
            row.on_mouse_down(gpui::MouseButton::Left, move |_event, window, cx| {
                on_click(path.clone(), window, cx);
            })
        })
        .child(
            div()
                .truncate()
                .text_size(px(11.5))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(Colors::text_startup_strong())
                .child(recent.name),
        )
        .child(
            div()
                .truncate()
                .text_size(px(10.0))
                .text_color(Colors::text_startup_faint())
                .child(if missing {
                    "Missing".to_string()
                } else {
                    path_label
                }),
        )
}

fn loading_rows(status: SharedString) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .w(px(320.0))
        .child(loading_row("Assets", "Ready"))
        .child(loading_row("Recent Projects", "Loaded"))
        .child(loading_row("Audio System", "Deferred"))
        .child(loading_row("Workspace", status))
}

fn loading_row(label: impl Into<String>, value: impl Into<SharedString>) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .h(px(28.0))
        .border_b(px(1.0))
        .border_color(Colors::border_startup_soft())
        .child(
            div()
                .text_size(px(10.5))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(Colors::text_startup_muted())
                .child(label.into()),
        )
        .child(
            div()
                .text_size(px(10.5))
                .text_color(Colors::text_startup_strong())
                .child(value.into()),
        )
}

fn section_label(label: &'static str) -> impl IntoElement {
    div()
        .h(px(22.0))
        .flex()
        .items_center()
        .text_size(px(10.0))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(Colors::text_startup_muted())
        .child(label)
}

fn row_icon(path: &'static str) -> impl IntoElement {
    div()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .w(px(32.0))
        .h(px(32.0))
        .rounded_md()
        .border(px(1.0))
        .border_color(Colors::border_startup_soft())
        .bg(Colors::surface_startup_elevated())
        .child(
            svg()
                .path(path)
                .w(px(14.0))
                .h(px(14.0))
                .text_color(Colors::text_startup()),
        )
}

fn shortcut_badge(shortcut: String) -> impl IntoElement {
    div()
        .flex_none()
        .rounded_sm()
        .border(px(1.0))
        .border_color(Colors::border_startup_soft())
        .bg(Colors::surface_startup_window())
        .px(px(7.0))
        .py(px(3.0))
        .text_size(px(9.5))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(Colors::text_startup_faint())
        .child(shortcut)
}

fn status_bar(
    left: impl Into<String>,
    center: impl Into<SharedString>,
    right: impl Into<String>,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .h(px(STATUSBAR_HEIGHT))
        .px(px(8.0))
        .border_t(px(1.0))
        .border_color(Colors::border_startup_soft())
        .bg(Colors::surface_startup_panel())
        .child(
            div()
                .text_size(px(10.0))
                .text_color(Colors::text_startup_muted())
                .child(left.into()),
        )
        .child(
            div()
                .text_size(px(10.0))
                .text_color(Colors::text_startup_faint())
                .child(center.into()),
        )
        .child(
            div()
                .text_size(px(10.0))
                .text_color(Colors::text_startup_muted())
                .child(right.into()),
        )
}
