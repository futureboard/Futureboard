#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio_state;
mod window;

#[cfg(feature = "exclusive")]
mod exclusive_edition;

use sphere_ui_components::boot;
use sphere_ui_components::embedded_assets::EmbeddedAssets;

fn main() {
    #[cfg(feature = "exclusive")]
    exclusive_edition::install().expect("failed to install Exclusive Edition providers");

    // ── Phase 0 — process setup ───────────────────────────────────────────────
    // env flags (before GPUI/window creation), panic hook, logging. No window,
    // no settings I/O, no device/plugin work here.
    boot::log("process setup start");
    eprintln!(
        "[process] role=main pid={} exe=futureboard_native",
        std::process::id()
    );
    // Plugin runtime selection diagnostics. External PluginHost bridge is the
    // default; legacy in-process VST3 requires FUTUREBOARD_PLUGIN_LEGACY_IN_PROCESS=1.
    sphere_ui_components::plugin_host_client::log_bridge_env();
    let soundfont_backend =
        sphere_ui_components::soundfont_player::soundfont_player_backend_status();
    boot::log(&format!(
        "soundfont player backend: {} available={}",
        soundfont_backend.backend, soundfont_backend.available
    ));
    sphere_ui_components::plugin_host_lifecycle::init_plugin_host_job();
    // Same explicit AppUserModelID as the plugin-host process: keeps any
    // app-visible plugin window from spawning a stray taskbar identity.
    sphere_ui_components::plugin_host_lifecycle::set_futureboard_app_user_model_id();

    // Discord IPC is optional and never runs on the GPUI thread. Production
    // builds can bake in FUTUREBOARD_DISCORD_CLIENT_ID; development builds may
    // provide it at runtime. Missing Discord/config must not block app startup.
    let discord_rpc_enabled = sphere_ui_components::settings::SettingsSchema::load_from_disk()
        .general
        .discord_rpc_enabled;
    let discord_application_id = std::env::var("FUTUREBOARD_DISCORD_CLIENT_ID")
        .ok()
        .or_else(|| option_env!("FUTUREBOARD_DISCORD_CLIENT_ID").map(str::to_owned));
    let discord_rpc = discord_application_id
        .and_then(|application_id| {
            sphere_discord_rpc::DiscordRpcConfig::from_application_id(
                application_id,
                env!("CARGO_PKG_VERSION"),
            )
        })
        .and_then(|config| {
            match sphere_discord_rpc::DiscordRpc::start(
                config,
                sphere_discord_rpc::Presence::Welcome,
                discord_rpc_enabled,
            ) {
                Ok(rpc) => {
                    app::install_discord_rpc(rpc.handle());
                    Some(rpc)
                }
                Err(error) => {
                    boot::log(&format!("Discord RPC disabled: {error}"));
                    None
                }
            }
        });

    // Catch any panic that escapes the GPUI render loop so we see *why*
    // the window blanks out instead of getting a silent crash.
    std::panic::set_hook(Box::new(|info| {
        eprintln!("[panic] {info}");
        let bt = std::backtrace::Backtrace::force_capture();
        eprintln!("[panic] backtrace:\n{bt}");
        sphere_ui_components::plugin_host_lifecycle::PluginHostProcessManager::global()
            .shutdown_all(sphere_ui_components::plugin_host_lifecycle::HOST_SHUTDOWN_TIMEOUT)
            .ok();
        app::shutdown_discord_rpc();
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
    if let Some(discord_rpc) = discord_rpc {
        discord_rpc.shutdown();
    }
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
        gpui_windows::WindowsPlatform::new(false).expect("failed to initialize Windows platform"),
    );

    #[cfg(target_os = "macos")]
    let platform: std::rc::Rc<dyn gpui::Platform> =
        std::rc::Rc::new(gpui_macos::MacPlatform::new(false));

    #[cfg(target_os = "linux")]
    let platform: std::rc::Rc<dyn gpui::Platform> = gpui_linux::current_platform(false);

    gpui::Application::with_platform(platform)
}
