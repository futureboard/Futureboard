use std::path::PathBuf;
use std::sync::Arc;

use gpui::prelude::FluentBuilder;
use gpui::{
    div, img, px, svg, App, Context, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window,
    WindowControlArea,
};

use crate::assets;
use crate::components::title_bar::{
    draggable_spacer, section_separator, window_control_button, CHROME_PAD_X, CHROME_TITLE_SIZE,
    STATUSBAR_HEIGHT,
};
use crate::components::{text_field, TextInputAction, TextInputState};
use crate::embedded_assets::APP_LOGO_PATH;
use crate::feeds::{FeedFilter, FeedItem, FeedProvider, StaticFeedProvider};
use crate::platform_chrome::PlatformChromePolicy;
use crate::project::{ProjectCreateOptions, ProjectTemplate, RecentProject, RecentProjectsStore};
use crate::settings::SettingsSchema;
use crate::theme::{self, Colors};

/// `FUTUREBOARD_WELCOME_DEBUG=1` enables QA logging for the start screen
/// (selected tab, resolved default project path, recent/feed activity).
fn welcome_debug(args: std::fmt::Arguments<'_>) {
    if std::env::var("FUTUREBOARD_WELCOME_DEBUG").as_deref() == Ok("1") {
        eprintln!("[welcome] {args}");
    }
}

