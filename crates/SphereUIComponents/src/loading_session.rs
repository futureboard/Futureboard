//! App-level "Loading Session…" gate — runs before [`crate::layout::StudioLayout`]
//! is mounted so no session-bound UI can observe a half-loaded project.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    div, px, App, AppContext, BorrowAppContext, Bounds, Context, FocusHandle, Global, InteractiveElement,
    IntoElement, ParentElement, Pixels, Render, SharedString, Styled, Window, WindowHandle,
};

use crate::app_state::{AppMode, AppSessionGate};
use crate::components::progress_dialog::{progress_bar, ProgressBarValue};
use crate::components::timeline::timeline_state::TimelineState;
use crate::components::title_bar::{external_window_titlebar_compact, TITLEBAR_HEIGHT};
use crate::layout::ProjectOpenOptions;
use crate::project::io::{load_project, validate_project_file};
use crate::project::{FutureboardProject, ProjectSession};
use crate::theme::{self, Colors};

const LOAD_WINDOW_WIDTH: f32 = 430.0;
const LOAD_WINDOW_HEIGHT: f32 = 168.0;
const BODY_PAD_X: f32 = 16.0;
const BODY_PAD_Y: f32 = 14.0;
const BODY_GAP: f32 = 10.0;
const STAGE_TICK: Duration = Duration::from_millis(20);

#[derive(Debug, Default)]
struct LoadingSessionGate {
    window: Option<WindowHandle<LoadingSessionWindow>>,
}

impl Global for LoadingSessionGate {}

macro_rules! session_log {
    ($($arg:tt)*) => {
        eprintln!("[SessionLoad] {}", format!($($arg)*))
    };
}

/// Decoded project payload handed to a freshly mounted [`crate::layout::StudioLayout`].
#[derive(Debug, Clone)]
pub struct LoadedSessionPackage {
    pub project: FutureboardProject,
    pub path: PathBuf,
    pub open_options: ProjectOpenOptions,
}

/// Snapshot captured before replacing an in-flight studio session so a failed
/// open can restore the previous project without mounting partial state.
#[derive(Debug, Clone)]
pub struct SessionRollbackSnapshot {
    pub timeline_state: TimelineState,
    pub session: ProjectSession,
    pub project_state: crate::app_state::ProjectState,
}

pub struct LoadFailedContext {
    pub title: String,
    pub message: String,
    pub detail: Option<String>,
    pub path: Option<PathBuf>,
    pub open_options: ProjectOpenOptions,
    pub rollback: Option<SessionRollbackSnapshot>,
}

pub type LoadSuccessCb = Arc<dyn Fn(LoadedSessionPackage, &mut App) + Send + Sync>;
pub type LoadFailedCb = Arc<dyn Fn(LoadFailedContext, &mut App) + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoadStage {
    Validate,
    Decode,
    Finalize,
}

impl LoadStage {
    fn label(self) -> &'static str {
        match self {
            LoadStage::Validate => "Validating project file",
            LoadStage::Decode => "Reading project data",
            LoadStage::Finalize => "Preparing session",
        }
    }

    fn progress(self) -> ProgressBarValue {
        match self {
            LoadStage::Validate => ProgressBarValue::value(0.2),
            LoadStage::Decode => ProgressBarValue::value(0.55),
            LoadStage::Finalize => ProgressBarValue::value(0.9),
        }
    }
}

struct SessionLoadTransaction {
    path: PathBuf,
    open_options: ProjectOpenOptions,
    rollback: Option<SessionRollbackSnapshot>,
    stage: LoadStage,
    project: Option<FutureboardProject>,
    on_success: LoadSuccessCb,
    on_failure: LoadFailedCb,
}

pub struct LoadingSessionWindow {
    heading: SharedString,
    detail: SharedString,
    progress: ProgressBarValue,
    footer: SharedString,
    focus_handle: FocusHandle,
    transaction: Option<SessionLoadTransaction>,
}

