use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::window::{splash_window_options, studio_window_options};
use gpui::{App, AppContext};
use sphere_ui_components::assets;
use sphere_ui_components::boot;
use sphere_ui_components::layout::StudioLayout;
use sphere_ui_components::splash::SplashWindow;

pub fn setup(cx: &mut App) {
    // Fonts must be registered before the splash so its status text renders.
    boot::log("register fonts");
    assets::register_fonts(cx);
    boot::log("fonts registered");

    // ── Phase 2 (entry) — show the splash immediately ─────────────────────────
    // The user gets visual feedback at once; the heavy critical init runs after
    // the splash has painted (below), so it never freezes a blank window.
    let splash = cx
        .open_window(splash_window_options(cx), |_window, cx| {
            cx.new(|_| SplashWindow::new("Starting…"))
        })
        .expect("failed to open splash window");
    boot::log("splash window shown");

    // Background executor for the inter-phase yields (lets the splash repaint
    // each status before the next, blocking, phase runs).
    let timers = cx.background_executor().clone();

    cx.spawn(async move |cx| {
        // Status: graphics. A short beat so it is actually visible.
        let _ = splash.update(cx, |s, _w, cx| {
            s.set_status("Initializing graphics…");
            cx.notify();
        });
        timers.timer(Duration::from_millis(140)).await;

        // Status: audio. Yield one frame so this paints *before* the blocking
        // engine init inside StudioLayout::new.
        let _ = splash.update(cx, |s, _w, cx| {
            s.set_status("Initializing audio…");
            cx.notify();
        });
        timers.timer(Duration::from_millis(32)).await;

        // Reveal the main window exactly once, whichever path fires first.
        let revealed = Arc::new(AtomicBool::new(false));
        let revealed_frame = revealed.clone();
        let splash_frame = splash;

        // ── Phase 1 — critical init: build main window HIDDEN + StudioLayout ──
        let main = cx
            .update(|cx| {
                let mut options = studio_window_options();
                options.show = false;
                options.focus = false;
                cx.open_window(options, move |window, cx| {
                    boot::log("build StudioLayout");
                    let layout = cx.new(|cx| StudioLayout::new(cx));
                    boot::log("StudioLayout built");
                    // Reveal main + close splash once the first frame is painted.
                    window.on_next_frame(move |window, cx| {
                        if !revealed_frame.swap(true, Ordering::SeqCst) {
                            boot::log("main window show");
                            window.activate_window();
                            let _ = splash_frame.update(cx, |_, w, _| w.remove_window());
                            boot::log("background tasks start");
                        }
                    });
                    layout
                })
                .expect("failed to open studio window")
            })
            .ok();

        let _ = splash.update(cx, |s, _w, cx| {
            s.set_status("Loading workspace…");
            cx.notify();
        });

        // Safety net: if the hidden main window never paints (e.g. driver quirk
        // on the DComp-disabled path), reveal it and close the splash anyway so
        // startup can never get stuck on the splash.
        timers.timer(Duration::from_millis(2000)).await;
        let _ = cx.update(|cx| {
            if !revealed.swap(true, Ordering::SeqCst) {
                boot::log("main window show (fallback timer)");
                if let Some(main) = main {
                    let _ = main.update(cx, |_, w, _| w.activate_window());
                }
                let _ = splash.update(cx, |_, w, _| w.remove_window());
            }
        });
    })
    .detach();

    boot::log("setup returned (boot continues async)");
}