macro_rules! welcome_debug {
    ($($arg:tt)*) => { welcome_debug(format_args!($($arg)*)) };
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupRoute {
    Splash,
    Welcome,
    Workspace,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WelcomeAction {
    EmptyProject,
    MidiComposer,
    AudioSession,
    MixTemplate,
    CreateProject(ProjectCreateOptions),
    /// Legacy: request the studio-side Open Project dialog. Kept for back-compat;
    /// the Welcome screen now browses + validates itself and emits
    /// [`WelcomeAction::OpenProjectFile`] instead.
    OpenProject,
    /// Open a specific, already-validated project file (from the Welcome
    /// Open Project tab's browse flow).
    OpenProjectFile(PathBuf),
    OpenRecent(PathBuf),
    /// Open the workspace shell directly with a blank, unsaved project.
    OpenEmptyWorkspace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StartupNav {
    Welcome,
    NewProject,
    OpenProject,
    RecentProjects,
    Feeds,
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
    gpu_status: SharedString,
    active_nav: StartupNav,
    recent_projects: Vec<RecentProject>,
    selected: Option<WelcomeSelection>,
    callbacks: WelcomeCallbacks,
    project_name_input: TextInputState,
    selected_template: ProjectTemplate,
    project_sample_rate: u32,
    project_bpm: f32,
    project_time_signature: (u32, u32),
    // Feeds tab
    feeds: Vec<FeedItem>,
    feed_filter: FeedFilter,
    // Default project location (resolved at construction from settings)
    default_project_dir: PathBuf,
    default_dir_configured: bool,
    // Quick audio status readout (from saved settings)
    audio_backend: SharedString,
    audio_device_out: SharedString,
    // Open Project tab: inline validation error from the last browse attempt.
    open_error: Option<SharedString>,
}

impl WelcomeWindow {
    pub fn new(
        version: impl Into<SharedString>,
        callbacks: WelcomeCallbacks,
        focus_handle: FocusHandle,
    ) -> Self {
        let mut recent = RecentProjectsStore::load();
        recent.refresh_missing();

        // Feeds load instantly from the static provider — no network, no block.
        let feeds = StaticFeedProvider.load_feed_items();
        welcome_debug!("feed provider returned {} item(s)", feeds.len());

        // Default project path + audio readout from saved settings (the global
        // SettingsModel entity does not exist yet at Welcome time).
        let schema = SettingsSchema::load_from_disk();
        let default_project_dir = schema.general.resolved_default_project_dir();
        let default_dir_configured = schema.general.has_configured_project_dir();
        welcome_debug!(
            "default project path resolved -> {} (configured={})",
            default_project_dir.display(),
            default_dir_configured
        );

        let mut project_name_input = TextInputState::new("welcome-project-name", focus_handle)
            .with_placeholder("Untitled Project");
        project_name_input.set_value("Untitled Project");

        Self {
            version: version.into(),
            route: StartupRoute::Splash,
            loading_status: SharedString::from("Loading Futureboard Studio"),
            gpu_status: SharedString::from("Pending"),
            active_nav: StartupNav::Welcome,
            recent_projects: recent.entries().iter().take(7).cloned().collect(),
            selected: Some(WelcomeSelection::Start(0)),
            callbacks,
            project_name_input,
            selected_template: ProjectTemplate::Empty,
            project_sample_rate: schema.general.project_defaults.sample_rate,
            project_bpm: 120.0,
            project_time_signature: (4, 4),
            feeds,
            feed_filter: FeedFilter::All,
            default_project_dir,
            default_dir_configured,
            audio_backend: SharedString::from(schema.hardware.audio.driver_type),
            audio_device_out: SharedString::from(schema.hardware.audio.device_out),
            open_error: None,
        }
    }

    pub fn set_loading_status(&mut self, status: impl Into<SharedString>) {
        self.loading_status = status.into();
    }

    pub fn set_gpu_status(&mut self, status: impl Into<SharedString>) {
        self.gpu_status = status.into();
    }

    pub fn show_welcome(&mut self) {
        self.route = StartupRoute::Welcome;
        self.loading_status = SharedString::from("Ready");
    }

    fn handle_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.is_held {
            return;
        }
        if self.project_name_input.is_focused(window) {
            let action = self
                .project_name_input
                .handle_key_with_clipboard(event, Some(cx));
            welcome_debug!("project name typed -> {}", self.project_name_input.value);
            match action {
                TextInputAction::Submit => self.create_project_from_welcome(window, cx),
                TextInputAction::Cancel => {
                    self.active_nav = StartupNav::Welcome;
                    cx.notify();
                }
                TextInputAction::Consumed | TextInputAction::Pass => cx.notify(),
            }
            return;
        }
        match event.keystroke.key.as_str() {
            "enter" | "numpad_enter" => {
                if let Some(action) = self.selected_action() {
                    // Open Project is handled inside Welcome (browse + validate),
                    // so Enter just reveals the Open Project tab rather than
                    // firing the legacy studio-side dialog.
                    if action == WelcomeAction::OpenProject {
                        self.active_nav = StartupNav::OpenProject;
                        cx.notify();
                    } else {
                        (self.callbacks.on_action)(action, window, cx);
                    }
                }
            }
            "escape" => window.remove_window(),
            _ => {}
        }
    }

    fn create_project_from_welcome(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = crate::project::io::sanitize_project_name(&self.project_name_input.value);
        let options = ProjectCreateOptions {
            name,
            base_dir: self.default_project_dir.clone(),
            template: self.selected_template,
            sample_rate: self.project_sample_rate,
            bpm: self.project_bpm,
            time_signature_num: self.project_time_signature.0,
            time_signature_den: self.project_time_signature.1,
        };
        welcome_debug!(
            "create project requested name={} dir={} template={}",
            options.name,
            options.base_dir.display(),
            options.template.label()
        );
        (self.callbacks.on_action)(WelcomeAction::CreateProject(options), window, cx);
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

    /// Open a folder picker to choose the default project directory. Persists
    /// the choice to settings on success. If the picker is unavailable or the
    /// user cancels, nothing changes (no faked success).
    fn change_default_dir(&mut self, cx: &mut Context<Self>) {
        let start_dir = self.default_project_dir.clone();
        let entity = cx.entity().clone();
        cx.spawn(async move |_this, cx| {
            let result = rfd::AsyncFileDialog::new()
                .set_title("Choose Default Project Location")
                .set_directory(&start_dir)
                .pick_folder()
                .await;
            let Some(handle) = result else {
                welcome_debug!("default project path change cancelled");
                return;
            };
            let path = handle.path().to_path_buf();
            // Best-effort: create the folder now so it is ready for new projects.
            let _ = std::fs::create_dir_all(&path);
            SettingsSchema::persist_default_project_directory(Some(path.clone()));
            welcome_debug!("default project path changed -> {}", path.display());
            let _ = entity.update(cx, |this, cx| {
                this.default_project_dir = path;
                this.default_dir_configured = true;
                cx.notify();
            });
        })
        .detach();
    }

    /// Open Project flow (Part B): browse for a `.fbproj` via the native picker,
    /// validate its header inline, then — only if valid — hand the path to the
    /// app to load and enter the studio. Cancel leaves Welcome untouched; an
    /// invalid pick shows an inline error rather than loading or crashing.
    fn browse_and_open_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_error = None;
        cx.notify();
        let start_dir = self.default_project_dir.clone();
        let on_action = self.callbacks.on_action.clone();
        // `spawn_in` keeps a window handle across the async picker await, so the
        // validated path can be handed to `on_action` (which needs a Window to
        // close Welcome) once the picker resolves.
        cx.spawn_in(window, async move |this, cx| {
            let result = rfd::AsyncFileDialog::new()
                .set_title("Open Project")
                .set_directory(&start_dir)
                .add_filter(
                    "Futureboard Project",
                    crate::project::io::SUPPORTED_PROJECT_FILE_EXTS,
                )
                .pick_file()
                .await;
            let Some(handle) = result else {
                welcome_debug!("open project cancelled");
                return;
            };
            let path = handle.path().to_path_buf();
            match crate::project::validate_project_file(&path) {
                Ok(version) => {
                    welcome_debug!("open project validated (v{version}) -> {}", path.display());
                    // Hand off to the app: opens the studio loading `path` and
                    // closes Welcome (see the on_action callback in app.rs).
                    let _ = cx.update(|window, app| {
                        on_action(WelcomeAction::OpenProjectFile(path), window, app);
                    });
                }
                Err(e) => {
                    let msg = format!("{} Details: {}", e.user_message(), e.technical_detail());
                    welcome_debug!("open project rejected -> {msg}");
                    let _ = this.update(cx, |this, cx| {
                        this.open_error = Some(SharedString::from(msg));
                        this.active_nav = StartupNav::OpenProject;
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }
}

impl Render for WelcomeWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let target = cx.entity().clone();
        let content = match self.route {
            StartupRoute::Splash => self.render_splash(),
            StartupRoute::Welcome | StartupRoute::Workspace => self.render_welcome(window, cx),
        };

        div()
            .key_context("WelcomeWindow")
            .capture_key_down(move |event, window, cx| {
                let _ = target.update(cx, |this, cx| this.handle_key(event, window, cx));
            })
            .flex()
            .flex_col()
            .size_full()
            .font(theme::ui_font())
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
                    .child(loading_rows(
                        self.loading_status.clone(),
                        self.gpu_status.clone(),
                    )),
            )
            .child(status_bar(
                "Startup",
                self.loading_status.clone(),
                "Native GPUI",
            ))
            .into_any_element()
    }

    fn render_welcome(&mut self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        // Center pane content depends on the selected sidebar tab.
        let center = match self.active_nav {
            StartupNav::NewProject => new_project_pane(
                cx,
                &self.project_name_input,
                self.project_name_input.is_focused(window),
                self.selected_template,
                self.project_sample_rate,
                self.project_bpm,
                self.project_time_signature,
                self.default_project_dir.clone(),
                &self.callbacks,
            ),
            StartupNav::Feeds => feeds_pane(cx, &self.feeds, self.feed_filter),
            StartupNav::OpenProject => open_project_pane(
                cx,
                &self.recent_projects,
                self.open_error.clone(),
                &self.callbacks,
            ),
            StartupNav::AudioSetup => audio_setup_pane(
                self.audio_backend.clone(),
                self.audio_device_out.clone(),
                self.gpu_status.clone(),
            ),
            _ => center_actions(cx, &self.selected, self.selected_template, &self.callbacks),
        };

        let default_path_label = self.default_project_dir.to_string_lossy().to_string();

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
                    .child(left_rail(cx, &self.active_nav))
                    .child(center)
                    .child(right_panel(
                        cx,
                        &self.recent_projects,
                        &self.selected,
                        &self.callbacks,
                        self.default_project_dir.clone(),
                        self.default_dir_configured,
                        self.audio_backend.clone(),
                        self.gpu_status.clone(),
                    )),
            )
            .child(status_bar(
                format!("Audio · {}", self.audio_backend),
                SharedString::from(default_path_label),
                format!("Native GPUI · {}", self.gpu_status),
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
            description: "Browse for an existing .fbproj project",
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

fn left_rail(cx: &mut Context<WelcomeWindow>, active: &StartupNav) -> impl IntoElement {
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
            StartupNav::OpenProject,
            "Open Project",
            assets::ICON_FOLDER_OPEN_PATH,
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
            StartupNav::Feeds,
            "Feeds",
            assets::ICON_NEWSPAPER_PATH,
            active,
            None,
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
    // Action rows (Open Project) never show the active highlight — only real
    // tabs do.
    let is_active = action.is_none() && active == &nav;
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
                welcome_debug!("selected tab -> {:?}", this.active_nav);
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
    selected_template: ProjectTemplate,
    callbacks: &WelcomeCallbacks,
) -> gpui::AnyElement {
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
            // Open Project lives in its own Welcome tab (browse + validate), so
            // its card reveals that tab rather than firing the legacy dialog.
            if action == WelcomeAction::OpenProject {
                let _ = target.update(cx, |this, cx| {
                    this.selected = Some(WelcomeSelection::Start(index));
                    this.active_nav = StartupNav::OpenProject;
                    cx.notify();
                });
                return;
            }
            if let Some(template) = template_for_action(&action) {
                let _ = target.update(cx, |this, cx| {
                    this.selected = Some(WelcomeSelection::Start(index));
                    this.selected_template = template;
                    this.project_bpm = template.default_bpm();
                    this.project_time_signature = template.time_signature();
                    this.active_nav = StartupNav::NewProject;
                    cx.notify();
                });
                return;
            }
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
                .max_w(px(560.0))
                .w_full()
                .text_size(px(10.0))
                .text_color(Colors::text_startup_faint())
                .child(format!("Selected template: {}", selected_template.label())),
        )
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
        .into_any_element()
}

fn template_for_action(action: &WelcomeAction) -> Option<ProjectTemplate> {
    match action {
        WelcomeAction::EmptyProject => Some(ProjectTemplate::Empty),
        WelcomeAction::MidiComposer => Some(ProjectTemplate::BeatMaking),
        WelcomeAction::AudioSession => Some(ProjectTemplate::Recording),
        WelcomeAction::MixTemplate => Some(ProjectTemplate::Mixing),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn new_project_pane(
    cx: &mut Context<WelcomeWindow>,
    project_name_input: &TextInputState,
    name_focused: bool,
    selected_template: ProjectTemplate,
    sample_rate: u32,
    bpm: f32,
    time_signature: (u32, u32),
    default_dir: PathBuf,
    callbacks: &WelcomeCallbacks,
) -> gpui::AnyElement {
    let safe_name = crate::project::io::sanitize_project_name(&project_name_input.value);
    let preview = default_dir.join(&safe_name).to_string_lossy().to_string();
    let target = cx.entity().clone();
    let create_target = cx.entity().clone();
    let continue_target = cx.entity().clone();
    let on_continue = callbacks.on_action.clone();

    let mut template_row = div().flex().flex_row().flex_wrap().gap(px(6.0));
    for template in [
        ProjectTemplate::Empty,
        ProjectTemplate::BeatMaking,
        ProjectTemplate::Recording,
        ProjectTemplate::Mixing,
        ProjectTemplate::Scoring,
    ] {
        let is_active = selected_template == template;
        let target = target.clone();
        template_row = template_row.child(
            div()
                .id(SharedString::from(format!(
                    "welcome-template-{}",
                    template.label()
                )))
                .flex()
                .items_center()
                .h(px(26.0))
                .px(px(9.0))
                .rounded_md()
                .border(px(1.0))
                .border_color(if is_active {
                    Colors::accent_startup()
                } else {
                    Colors::border_startup_soft()
                })
                .bg(if is_active {
                    Colors::accent_startup_soft()
                } else {
                    Colors::surface_startup_panel()
                })
                .cursor(gpui::CursorStyle::PointingHand)
                .hover(|style| style.bg(Colors::surface_startup_elevated()))
                .on_mouse_down(gpui::MouseButton::Left, move |_event, _window, cx| {
                    let _ = target.update(cx, |this, cx| {
                        this.selected_template = template;
                        this.project_bpm = template.default_bpm();
                        this.project_time_signature = template.time_signature();
                        cx.notify();
                    });
                })
                .child(
                    div()
                        .text_size(px(10.5))
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
                        .child(template.label()),
                ),
        );
    }

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .p(px(16.0))
        .gap(px(12.0))
        .bg(Colors::surface_startup_window())
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(Colors::text_startup_strong())
                        .child("New Project"),
                )
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(Colors::text_startup_muted())
                        .child(
                            "Name the session, choose a template, then create the project folder.",
                        ),
                ),
        )
        .child(form_label("Project Name"))
        .child(text_field(project_name_input, name_focused))
        .child(form_label("Location"))
        .child(
            div()
                .rounded_md()
                .border(px(1.0))
                .border_color(Colors::border_startup_soft())
                .bg(Colors::surface_startup_panel())
                .px(px(10.0))
                .py(px(8.0))
                .text_size(px(10.5))
                .text_color(Colors::text_startup())
                .child(preview),
        )
        .child(form_label("Template"))
        .child(template_row)
        .child(form_label("Audio"))
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap(px(6.0))
                .child(readout_chip(format!("{} Hz", sample_rate)))
                .child(readout_chip(format!("{:.0} BPM", bpm)))
                .child(readout_chip(format!(
                    "{}/{}",
                    time_signature.0, time_signature.1
                ))),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .pt(px(4.0))
                .child(
                    div()
                        .id("welcome-create-project")
                        .flex()
                        .items_center()
                        .h(px(30.0))
                        .px(px(12.0))
                        .rounded_md()
                        .border(px(1.0))
                        .border_color(Colors::accent_startup())
                        .bg(Colors::accent_startup_soft())
                        .cursor(gpui::CursorStyle::PointingHand)
                        .hover(|style| style.bg(Colors::surface_startup_elevated()))
                        .on_mouse_down(gpui::MouseButton::Left, move |_event, window, cx| {
                            let _ = create_target.update(cx, |this, cx| {
                                this.create_project_from_welcome(window, cx);
                            });
                        })
                        .child(
                            div()
                                .text_size(px(11.0))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(Colors::text_startup_strong())
                                .child("Create Project"),
                        ),
                )
                .child(
                    div()
                        .id("welcome-new-continue")
                        .flex()
                        .items_center()
                        .h(px(30.0))
                        .px(px(10.0))
                        .rounded_md()
                        .border(px(1.0))
                        .border_color(Colors::border_startup())
                        .bg(Colors::surface_startup_panel())
                        .cursor(gpui::CursorStyle::PointingHand)
                        .hover(|style| style.bg(Colors::surface_startup_elevated()))
                        .on_mouse_down(gpui::MouseButton::Left, move |_event, window, cx| {
                            let _ = continue_target.update(cx, |this, cx| {
                                this.selected = Some(WelcomeSelection::Continue);
                                cx.notify();
                            });
                            on_continue(WelcomeAction::OpenEmptyWorkspace, window, cx);
                        })
                        .child(
                            div()
                                .text_size(px(11.0))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(Colors::text_startup_strong())
                                .child("Continue Without Project"),
                        ),
                ),
        )
        .into_any_element()
}

// ── Open Project tab (Part B) ─────────────────────────────────────────────────

fn open_project_pane(
    cx: &mut Context<WelcomeWindow>,
    recent: &[RecentProject],
    open_error: Option<SharedString>,
    callbacks: &WelcomeCallbacks,
) -> gpui::AnyElement {
    let browse_target = cx.entity().clone();

    let recent_list = if recent.is_empty() {
        div()
            .flex()
            .items_center()
            .justify_center()
            .min_h(px(80.0))
            .text_size(px(11.0))
            .text_color(Colors::text_startup_faint())
            .child("No recent projects yet")
            .into_any_element()
    } else {
        let mut list = div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .max_w(px(620.0))
            .w_full();
        for (index, item) in recent.iter().cloned().enumerate() {
            let target = cx.entity().clone();
            let on_action = callbacks.on_action.clone();
            list = list.child(recent_row(index, item, false, move |path, window, cx| {
                welcome_debug!("recent project clicked (open tab) -> {}", path.display());
                let _ = target.update(cx, |this, cx| {
                    this.active_nav = StartupNav::OpenProject;
                    cx.notify();
                });
                on_action(WelcomeAction::OpenRecent(path), window, cx);
            }));
        }
        list.into_any_element()
    };

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .p(px(16.0))
        .gap(px(12.0))
        .bg(Colors::surface_startup_window())
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(Colors::text_startup_strong())
                        .child("Open Project"),
                )
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(Colors::text_startup_muted())
                        .child("Browse for an existing project, or pick a recent one."),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .child(
                    div()
                        .id("welcome-open-browse")
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .h(px(30.0))
                        .px(px(12.0))
                        .rounded_md()
                        .border(px(1.0))
                        .border_color(Colors::accent_startup())
                        .bg(Colors::accent_startup_soft())
                        .cursor(gpui::CursorStyle::PointingHand)
                        .hover(|style| style.bg(Colors::surface_startup_elevated()))
                        .on_mouse_down(gpui::MouseButton::Left, move |_event, window, cx| {
                            let _ = browse_target
                                .update(cx, |this, cx| this.browse_and_open_project(window, cx));
                        })
                        .child(
                            svg()
                                .path(assets::ICON_FOLDER_OPEN_PATH)
                                .w(px(13.0))
                                .h(px(13.0))
                                .text_color(Colors::text_startup_strong()),
                        )
                        .child(
                            div()
                                .text_size(px(11.0))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(Colors::text_startup_strong())
                                .child("Browse…"),
                        ),
                )
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(Colors::text_startup_faint())
                        .child("Supported: .fbproj, .fbs"),
                ),
        )
        .when_some(open_error, |el, msg| el.child(open_error_banner(msg)))
        .child(section_label("Recent"))
        .child(
            div()
                .id("welcome-open-recent-scroll")
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(recent_list),
        )
        .into_any_element()
}

fn open_error_banner(msg: SharedString) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .rounded_md()
        .border(px(1.0))
        .border_color(Colors::status_error())
        .bg(Colors::surface_startup_panel())
        .px(px(10.0))
        .py(px(8.0))
        .child(
            div()
                .text_size(px(10.5))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(Colors::status_error())
                .child(msg),
        )
}

fn form_label(label: &'static str) -> impl IntoElement {
    div()
        .text_size(px(10.0))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(Colors::text_startup_muted())
        .child(label)
}

fn readout_chip(label: impl Into<String>) -> impl IntoElement {
    div()
        .rounded_md()
        .border(px(1.0))
        .border_color(Colors::border_startup_soft())
        .bg(Colors::surface_startup_panel())
        .px(px(8.0))
        .py(px(5.0))
        .text_size(px(10.5))
        .text_color(Colors::text_startup_strong())
        .child(label.into())
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
                        .child("Enter the studio without creating a project"),
                ),
        )
}

