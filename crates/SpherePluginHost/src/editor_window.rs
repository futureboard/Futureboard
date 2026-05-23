use std::ffi::CString;
use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_ulonglong};

use napi_derive::napi;

extern "C" {
    fn sphere_plugin_editor_open_window(
        window_id: *const c_char,
        title: *const c_char,
        subtitle: *const c_char,
        width: c_int,
        height: c_int,
    ) -> c_ulonglong;
    fn sphere_plugin_editor_get_attach_handle(handle: c_ulonglong) -> c_ulonglong;
    fn sphere_plugin_editor_attach_vst3_view(
        handle: c_ulonglong,
        plugin_path: *const c_char,
        class_id: *const c_char,
    ) -> c_int;
    fn sphere_plugin_editor_close_window(handle: c_ulonglong);
    fn sphere_plugin_editor_focus_window(handle: c_ulonglong);
    fn sphere_plugin_editor_resize_window(handle: c_ulonglong, width: c_int, height: c_int);
    fn sphere_plugin_editor_drain_param_events_json() -> SpherePluginHostString;
    fn sphere_plugin_host_free_string(value: SpherePluginHostString);
}

#[repr(C)]
struct SpherePluginHostString {
    data: *const c_char,
    len: u64,
}

#[napi(object)]
pub struct PluginEditorWindowOptions {
    pub window_id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub plugin_path: Option<String>,
    pub class_id: Option<String>,
    pub format: Option<String>,
}

#[napi]
pub fn open_plugin_editor_window(options: PluginEditorWindowOptions) -> napi::Result<f64> {
    let window_id = CString::new(options.window_id)
        .map_err(|error| napi::Error::from_reason(error.to_string()))?;
    let title =
        CString::new(options.title).map_err(|error| napi::Error::from_reason(error.to_string()))?;
    let subtitle = CString::new(
        options
            .subtitle
            .unwrap_or_else(|| "Native plugin editor window".to_string()),
    )
    .map_err(|error| napi::Error::from_reason(error.to_string()))?;
    let handle = unsafe {
        sphere_plugin_editor_open_window(
            window_id.as_ptr(),
            title.as_ptr(),
            subtitle.as_ptr(),
            options.width.unwrap_or(560) as c_int,
            options.height.unwrap_or(380) as c_int,
        )
    };
    if handle == 0 {
        return Err(napi::Error::from_reason(
            "Plugin editor window failed to open",
        ));
    }
    if options
        .format
        .as_deref()
        .map(|format| format.eq_ignore_ascii_case("VST3"))
        .unwrap_or(false)
    {
        if let (Some(plugin_path), Some(class_id)) = (options.plugin_path, options.class_id) {
            let _ = attach_vst3_editor_view(handle as f64, plugin_path, class_id);
        }
    }
    Ok(handle as f64)
}

#[napi]
pub fn open_plugin_editor_for_path(plugin_path: String) -> napi::Result<f64> {
    let plugins = crate::scanner::scan_audio_plugin_paths(std::slice::from_ref(&plugin_path))
        .map_err(napi::Error::from_reason)?;
    let plugin = plugins.first();
    let title = plugin.map(|plugin| plugin.name.clone()).unwrap_or_else(|| {
        std::path::Path::new(&plugin_path)
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("Plugin Editor")
            .to_string()
    });
    let subtitle = plugin
        .map(|plugin| format!("{} • {} • {}", plugin.format, plugin.vendor, plugin_path))
        .unwrap_or_else(|| format!("Native plugin editor • {plugin_path}"));
    let handle = open_plugin_editor_window(PluginEditorWindowOptions {
        window_id: format!("plugin-editor:{}", stable_id(&plugin_path)),
        title,
        subtitle: Some(subtitle),
        width: Some(820),
        height: Some(560),
        plugin_path: None,
        class_id: None,
        format: None,
    })?;
    if let Some(plugin) = plugin {
        if plugin.format == "VST3" {
            if let Some(class_id) = plugin.class_id.as_deref() {
                let _ = attach_vst3_editor_view(handle, plugin_path, class_id.to_string());
            }
        }
    }
    Ok(handle)
}

#[napi]
pub fn get_plugin_editor_attach_handle(handle: f64) -> napi::Result<f64> {
    if handle <= 0.0 {
        return Ok(0.0);
    }
    let attach = unsafe { sphere_plugin_editor_get_attach_handle(handle as c_ulonglong) };
    Ok(attach as f64)
}

#[napi]
pub fn attach_vst3_editor_view(
    handle: f64,
    plugin_path: String,
    class_id: String,
) -> napi::Result<bool> {
    if handle <= 0.0 {
        return Ok(false);
    }
    let plugin_path =
        CString::new(plugin_path).map_err(|error| napi::Error::from_reason(error.to_string()))?;
    let class_id =
        CString::new(class_id).map_err(|error| napi::Error::from_reason(error.to_string()))?;
    let ok = unsafe {
        sphere_plugin_editor_attach_vst3_view(
            handle as c_ulonglong,
            plugin_path.as_ptr(),
            class_id.as_ptr(),
        )
    };
    Ok(ok != 0)
}

#[napi]
pub fn close_plugin_editor_window(handle: f64) -> napi::Result<()> {
    if handle <= 0.0 {
        return Ok(());
    }
    unsafe { sphere_plugin_editor_close_window(handle as c_ulonglong) };
    Ok(())
}

#[napi]
pub fn focus_plugin_editor_window(handle: f64) -> napi::Result<()> {
    if handle <= 0.0 {
        return Ok(());
    }
    unsafe { sphere_plugin_editor_focus_window(handle as c_ulonglong) };
    Ok(())
}

#[napi]
pub fn resize_plugin_editor_window(handle: f64, width: u32, height: u32) -> napi::Result<()> {
    if handle <= 0.0 {
        return Ok(());
    }
    unsafe {
        sphere_plugin_editor_resize_window(handle as c_ulonglong, width as c_int, height as c_int)
    };
    Ok(())
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

fn stable_id(input: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}
