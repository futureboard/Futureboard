use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::window::{studio_window_options, welcome_window_options};
use gpui::{App, AppContext};
use sphere_ui_components::assets;
use sphere_ui_components::boot;
use sphere_ui_components::layout::StudioLayout;
use sphere_ui_components::project::ProjectTemplate;
use sphere_ui_components::welcome::{WelcomeAction, WelcomeCallbacks, WelcomeWindow};

pub fn setup(cx: &mut App) {
    // Fonts must be registered before the first native view renders.
    boot::log("register fonts");
    assets::register_fonts(cx);
    boot::log("fonts registered");

    // Apply the saved renderer preference now (before any window) so the GPU
    // renderer can be warmed during the loading screen rather than stalling the
    // first studio frame.
    sphere_ui_components::layout::apply_saved_renderer_preference(cx);
    boot::log("renderer preference applied");

    // Launch flow: Splash / Loading -> Welcome.
    open_welcome_window(cx, false);
}

/// Open (or re-open) the Welcome window. This is the fallback route whenever no
/// project is open — at launch and after `Close Project`.
///
/// When `skip_splash` is true the window jumps straight to the Welcome screen
/// (used when returning from a closed project); otherwise it plays the startup
/// splash/loading sequence.
fn open_welcome_window(cx: &mut App, skip_splash: bool) {
    let callbacks = WelcomeCallbacks {
        on_action: Arc::new(|action, welcome_window, cx| {
            open_studio_for_action(action, cx);
            // Welcome -> Workspace: the workspace is its own window, so close
            // Welcome once the studio is up.
            welcome_window.remove_window();
        }),
    };
    let welcome = cx
        .open_window(welcome_window_options(cx), |_window, cx| {
            cx.new(|_| WelcomeWindow::new(env!("CARGO_PKG_VERSION"), callbacks))
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
            let backend = sphere_ui_components::layout::warm_up_renderer();
            welcome.set_gpu_status(format!("Ready · {backend}"));
            welcome.set_loading_status("Ready");
            cx.notify();
        });

        executor.timer(Duration::from_millis(120)).await;
        let _ = welcome.update(cx, |welcome, _window, cx| {
            welcome.show_welcome();
            cx.notify();
        });
    })
    .detach();
}

/// What the freshly opened workspace should do once its window exists.
enum WorkspaceInit {
    /// Blank, unsaved project.
    Empty,
    /// New unsaved project pre-populated from a template.
    Template(ProjectTemplate),
    /// Show the native Open Project file picker.
    OpenDialog,
    /// Load a specific recent/existing project file.
    Load(PathBuf),
}

fn open_studio_for_action(action: WelcomeAction, cx: &mut App) {
    let init = match action {
        // Empty Project / Open Empty Workspace -> blank unsaved workspace.
        WelcomeAction::EmptyProject | WelcomeAction::OpenEmptyWorkspace => WorkspaceInit::Empty,
        // Template sessions create template-backed (still unsaved) workspaces.
        // TODO: richer presets (inserts, sends, master chain) once the template
        // system lands; for now these only seed tempo + an initial track layout.
        WelcomeAction::MidiComposer => WorkspaceInit::Template(ProjectTemplate::BeatMaking),
        WelcomeAction::AudioSession => WorkspaceInit::Template(ProjectTemplate::Recording),
        WelcomeAction::MixTemplate => WorkspaceInit::Template(ProjectTemplate::Mixing),
        WelcomeAction::OpenProject => WorkspaceInit::OpenDialog,
        WelcomeAction::OpenRecent(path) => WorkspaceInit::Load(path),
    };

    let studio = cx
        .open_window(studio_window_options(), |window, cx| {
            boot::log("build StudioLayout");
            let layout = cx.new(StudioLayout::new);
            boot::log("StudioLayout built");

            // WCO / OS window close → quit the app, but go through the
            // unsaved-changes guard first. Returning `false` always prevents
            // GPUI's default close: `request_quit` drives `cx.quit()` only once
            // the user confirms, so Cancel keeps the app open and the close
            // never routes to Welcome.
            let weak = layout.downgrade();
            window.on_window_should_close(cx, move |window, cx| {
                let bounds = window.bounds();
                let _ = weak.update(cx, |studio, cx| {
                    studio.request_quit(Some(bounds), cx);
                });
                false
            });

            window.activate_window();
            layout
        })
        .expect("failed to open studio window");

    let studio_handle = studio;
    let _ = studio.update(cx, move |layout, _window, cx| {
        // Wire the workspace lifecycle: its own window handle (so Close Project
        // can close it) and the hook that re-opens Welcome.
        layout.set_self_window(studio_handle);
        layout.set_request_welcome_callback(Arc::new(|cx| open_welcome_window(cx, true)));

        match init {
            WorkspaceInit::Empty => layout.new_empty_project(cx),
            WorkspaceInit::Template(template) => layout.new_project_from_template(template, cx),
            WorkspaceInit::OpenDialog => layout.dispatch_command_id("project:open", cx),
            WorkspaceInit::Load(path) => layout.load_project_from_path(path, cx),
        }
    });
}