// ── Feeds tab ───────────────────────────────────────────────────────────────

fn feeds_pane(
    cx: &mut Context<WelcomeWindow>,
    feeds: &[FeedItem],
    filter: FeedFilter,
) -> gpui::AnyElement {
    let mut chips = div().flex().flex_row().flex_wrap().gap(px(6.0));
    for f in FeedFilter::all() {
        chips = chips.child(feed_filter_chip(cx, f, filter));
    }

    let visible: Vec<&FeedItem> = feeds.iter().filter(|item| filter.matches(item)).collect();

    let list = if visible.is_empty() {
        div()
            .flex()
            .items_center()
            .justify_center()
            .flex_1()
            .min_h(px(160.0))
            .text_size(px(11.0))
            .text_color(Colors::text_startup_faint())
            .child("No updates in this category yet")
            .into_any_element()
    } else {
        let mut col = div()
            .id("welcome-feeds-scroll")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .gap(px(8.0))
            .max_w(px(620.0))
            .w_full();
        for item in visible {
            col = col.child(feed_card(item));
        }
        col.into_any_element()
    };

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .p(px(16.0))
        .gap(px(12.0))
        .bg(Colors::surface_startup_window())
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(Colors::text_startup_strong())
                        .child("Feeds"),
                )
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(Colors::text_startup_muted())
                        .child("Announcements, release notes, and project updates."),
                ),
        )
        .child(chips)
        .child(list)
        .into_any_element()
}

