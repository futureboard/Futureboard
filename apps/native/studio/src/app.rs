use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::window::{studio_window_options, welcome_window_options};
use gpui::{App, AppContext, BorrowAppContext, Global, WindowHandle};
use sphere_ui_components::app_state::{AppMode, AppSessionGate, StudioRoute};
use sphere_ui_components::assets;
use sphere_ui_components::boot;
use sphere_ui_components::layout::warm_up_renderer_status;
use sphere_ui_components::layout::{PendingCloseAction, ProjectOpenOptions, StudioLayout};
use sphere_ui_components::loading_session::{
    begin_project_session_load, close_loading_session_window, show_loading_session_error,
    LoadedSessionPackage, LoadFailedContext, SessionRollbackSnapshot,
};
use sphere_ui_components::project::{ProjectCreateOptions, ProjectTemplate};
use sphere_ui_components::settings::SettingsSchema;
use sphere_ui_components::welcome::{WelcomeAction, WelcomeCallbacks, WelcomeWindow};

/// Retains the studio window handle at app scope so GPUI never drops the last
/// window during LoadingSession → Studio transitions.
#[derive(Default)]
struct NativeShellWindows {
    studio: Option<WindowHandle<StudioLayout>>,
}

impl Global for NativeShellWindows {}

pub fn setup(cx: &mut App) {
    boot::log("app boot start");
    cx.set_global(AppSessionGate {
        mode: AppMode::Welcome,
    });
    cx.set_global(NativeShellWindows::default());

    // Fonts must be registered before the first native view renders.
    assets::register_fonts(cx);
    boot::log("fonts registered");

    // Apply the saved renderer preference now (before any window) so the GPU
    // renderer can be warmed during the loading screen rather than stalling the
    // first studio frame.
    sphere_ui_components::layout::apply_saved_renderer_preference(cx);
    boot::log("renderer preference applied");

    // Startup route honors the "Show start screen on launch" preference (Part G).
    let show_welcome = SettingsSchema::load_from_disk().general.show_start_screen;
    let route = StudioRoute::from_show_welcome(show_welcome);
    boot::log(&format!("startup route: {}", route.label()));

    match route {
        // Launch flow: Splash / Loading -> Welcome (renderer warms during splash).
        StudioRoute::Welcome => open_welcome_window(cx, false),
        // Welcome disabled: warm the renderer now, then boot straight into an
        // empty unsaved workspace. Welcome stays reachable via File → Close
        // Project.
        StudioRoute::StudioWorkspace => {
            let warm = warm_up_renderer_status();
            boot::log(&format!(
                "renderer warm (no-welcome): {} [{}]",
                warm.status_text(),
                warm.backend_label
            ));
            open_studio_for_action(WelcomeAction::OpenEmptyWorkspace, cx);
            boot::log("workspace entered (welcome disabled)");
        }
    }
}

fn set_app_mode(cx: &mut App, mode: AppMode) {
    cx.update_global::<AppSessionGate, _>(|gate, _| gate.mode = mode);
}

fn store_studio_window_handle(cx: &mut App, handle: WindowHandle<StudioLayout>) {
    cx.update_global::<NativeShellWindows, _>(|shell, _| {
        shell.studio = Some(handle);
    });
    eprintln!("[StudioOpen] stored studio window handle");
}

fn finish_loading_to_studio(cx: &mut App) {
    close_loading_session_window(cx);
    eprintln!("[SessionLoad] studio ready");
}

