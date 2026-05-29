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
    gpui::Application::new()
        .with_assets(EmbeddedAssets::new())
        .run(app::setup);
    boot::log("gpui application exited");
}