fn feed_filter_chip(
    cx: &mut Context<WelcomeWindow>,
    filter: FeedFilter,
    active: FeedFilter,
) -> impl IntoElement {
    let is_active = filter == active;
    let target = cx.entity().clone();
    div()
        .id(SharedString::from(format!(
            "feed-filter-{}",
            filter.label()
        )))
        .flex()
        .items_center()
        .h(px(24.0))
        .px(px(10.0))
        .rounded_md()
        .border(px(1.0))
        .border_color(if is_active {
            Colors::accent_startup()
        } else {
            Colors::border_startup_soft()
        })
        .bg(if is_active {
            Colors::accent_startup_soft()
        } else {
            Colors::surface_startup_panel()
        })
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|style| style.bg(Colors::surface_startup_elevated()))
        .on_mouse_down(gpui::MouseButton::Left, move |_event, _window, cx| {
            let _ = target.update(cx, |this, cx| {
                this.feed_filter = filter;
                cx.notify();
            });
        })
        .child(
            div()
                .text_size(px(10.5))
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
                .child(filter.label()),
        )
}

fn feed_card(item: &FeedItem) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .rounded_md()
        .border(px(1.0))
        .border_color(Colors::border_startup_soft())
        .bg(Colors::surface_startup_panel())
        .px(px(12.0))
        .py(px(10.0))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(category_badge(item.category.label()))
                .when(item.unread, |row| row.child(unread_dot()))
                .child(div().flex_1())
                .child(
                    div()
                        .flex_none()
                        .text_size(px(10.0))
                        .text_color(Colors::text_startup_faint())
                        .child(item.date.clone()),
                ),
        )
        .child(
            div()
                .text_size(px(12.5))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(Colors::text_startup_strong())
                .child(item.title.clone()),
        )
        .child(
            div()
                .text_size(px(10.5))
                .text_color(Colors::text_startup_muted())
                .child(item.summary.clone()),
        )
}