/// Open (or re-open) the Welcome window. This is the fallback route whenever no
/// project is open — at launch and after `Close Project`.
///
/// When `skip_splash` is true the window jumps straight to the Welcome screen
/// (used when returning from a closed project); otherwise it plays the startup
/// splash/loading sequence.
fn open_welcome_window(cx: &mut App, skip_splash: bool) {
    set_app_mode(cx, AppMode::Welcome);
    let callbacks = WelcomeCallbacks {
        on_action: Arc::new(|action, welcome_window, cx| {
            match action {
                WelcomeAction::OpenProjectFile(path) => {
                    welcome_window.remove_window();
                    begin_load_project_from_welcome(path, ProjectOpenOptions::default(), cx);
                }
                WelcomeAction::OpenRecent(path) => {
                    welcome_window.remove_window();
                    begin_load_project_from_welcome(
                        path,
                        ProjectOpenOptions {
                            from_recent: true,
                        },
                        cx,
                    );
                }
                other => {
                    open_studio_for_action(other, cx);
                    // Welcome -> Workspace: the workspace is its own window, so close
                    // Welcome once the studio is up.
                    welcome_window.remove_window();
                }
            }
        }),
    };
    let welcome = cx
        .open_window(welcome_window_options(cx), |_window, cx| {
            cx.new(|cx| WelcomeWindow::new(env!("CARGO_PKG_VERSION"), callbacks, cx.focus_handle()))
        })
        .expect("failed to open welcome window");
    boot::log("welcome window shown");

    if skip_splash {
        let _ = welcome.update(cx, |welcome, _window, cx| {
            welcome.show_welcome();
            cx.notify();
        });
        return;
    }

    let executor = cx.background_executor().clone();
    cx.spawn(async move |cx| {
        for status in [
            "Loading shared assets",
            "Loading recent projects",
            "Preparing native workspace",
        ] {
            executor.timer(Duration::from_millis(150)).await;
            let _ = welcome.update(cx, |welcome, _window, cx| {
                welcome.set_loading_status(status);
                cx.notify();
            });
        }

        // Real device initialization. Audio + MIDI are scanned into the process
        // device registry so Preferences renders real devices (no mocks) and
        // never scans on a render path. Scans run on the background executor;
        // failures are non-fatal (empty registry + warning) so startup always
        // continues into the workspace.
        let _ = welcome.update(cx, |welcome, _window, cx| {
            welcome.set_loading_status("Scanning audio devices");
            cx.notify();
        });
        boot::log("[Startup] phase=ScanAudio");
        executor
            .spawn(async {
                sphere_ui_components::device_registry::scan_audio();
            })
            .await;

        let _ = welcome.update(cx, |welcome, _window, cx| {
            welcome.set_loading_status("Scanning MIDI devices");
            cx.notify();
        });
        boot::log("[Startup] phase=ScanMidi");
        executor
            .spawn(async {
                sphere_ui_components::device_registry::scan_midi();
            })
            .await;

        // Warm the GPU renderer here — on the loading screen — so the first
        // studio frame doesn't stall on adapter/device creation. The warm-up
        // runs on the main thread (inside the window update) and reuses the
        // same thread-local renderer the studio paints with.
        executor.timer(Duration::from_millis(120)).await;
        let _ = welcome.update(cx, |welcome, _window, cx| {
            welcome.set_loading_status("Initializing GPU renderer");
            welcome.set_gpu_status("Initializing");
            cx.notify();
        });
        let _ = welcome.update(cx, |welcome, _window, cx| {
            let warm = sphere_ui_components::layout::warm_up_renderer_status();
            welcome.set_gpu_status(format!("{} · {}", warm.status_text(), warm.backend_label));
            welcome.set_loading_status("Ready");
            cx.notify();
        });
        boot::log("renderer warm-up complete (welcome)");

        executor.timer(Duration::from_millis(120)).await;
        let _ = welcome.update(cx, |welcome, _window, cx| {
            welcome.show_welcome();
            cx.notify();
        });
    })
    .detach();
}

fn transition_loaded_package_to_studio(package: LoadedSessionPackage, cx: &mut App) {
    eprintln!("[SessionLoad] transition: loading -> studio");
    set_app_mode(cx, AppMode::Studio);
    match open_studio_workspace(WorkspaceInit::Loaded(package), cx) {
        Ok(()) => finish_loading_to_studio(cx),
        Err(error) => {
            eprintln!("[StudioOpen] studio window open failed: {error}");
            set_app_mode(cx, AppMode::LoadFailed);
            show_loading_session_error(
                cx,
                format!("Could not open the studio workspace.\n\nDetails: {error}"),
            );
        }
    }
}

fn begin_load_project_from_welcome(
    path: PathBuf,
    open_options: ProjectOpenOptions,
    cx: &mut App,
) {
    let on_success = Arc::new(|package: LoadedSessionPackage, cx: &mut App| {
        transition_loaded_package_to_studio(package, cx);
    });
    let on_failure = Arc::new(|ctx: LoadFailedContext, cx: &mut App| {
        handle_load_failed(ctx, cx);
    });
    begin_project_session_load(path, open_options, None, None, on_success, on_failure, cx);
}

fn request_project_switch_in_studio(
    studio: gpui::WindowHandle<StudioLayout>,
    path: PathBuf,
    open_options: ProjectOpenOptions,
    cx: &mut App,
) {
    // Never call `studio.update` synchronously here — this hook is invoked from
    // inside an active `StudioLayout` update (e.g. File → Open Project).
    cx.defer(move |cx| {
        let _ = studio.update(cx, |layout, _window, cx| {
            layout.begin_in_studio_project_switch(path, open_options, cx);
        });
    });
}

fn handle_load_failed(ctx: LoadFailedContext, cx: &mut App) {
    eprintln!(
        "[SessionLoad] open failed: {} — {}",
        ctx.title, ctx.message
    );
    if let Some(snapshot) = ctx.rollback {
        set_app_mode(cx, AppMode::Studio);
        match open_studio_workspace(WorkspaceInit::Rollback(snapshot), cx) {
            Ok(()) => finish_loading_to_studio(cx),
            Err(error) => {
                eprintln!("[StudioOpen] rollback studio open failed: {error}");
                set_app_mode(cx, AppMode::LoadFailed);
                show_loading_session_error(
                    cx,
                    format!(
                        "{}\n\n{}\n\nRollback failed: {error}",
                        ctx.title, ctx.message
                    ),
                );
            }
        }
        return;
    }
    set_app_mode(cx, AppMode::LoadFailed);
    open_welcome_window(cx, true);
    close_loading_session_window(cx);
}

