//! Sphere plug-in host: VST3/CLAP scan (N-API cdylib for Electron, rlib for native GPUI).
//!
//! Electron loads `sphere_plugin_host` as `PluginHost.node`; GPUI links the rlib and uses
//! [`registry`] for types and VST3/CLAP registry scan.

#![allow(clippy::needless_pass_by_value)]
#![allow(non_snake_case)]

mod editor_window;
pub mod preset;
pub mod registry;
mod scanner;
mod types;

pub use preset::{clear_all_presets, ensure_preset_folders, register_plugin, validate_plugin_for_registration, write_preset};
pub use registry::{
    classify_kind, default_preset_root, default_scan_paths, display_category,
    native_host_status, registry_plugin_from_scan, NativeHostStatus, PluginFormat, PluginKind,
    PluginRegistry, PluginScanFailure, PluginStatus, RegistryPlugin, RegistryScanResult, ScanOptions,
    ScanProgress,
};
pub use scanner::{discover_plugin_bundles, scan_plugin_bundle};

pub use editor_window::{
    attach_vst3_editor_view, close_plugin_editor_window, drain_plugin_editor_param_events,
    focus_plugin_editor_window, get_plugin_editor_attach_handle, open_plugin_editor_for_path,
    open_plugin_editor_window, resize_plugin_editor_window, PluginEditorParamEvent,
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