fn category_badge(label: &'static str) -> impl IntoElement {
    div()
        .flex_none()
        .rounded_sm()
        .bg(Colors::feed_badge_background())
        .px(px(7.0))
        .py(px(2.0))
        .text_size(px(9.0))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(Colors::feed_badge_text())
        .child(label)
}

fn unread_dot() -> impl IntoElement {
    div()
        .flex_none()
        .w(px(6.0))
        .h(px(6.0))
        .rounded_full()
        .bg(Colors::feed_unread_dot())
}

// ── Audio Setup tab ───────────────────────────────────────────────────────────

fn audio_setup_pane(
    backend: SharedString,
    device_out: SharedString,
    gpu_status: SharedString,
) -> gpui::AnyElement {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .p(px(16.0))
        .gap(px(12.0))
        .bg(Colors::surface_startup_window())
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(Colors::text_startup_strong())
                        .child("Audio Setup"),
                )
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(Colors::text_startup_muted())
                        .child(
                            "Quick view of the saved audio configuration. Full options live in Preferences › Audio.",
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(0.0))
                .max_w(px(560.0))
                .w_full()
                .rounded_md()
                .border(px(1.0))
                .border_color(Colors::border_startup_soft())
                .bg(Colors::surface_startup_panel())
                .px(px(12.0))
                .py(px(4.0))
                .child(info_row("Audio Backend", backend))
                .child(info_row("Output Device", device_out))
                .child(info_row("GPU Renderer", gpu_status)),
        )
        .child(
            div()
                .text_size(px(10.0))
                .text_color(Colors::text_startup_faint())
                .child("Open a project to access the full audio preferences."),
        )
        .into_any_element()
}

