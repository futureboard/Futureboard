//! Windows Job Object, AppUserModelID, and coordinated plugin-host shutdown.
//!
//! Implementation lives in [`crate::process_manager`] and [`crate::platform`].

pub use crate::process_manager::{
    shutdown_host_client, shutdown_host_client_with_timeout, BridgeHostManager,
    BridgeHostRecord, HostLifecycleState, PluginHostHandle, PluginHostId,
    PluginHostProcessManager, HOST_SHUTDOWN_TIMEOUT, init_plugin_host_job,
};
pub use crate::plugin_host_spawn_config::PluginHostSpawnConfig;

/// Shared Windows shell identity for FutureboardNative and PluginHost.
pub const APP_USER_MODEL_ID: &str = "studio.futureboard.Futureboard";

/// Set the process-wide explicit AppUserModelID so plugin-host and editor
/// windows group under the DAW shell identity.
pub fn set_futureboard_app_user_model_id() {
    set_app_user_model_id();
}

#[cfg(windows)]
pub fn set_app_user_model_id() {
    use windows::core::w;
    use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
    // SAFETY: `w!` is a 'static NUL-terminated UTF-16 literal.
    let result =
        unsafe { SetCurrentProcessExplicitAppUserModelID(w!("studio.futureboard.Futureboard")) };
    match result {
        Ok(()) => eprintln!(
            "[app-id] SetCurrentProcessExplicitAppUserModelID id={APP_USER_MODEL_ID} ok=true"
        ),
        Err(error) => eprintln!(
            "[app-id] SetCurrentProcessExplicitAppUserModelID id={APP_USER_MODEL_ID} ok=false error={error}"
        ),
    }
}

#[cfg(not(windows))]
pub fn set_app_user_model_id() {}
