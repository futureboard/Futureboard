use std::ffi::CStr;
use std::os::raw::c_char;

use napi_derive::napi;

extern "C" {
    fn sphere_plugin_editor_drain_param_events_json() -> SpherePluginHostString;
    fn sphere_plugin_host_free_string(value: SpherePluginHostString);
}

#[repr(C)]
struct SpherePluginHostString {
    data: *const c_char,
    len: u64,
}

#[napi(object)]
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginEditorParamEvent {
    pub window_id: String,
    pub param_id: f64,
    pub value: f64,
}

#[napi]
pub fn drain_plugin_editor_param_events() -> napi::Result<Vec<PluginEditorParamEvent>> {
    let native = unsafe { sphere_plugin_editor_drain_param_events_json() };
    if native.data.is_null() {
        return Ok(Vec::new());
    }
    let json = unsafe { CStr::from_ptr(native.data) }
        .to_string_lossy()
        .into_owned();
    unsafe { sphere_plugin_host_free_string(native) };
    serde_json::from_str::<Vec<PluginEditorParamEvent>>(&json)
        .map_err(|error| napi::Error::from_reason(error.to_string()))
}