fn info_row(label: &'static str, value: SharedString) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .h(px(30.0))
        .child(
            div()
                .text_size(px(10.5))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(Colors::text_startup_muted())
                .child(label),
        )
        .child(
            div()
                .truncate()
                .text_size(px(10.5))
                .text_color(Colors::text_startup_strong())
                .child(value),
        )
}

// ── Right panel: recent + default location + system status ─────────────────────

#[allow(clippy::too_many_arguments)]
fn right_panel(
    cx: &mut Context<WelcomeWindow>,
    recent: &[RecentProject],
    selected: &Option<WelcomeSelection>,
    callbacks: &WelcomeCallbacks,
    default_dir: PathBuf,
    configured: bool,
    audio_backend: SharedString,
    gpu_status: SharedString,
) -> impl IntoElement {
    let recent_content = if recent.is_empty() {
        div()
            .flex()
            .items_center()
            .justify_center()
            .min_h(px(120.0))
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
                    welcome_debug!("recent project clicked -> {}", path.display());
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
        .min_h_0()
        .border_l(px(1.0))
        .border_color(Colors::border_startup_soft())
        .bg(Colors::surface_startup_panel())
        .p(px(12.0))
        .gap(px(10.0))
        .child(section_label("Recent"))
        .child(
            div()
                .id("welcome-recent-scroll")
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(recent_content),
        )
        .child(default_location_section(cx, default_dir, configured))
        .child(system_status_section(audio_backend, gpu_status))
}

fn default_location_section(
    cx: &mut Context<WelcomeWindow>,
    default_dir: PathBuf,
    configured: bool,
) -> impl IntoElement {
    let exists = default_dir.exists();
    let path_label = default_dir.to_string_lossy().to_string();
    let target = cx.entity().clone();

    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .pt(px(10.0))
        .border_t(px(1.0))
        .border_color(Colors::border_startup_soft())
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(section_label("Default Project Location"))
                .child(
                    div()
                        .id("welcome-change-default-dir")
                        .flex()
                        .items_center()
                        .h(px(22.0))
                        .px(px(8.0))
                        .rounded_md()
                        .border(px(1.0))
                        .border_color(Colors::border_startup())
                        .bg(Colors::surface_startup_elevated())
                        .cursor(gpui::CursorStyle::PointingHand)
                        .hover(|style| style.border_color(Colors::accent_startup()))
                        .on_mouse_down(gpui::MouseButton::Left, move |_event, _window, cx| {
                            let _ = target.update(cx, |this, cx| this.change_default_dir(cx));
                        })
                        .child(
                            div()
                                .text_size(px(10.0))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(Colors::text_startup_strong())
                                .child("Change…"),
                        ),
                ),
        )
        .child(
            div()
                .rounded_md()
                .border(px(1.0))
                .border_color(Colors::border_startup_soft())
                .bg(Colors::surface_startup_window())
                .px(px(10.0))
                .py(px(8.0))
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(Colors::text_startup())
                        .child(path_label),
                ),
        )
        .child(
            div()
                .text_size(px(9.5))
                .text_color(Colors::text_startup_faint())
                .child(if !configured {
                    "Using the platform default location.".to_string()
                } else if !exists {
                    "Folder will be created when needed.".to_string()
                } else {
                    "New projects are created here.".to_string()
                }),
        )
}