/// What the freshly opened workspace should do once its window exists.
enum WorkspaceInit {
    /// Blank, unsaved project.
    Empty,
    /// New unsaved project pre-populated from a template.
    Template(ProjectTemplate),
    /// Show the native Open Project file picker, then load through the gate.
    OpenDialog,
    /// Install a decoded project that was loaded before studio mounted.
    Loaded(LoadedSessionPackage),
    /// Restore a rollback snapshot after a failed in-studio replace.
    Rollback(SessionRollbackSnapshot),
    /// Create a named project on disk, then enter the saved workspace.
    CreateProject(ProjectCreateOptions),
}

fn open_studio_for_action(action: WelcomeAction, cx: &mut App) {
    let init = match action {
        // Empty Project / Open Empty Workspace -> blank unsaved workspace.
        WelcomeAction::EmptyProject | WelcomeAction::OpenEmptyWorkspace => WorkspaceInit::Empty,
        // Template sessions create template-backed (still unsaved) workspaces.
        WelcomeAction::MidiComposer => WorkspaceInit::Template(ProjectTemplate::BeatMaking),
        WelcomeAction::AudioSession => WorkspaceInit::Template(ProjectTemplate::Recording),
        WelcomeAction::MixTemplate => WorkspaceInit::Template(ProjectTemplate::Mixing),
        WelcomeAction::OpenProject => WorkspaceInit::OpenDialog,
        // Handled before studio mount in the Welcome callback.
        WelcomeAction::OpenProjectFile(_) | WelcomeAction::OpenRecent(_) => return,
        WelcomeAction::CreateProject(options) => WorkspaceInit::CreateProject(options),
    };
    set_app_mode(cx, AppMode::Studio);
    let _ = open_studio_workspace(init, cx);
}

fn open_studio_workspace(init: WorkspaceInit, cx: &mut App) -> Result<(), String> {
    if !cx
        .try_global::<AppSessionGate>()
        .map(|g| g.mode.allows_studio_mount())
        .unwrap_or(false)
    {
        boot::log("studio mount blocked — app mode is not Studio");
        return Err("app mode is not Studio".to_string());
    }

    eprintln!("[StudioOpen] opening studio window");
    let studio_options = studio_window_options(cx);
    let studio = cx
        .open_window(studio_options, |window, cx| {
            boot::log("build StudioLayout");
            let layout = cx.new(StudioLayout::new);
            boot::log("StudioLayout built");

            // WCO / OS window close → quit the app, but go through the
            // unsaved-changes guard first. Returning `false` always prevents
            // GPUI's default close: `request_quit` drives `cx.quit()` only once
            // the user confirms, so Cancel keeps the app open and the close
            // never routes to Welcome.
            let studio_entity = layout.clone();
            window.on_window_should_close(cx, move |window, cx| {
                sphere_ui_components::window_position::persist_studio_window_from_window(
                    window, cx,
                );
                let bounds = window.bounds();
                studio_entity.update(cx, |studio, cx| {
                    studio.request_close(PendingCloseAction::QuitApp, Some(bounds), cx);
                });
                // Always veto the platform close; confirmed quit runs via `do_quit`.
                false
            });

            window.activate_window();
            layout
        })
        .map_err(|e| e.to_string())?;
    eprintln!("[StudioOpen] studio window opened");

    let studio_handle = studio.clone();
    store_studio_window_handle(cx, studio_handle.clone());

    studio
        .update(cx, move |layout, _window, cx| {
            // Wire the workspace lifecycle: its own window handle (so Close Project
            // can close it) and the hook that re-opens Welcome.
            layout.set_self_window(studio_handle.clone());
            layout.set_request_welcome_callback(Arc::new(|cx| open_welcome_window(cx, true)));
            layout.set_request_project_load_callback(Arc::new({
                let studio_handle = studio_handle.clone();
                move |path, open_options, cx| {
                    request_project_switch_in_studio(
                        studio_handle.clone(),
                        path,
                        open_options,
                        cx,
                    );
                }
            }));

            match init {
                WorkspaceInit::Empty => layout.new_empty_project(cx),
                WorkspaceInit::Template(template) => layout.new_project_from_template(template, cx),
                WorkspaceInit::OpenDialog => layout.dispatch_command_id("project:open", cx),
                WorkspaceInit::Loaded(package) => layout.install_loaded_session(package, cx),
                WorkspaceInit::Rollback(snapshot) => {
                    layout.restore_session_rollback_snapshot(snapshot, cx)
                }
                WorkspaceInit::CreateProject(options) => {
                    layout.create_saved_project_from_options(options, cx)
                }
            }
        })
        .map_err(|e| e.to_string())?;

    boot::log("workspace entered");
    Ok(())
}
