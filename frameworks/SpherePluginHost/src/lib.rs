#![allow(clippy::needless_pass_by_value)]
#![allow(non_snake_case)]

mod scanner;
mod types;

use napi_derive::napi;
use scanner::scan_vst3_paths;
use types::{HostStatus, PluginInfo};

#[napi]
pub fn init_plugin_host() -> napi::Result<HostStatus> {
    let vst3_sdk_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../external/vst3sdk/public.sdk");
    Ok(HostStatus {
        available: true,
        backend: "vst3-sdk-native-scanner".to_string(),
        vst3_sdk: vst3_sdk_path.exists(),
        message: "SpherePluginHost initialized. VST3 factory metadata scanner is available.".to_string(),
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
pub fn get_backend_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