fn system_status_section(
    audio_backend: SharedString,
    gpu_status: SharedString,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .pt(px(10.0))
        .border_t(px(1.0))
        .border_color(Colors::border_startup_soft())
        .child(section_label("System"))
        .child(status_line("Audio Backend", audio_backend))
        .child(status_line("GPU Renderer", gpu_status))
        .child(status_line("Runtime", SharedString::from("Native GPUI")))
}

fn status_line(label: &'static str, value: SharedString) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(10.0))
        .h(px(20.0))
        .child(
            div()
                .text_size(px(10.0))
                .text_color(Colors::text_startup_muted())
                .child(label),
        )
        .child(
            div()
                .truncate()
                .text_size(px(10.0))
                .text_color(Colors::text_startup_strong())
                .child(value),
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
    let last_opened = format_last_opened(recent.last_opened_at);
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
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .child(
                    div()
                        .truncate()
                        .min_w_0()
                        .flex_1()
                        .text_size(px(11.5))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(Colors::text_startup_strong())
                        .child(recent.name),
                )
                .when(missing, |row| {
                    row.child(
                        div()
                            .flex_none()
                            .rounded_sm()
                            .bg(Colors::feed_badge_background())
                            .px(px(6.0))
                            .py(px(1.0))
                            .text_size(px(8.5))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(Colors::status_warning())
                            .child("Missing"),
                    )
                })
                .when(!missing && !last_opened.is_empty(), |row| {
                    row.child(
                        div()
                            .flex_none()
                            .text_size(px(9.0))
                            .text_color(Colors::text_startup_faint())
                            .child(last_opened.clone()),
                    )
                }),
        )
        .child(
            div()
                .truncate()
                .text_size(px(10.0))
                .text_color(Colors::text_startup_faint())
                .child(path_label),
        )
}

