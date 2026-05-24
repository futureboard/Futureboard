#![windows_subsystem = "windows"]

mod app;
mod audio_state;
mod window;

use sphere_ui_components::embedded_assets::EmbeddedAssets;

fn main() {
    // Build the native audio facade up front. Stage 1 wiring: own the
    // engine handle and enumerate devices, but do not open the OS audio
    // stream — playback scheduling is not wired yet.
    let mut audio = audio_state::NativeAudioState::new();
    match audio.initialize_engine() {
        Ok(()) => {
            let version = audio.version().unwrap_or_else(|| "?".into());
            eprintln!(
                "[audio] sphere-direct-audio-engine v{} ready (backend={:?}, sr={}, buf={})",
                version, audio.config.backend, audio.config.sample_rate, audio.config.buffer_size,
            );
            let devices = audio.list_devices();
            eprintln!("[audio] {} output device(s) discovered", devices.len());
            for d in devices.iter().take(8) {
                eprintln!(
                    "[audio]   - {} ({} ch @ {} Hz){}",
                    d.name,
                    d.channels,
                    d.default_sample_rate,
                    if d.is_default { "  [default]" } else { "" }
                );
            }
        }
        Err(e) => {
            eprintln!("[audio] failed to initialize engine: {e}");
        }
    }

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
