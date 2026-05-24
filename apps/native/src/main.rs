#![windows_subsystem = "windows"]

mod app;
mod audio_state;
mod window;

use sphere_ui_components::embedded_assets::EmbeddedAssets;

fn main() {
    // Catch any panic that escapes the GPUI render loop so we see *why*
    // the window blanks out instead of getting a silent crash.
    std::panic::set_hook(Box::new(|info| {
        eprintln!("[panic] {info}");
        let bt = std::backtrace::Backtrace::force_capture();
        eprintln!("[panic] backtrace:\n{bt}");
    }));

    eprintln!("[boot] gpui application starting");
    gpui::Application::new()
        .with_assets(EmbeddedAssets::new())
        .run(app::setup);
    eprintln!("[boot] gpui application exited");
}