/// Render a coarse "time ago" label from a unix-seconds timestamp. Empty when
/// the timestamp is zero/unknown. Intentionally low-resolution — exact times
/// add no value on the start screen.
fn format_last_opened(last_opened_at: u64) -> String {
    if last_opened_at == 0 {
        return String::new();
    }
    let now = crate::project::now_secs();
    if now <= last_opened_at {
        return "Just now".to_string();
    }
    let secs = now - last_opened_at;
    let mins = secs / 60;
    let hours = mins / 60;
    let days = hours / 24;
    if mins < 1 {
        "Just now".to_string()
    } else if hours < 1 {
        format!("{mins}m ago")
    } else if days < 1 {
        format!("{hours}h ago")
    } else if days < 30 {
        format!("{days}d ago")
    } else {
        String::new()
    }
}

fn loading_rows(status: SharedString, gpu_status: SharedString) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .w(px(320.0))
        .child(loading_row("Assets", "Ready"))
        .child(loading_row("Recent Projects", "Loaded"))
        .child(loading_row("Audio System", "Deferred"))
        .child(loading_row("GPU Renderer", gpu_status))
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
                .flex_none()
                .text_size(px(10.0))
                .text_color(Colors::text_startup_muted())
                .child(left.into()),
        )
        .child(
            div()
                .truncate()
                .min_w_0()
                .px(px(12.0))
                .text_size(px(10.0))
                .text_color(Colors::text_startup_faint())
                .child(center.into()),
        )
        .child(
            div()
                .flex_none()
                .text_size(px(10.0))
                .text_color(Colors::text_startup_muted())
                .child(right.into()),
        )
}