impl LoadingSessionWindow {
    fn new(
        session_name: Option<String>,
        transaction: SessionLoadTransaction,
        cx: &mut Context<Self>,
    ) -> Self {
        let heading = session_name
            .filter(|name| !name.is_empty())
            .map(|name| format!("Loading {name}"))
            .unwrap_or_else(|| "Loading Session…".to_string());
        Self {
            heading: heading.into(),
            detail: LoadStage::Validate.label().into(),
            progress: LoadStage::Validate.progress(),
            footer: "This can take a moment for large sessions.".into(),
            focus_handle: cx.focus_handle(),
            transaction: Some(transaction),
        }
    }

    fn set_stage(&mut self, stage: LoadStage, cx: &mut Context<Self>) {
        self.detail = stage.label().into();
        self.progress = stage.progress();
        cx.notify();
    }

    fn set_detail(&mut self, detail: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.detail = detail.into();
        cx.notify();
    }

    fn schedule_tick(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(STAGE_TICK).await;
            let _ = this.update(cx, |this, cx| this.advance(cx));
        })
        .detach();
    }

    fn advance(&mut self, cx: &mut Context<Self>) {
        let Some(stage) = self.transaction.as_ref().map(|load| load.stage) else {
            return;
        };
        session_log!("stage: {}", stage.label());
        self.set_stage(stage, cx);

        match stage {
            LoadStage::Validate => {
                let path = self.transaction.as_ref().unwrap().path.clone();
                if !path.exists() {
                    self.finish_failure(
                        "Open Project Failed",
                        "The project file could not be found at the saved location.",
                        Some(format!("Details: {}", path.display())),
                        cx,
                    );
                    return;
                }
                match validate_project_file(&path) {
                    Ok(version) => {
                        session_log!("project schema version={version}");
                        if let Some(load) = self.transaction.as_mut() {
                            load.stage = LoadStage::Decode;
                        }
                        self.schedule_tick(cx);
                    }
                    Err(e) => {
                        session_log!("header validation failed: {}", e.technical_detail());
                        self.finish_failure(
                            "Open Project Failed",
                            e.user_message(),
                            Some(format!("Details: {}", e.technical_detail())),
                            cx,
                        );
                    }
                }
            }
            LoadStage::Decode => {
                self.set_detail("Loading project file", cx);
                let path = self.transaction.as_ref().unwrap().path.clone();
                let this = cx.entity().clone();
                cx.spawn(async move |_entity, cx| {
                    let decoded = cx
                        .background_executor()
                        .spawn(async move { load_project(&path) })
                        .await;
                    let _ = this.update(cx, |this, cx| this.on_decode_complete(decoded, cx));
                })
                .detach();
            }
            LoadStage::Finalize => {
                let Some(mut transaction) = self.transaction.take() else {
                    return;
                };
                let Some(project) = transaction.project.take() else {
                    self.transaction = Some(transaction);
                    self.finish_failure(
                        "Open Project Failed",
                        "The project file could not be restored into the session.",
                        Some("Decoded project data was missing.".to_string()),
                        cx,
                    );
                    return;
                };
                let package = LoadedSessionPackage {
                    project,
                    path: transaction.path,
                    open_options: transaction.open_options,
                };
                let on_success = transaction.on_success;
                // Hand off to the shell — it must open Studio and retain the
                // window handle before closing this loader (never reach 0 windows).
                cx.defer(move |cx| {
                    on_success(package, cx);
                });
            }
        }
    }

    fn on_decode_complete(
        &mut self,
        decoded: Result<FutureboardProject, crate::project::ProjectError>,
        cx: &mut Context<Self>,
    ) {
        let Some(load) = self.transaction.as_mut() else {
            return;
        };
        match decoded {
            Ok(project) => {
                let track_count = project.tracks.len();
                let clip_count: usize = project.tracks.iter().map(|t| t.clips.len()).sum();
                session_log!("decoded: tracks={track_count} clips={clip_count}");
                load.project = Some(project);
                load.stage = LoadStage::Finalize;
                self.schedule_tick(cx);
            }
            Err(e) => {
                session_log!("decode failed: {}", e.technical_detail());
                self.finish_failure(
                    "Open Project Failed",
                    e.user_message(),
                    Some(format!("Details: {}", e.technical_detail())),
                    cx,
                );
            }
        }
    }

    fn finish_failure(
        &mut self,
        title: &str,
        message: &str,
        detail: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(transaction) = self.transaction.take() else {
            return;
        };
        let ctx = LoadFailedContext {
            title: title.to_string(),
            message: message.to_string(),
            detail,
            path: Some(transaction.path),
            open_options: transaction.open_options,
            rollback: transaction.rollback,
        };
        let on_failure = transaction.on_failure;
        // Defer so we never invoke failure handling from inside our own update.
        // The shell closes this window only after a replacement surface exists.
        cx.defer(move |cx| {
            on_failure(ctx, cx);
        });
    }

    fn show_terminal_error(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.heading = "Open Failed".into();
        self.detail = message.into();
        self.progress = ProgressBarValue::Indeterminate;
        self.footer = "The project could not be opened.".into();
        cx.notify();
    }
}

