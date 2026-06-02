#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio_state;
mod window;

use sphere_ui_components::boot;
use sphere_ui_components::embedded_assets::EmbeddedAssets;

fn main() {
    // ── Phase 0 — process setup ───────────────────────────────────────────────
    // env flags (before GPUI/window creation), panic hook, logging. No window,
    // no settings I/O, no device/plugin work here.
    boot::log("process setup start");

    // Catch any panic that escapes the GPUI render loop so we see *why*
    // the window blanks out instead of getting a silent crash.
    std::panic::set_hook(Box::new(|info| {
        eprintln!("[panic] {info}");
        let bt = std::backtrace::Backtrace::force_capture();
        eprintln!("[panic] backtrace:\n{bt}");
    }));

    // GPUI's default DirectComposition target is created with topmost=true, which
    // draws above all WS_CHILD HWNDs. Plugin editors embed VST3 UI as children of
    // the GPUI window; without this, transparent windows show the DAW behind them
    // instead of the native plugin. Disabling DComp lets child HWNDs composite
    // above the swap chain. MUST be set before GPUI creates any window.
    #[cfg(target_os = "windows")]
    if std::env::var_os("GPUI_DISABLE_DIRECT_COMPOSITION").is_none() {
        std::env::set_var("GPUI_DISABLE_DIRECT_COMPOSITION", "1");
        boot::log("GPUI_DISABLE_DIRECT_COMPOSITION=1 (plugin editor HWND embedding)");
    }

    boot::log("process setup done");
    application()
        .with_assets(EmbeddedAssets::new())
        .run(app::setup);
    boot::log("gpui application exited");
}

/// Builds a GPUI [`Application`] with the correct OS platform backend.
///
/// The vendored standalone gpui removed `Application::new()`; the platform must
/// now be constructed explicitly. We mirror `gpui_platform::current_platform`
/// here instead of depending on `gpui_platform`, because that crate
/// force-enables gpui's `windows-manifest` feature, which would embed a second
/// application manifest and collide (CVT1100) with this binary's own manifest
/// from `app.rc`.
fn application() -> gpui::Application {
    #[cfg(target_os = "windows")]
    let platform: std::rc::Rc<dyn gpui::Platform> = std::rc::Rc::new(
        gpui_windows::WindowsPlatform::new(false)
            .expect("failed to initialize Windows platform"),
    );

    #[cfg(target_os = "macos")]
    let platform: std::rc::Rc<dyn gpui::Platform> =
        std::rc::Rc::new(gpui_macos::MacPlatform::new(false));

    #[cfg(target_os = "linux")]
    let platform: std::rc::Rc<dyn gpui::Platform> = gpui_linux::current_platform(false);

    gpui::Application::with_platform(platform)
}
