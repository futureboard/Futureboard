#![allow(clippy::needless_pass_by_value)]
#![allow(non_snake_case)]

mod editor_window;
mod scanner;
mod types;

pub use editor_window::{
    attach_vst3_editor_view, close_plugin_editor_window, focus_plugin_editor_window,
    get_plugin_editor_attach_handle, open_plugin_editor_for_path, open_plugin_editor_window,
    resize_plugin_editor_window, drain_plugin_editor_param_events, PluginEditorParamEvent,
    PluginEditorWindowOptions,
};
use napi_derive::napi;
use scanner::{scan_audio_plugin_paths, scan_clap_paths, scan_vst3_paths};
use types::{HostStatus, PluginInfo};

#[napi]
pub fn init_plugin_host() -> napi::Result<HostStatus> {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let vst3_sdk_path = manifest_dir.join("../../external/vst3sdk/public.sdk");
    let clap_sdk_path = manifest_dir.join("../../external/clap/include/clap/clap.h");
    let clap_helpers_path = manifest_dir.join("../../external/clap-helpers/include/clap/helpers");
    Ok(HostStatus {
        available: true,
        backend: "vst3-clap-native-scanner".to_string(),
        vst3_sdk: vst3_sdk_path.exists(),
        clap_sdk: clap_sdk_path.exists(),
        clap_helpers: clap_helpers_path.exists(),
        message: "SpherePluginHost initialized. VST3 and CLAP metadata scanners are available."
            .to_string(),
    })
}

#[napi]
pub fn shutdown_plugin_host() -> napi::Result<()> {
    Ok(())
}

#[napi]
pub fn scan_vst3(paths: Vec<String>) -> napi::Result<Vec<PluginInfo>> {
    scan_vst3_paths(&paths).map_err(napi::Error::from_reason)
}

#[napi]
pub fn scan_clap(paths: Vec<String>) -> napi::Result<Vec<PluginInfo>> {
    scan_clap_paths(&paths).map_err(napi::Error::from_reason)
}

#[napi]
pub fn scan_audio_plugins(paths: Vec<String>) -> napi::Result<Vec<PluginInfo>> {
    scan_audio_plugin_paths(&paths).map_err(napi::Error::from_reason)
}

#[napi]
pub fn get_backend_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