/// Close the loading-session window. Call only after a replacement window
/// (Studio or Welcome) is open and its handle is retained in app state.
pub fn close_loading_session_window(cx: &mut App) {
    if !cx.has_global::<LoadingSessionGate>() {
        return;
    }
    session_log!("closing loading window");
    cx.update_global::<LoadingSessionGate, _>(|gate, cx| {
        if let Some(handle) = gate.window.take() {
            let _ = handle.update(cx, |_view, window, _cx| window.remove_window());
        }
    });
}

/// Show a terminal error on the loading window instead of closing the app.
pub fn show_loading_session_error(cx: &mut App, message: impl Into<String>) {
    if !cx.has_global::<LoadingSessionGate>() {
        return;
    }
    let message = message.into();
    cx.update_global::<LoadingSessionGate, _>(|gate, cx| {
        if let Some(handle) = gate.window.as_ref() {
            let msg = message.clone();
            let _ = handle.update(cx, |window, _win, cx| window.show_terminal_error(msg, cx));
        }
    });
}

fn store_loading_session_window(cx: &mut App, handle: WindowHandle<LoadingSessionWindow>) {
    if cx.has_global::<LoadingSessionGate>() {
        cx.update_global::<LoadingSessionGate, _>(|gate, _| gate.window = Some(handle));
    } else {
        cx.set_global(LoadingSessionGate {
            window: Some(handle),
        });
    }
}

impl Render for LoadingSessionWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let heading = self.heading.clone();
        let detail = self.detail.clone();
        let progress = self.progress;
        let footer = self.footer.clone();

        div()
            .flex()
            .flex_col()
            .size_full()
            .font(theme::ui_font())
            .bg(Colors::surface_base())
            .overflow_hidden()
            .rounded_md()
            .border(px(1.0))
            .border_color(Colors::border_subtle())
            .shadow(vec![gpui::BoxShadow {
                color: Colors::surface_overlay().into(),
                offset: gpui::point(px(0.0), px(6.0)),
                blur_radius: px(20.0),
                spread_radius: px(0.0),
                inset: false,
            }])
            .child(div().w(px(0.0)).h(px(0.0)).track_focus(&self.focus_handle))
            .child(external_window_titlebar_compact(
                "Loading Session".to_string(),
                "loading-session-close",
                |_window, _cx| {
                    // Non-closable while loading — the transaction owns lifecycle.
                },
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .px(px(BODY_PAD_X))
                    .py(px(BODY_PAD_Y))
                    .gap(px(BODY_GAP))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(Colors::text_primary())
                            .child(heading),
                    )
                    .child(
                        div()
                            .text_size(px(10.0))
                            .line_height(px(15.0))
                            .text_color(Colors::text_muted())
                            .child(detail),
                    )
                    .child(progress_bar(progress))
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(Colors::text_faint())
                            .child(footer),
                    ),
            )
    }
}

fn set_app_mode(cx: &mut App, mode: AppMode) {
    if cx.has_global::<AppSessionGate>() {
        cx.update_global::<AppSessionGate, _>(|gate, _| gate.mode = mode);
    } else {
        cx.set_global(AppSessionGate { mode });
    }
}

