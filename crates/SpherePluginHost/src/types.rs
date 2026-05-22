use napi_derive::napi;
use serde::{Deserialize, Serialize};

#[napi(object)]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HostStatus {
    pub available: bool,
    pub backend: String,
    pub vst3_sdk: bool,
    pub clap_sdk: bool,
    pub clap_helpers: bool,
    pub message: String,
}

#[napi(object)]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub category: String,
    pub sub_categories: Option<String>,
    pub format: String,
    pub path: String,
    pub module_path: Option<String>,
    pub class_id: Option<String>,
    pub version: Option<String>,
    pub sdk_version: Option<String>,
    pub is_shell_child: bool,
    pub sdk_metadata_loaded: bool,
}