fn run_headless_load(
    path: PathBuf,
    open_options: ProjectOpenOptions,
    rollback: Option<SessionRollbackSnapshot>,
    on_success: &LoadSuccessCb,
    on_failure: &LoadFailedCb,
    cx: &mut App,
) {
    if !path.exists() {
        on_failure(
            LoadFailedContext {
                title: "Open Project Failed".to_string(),
                message: "The project file could not be found at the saved location.".to_string(),
                detail: Some(format!("Details: {}", path.display())),
                path: Some(path),
                open_options,
                rollback,
            },
            cx,
        );
        return;
    }
    match validate_project_file(&path) {
        Ok(_) => match load_project(&path) {
            Ok(project) => on_success(
                LoadedSessionPackage {
                    project,
                    path,
                    open_options,
                },
                cx,
            ),
            Err(e) => on_failure(
                LoadFailedContext {
                    title: "Open Project Failed".to_string(),
                    message: e.user_message().to_string(),
                    detail: Some(format!("Details: {}", e.technical_detail())),
                    path: Some(path),
                    open_options,
                    rollback,
                },
                cx,
            ),
        },
        Err(e) => on_failure(
            LoadFailedContext {
                title: "Open Project Failed".to_string(),
                message: e.user_message().to_string(),
                detail: Some(format!("Details: {}", e.technical_detail())),
                path: Some(path),
                open_options,
                rollback,
            },
            cx,
        ),
    }
}

/// Begin a pre-studio project open. Shows the loading window immediately and
/// only invokes `on_success` after decode/validate succeed.
pub fn begin_project_session_load(
    path: PathBuf,
    open_options: ProjectOpenOptions,
    rollback: Option<SessionRollbackSnapshot>,
    owner_bounds: Option<Bounds<Pixels>>,
    on_success: LoadSuccessCb,
    on_failure: LoadFailedCb,
    cx: &mut App,
) {
    set_app_mode(cx, AppMode::LoadingSession);
    session_log!("begin pre-studio load: {}", path.display());

    let session_name = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::to_string);

    let rollback_for_headless = rollback.clone();
    let transaction = SessionLoadTransaction {
        path: path.clone(),
        open_options,
        rollback,
        stage: LoadStage::Validate,
        project: None,
        on_success: on_success.clone(),
        on_failure: on_failure.clone(),
    };

    match open_loading_session_window(session_name, transaction, owner_bounds, cx) {
        Ok(handle) => {
            store_loading_session_window(cx, handle.clone());
            let _ = handle.update(cx, |window, _win, cx| {
                window.schedule_tick(cx);
            });
        }
        Err(err) => {
            session_log!("loading window unavailable: {err}");
            run_headless_load(
                path,
                open_options,
                rollback_for_headless,
                &on_success,
                &on_failure,
                cx,
            );
        }
    }
}

#[cfg(target_os = "windows")]
fn open_loading_session_window(
    session_name: Option<String>,
    transaction: SessionLoadTransaction,
    owner_bounds: Option<Bounds<Pixels>>,
    cx: &mut App,
) -> Result<WindowHandle<LoadingSessionWindow>, String> {
    use crate::window_position::{apply_owner_display, centered_window_bounds};
    use gpui::{size, WindowBackgroundAppearance, WindowBounds, WindowKind};

    let height = LOAD_WINDOW_HEIGHT + TITLEBAR_HEIGHT;
    let window_bounds = centered_window_bounds(
        owner_bounds,
        size(px(LOAD_WINDOW_WIDTH), px(height)),
        cx,
    );

    let mut window_options = crate::platform_chrome::external_dialog_window_options_partial();
    window_options.window_bounds = Some(WindowBounds::Windowed(window_bounds));
    window_options.kind = WindowKind::Floating;
    window_options.is_resizable = false;
    window_options.is_minimizable = false;
    window_options.window_background = WindowBackgroundAppearance::Transparent;
    apply_owner_display(&mut window_options, owner_bounds, cx);

    cx.open_window(window_options, move |_window, cx| {
        cx.new(|cx| LoadingSessionWindow::new(session_name, transaction, cx))
    })
    .map_err(|e| e.to_string())
}

#[cfg(not(target_os = "windows"))]
fn open_loading_session_window(
    _session_name: Option<String>,
    _transaction: SessionLoadTransaction,
    _owner_bounds: Option<Bounds<Pixels>>,
    _cx: &mut App,
) -> Result<WindowHandle<LoadingSessionWindow>, String> {
    Err("native loading session window is only available on Windows".to_string())
}
