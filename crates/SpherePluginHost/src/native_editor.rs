//! Plain-Rust facade over the C ABI editor-window helpers in
//! `sphere_plugin_editor_*` (defined in `vst3backend`). The N-API wrapper
//! in `editor_window.rs` is feature-gated and unavailable to the native
//! `futureboard_native` binary; this module ships the same surface as
//! `Result<u64, String>` so both targets can drive the IPlugView
//! lifecycle.
//!
//! Hard rules (per `SKILL.md` §13–14):
//! - These calls must not run on the audio thread.
//! - Every `open_plugin_editor_window` must pair with `close_plugin_editor_window`.
//! - Bad plugin → `Err(...)`, never panic.
//! - `attach_vst3_editor_view` is best-effort; failure leaves the host
//!   window open so the caller can render a GPUI fallback.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_ulonglong};

#[repr(C)]
struct SpherePluginHostString {
    data: *const c_char,
    len: u64,
}

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

    fn sphere_plugin_editor_embed_attach(
        parent_hwnd: c_ulonglong,
        plugin_path: *const c_char,
        class_id: *const c_char,
        x: c_int,
        y: c_int,
        width: c_int,
        height: c_int,
    ) -> c_ulonglong;
    fn sphere_plugin_editor_embed_set_bounds(
        handle: c_ulonglong,
        x: c_int,
        y: c_int,
        width: c_int,
        height: c_int,
    );
    fn sphere_plugin_editor_embed_detach(handle: c_ulonglong);
    fn sphere_plugin_editor_embed_detach_all();
    fn sphere_plugin_editor_embed_is_valid(handle: c_ulonglong) -> c_int;
    fn sphere_plugin_editor_embed_has_visible_ui(handle: c_ulonglong) -> c_int;
    fn sphere_plugin_editor_embed_host_kind(handle: c_ulonglong) -> c_int;
    fn sphere_plugin_editor_embed_refresh(handle: c_ulonglong);
}

/// Which native presentation backs an attached embed session. Exactly one mode
/// is ever active per session — the C++ host never creates both a WS_CHILD
/// embed and an owned tool window at the same time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginEditorPresentationMode {
    /// `WS_CHILD` region under the GPUI window's HWND.
    ChildHwndEmbed,
    /// `WS_POPUP | WS_EX_TOOLWINDOW` owned by the GPUI window (default).
    OwnedToolWindowFallback,
}

/// Query the presentation mode currently backing an attached editor session.
/// `None` if the handle is unknown / detached.
pub fn editor_presentation_mode(handle: u64) -> Option<PluginEditorPresentationMode> {
    if handle == 0 {
        return None;
    }
    match unsafe { sphere_plugin_editor_embed_host_kind(handle as c_ulonglong) } {
        0 => Some(PluginEditorPresentationMode::ChildHwndEmbed),
        1 => Some(PluginEditorPresentationMode::OwnedToolWindowFallback),
        _ => None,
    }
}

/// Region (in physical pixels, relative to the parent window's client area)
/// for the embedded plugin host child window.
#[derive(Debug, Clone, Copy, Default)]
pub struct EmbedRegion {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Attach a VST3 IPlugView into a WS_CHILD host region under `parent_hwnd`
/// (the GPUI editor window's native handle). Returns a non-zero session handle.
///
/// Must be called on the thread that owns `parent_hwnd` (the GPUI UI thread),
/// never the audio thread. `Err` on any failure — the caller renders a GPUI
/// fallback; no panic.
pub fn attach_editor_into_parent(
    parent_hwnd: u64,
    plugin_path: &str,
    class_id: &str,
    region: EmbedRegion,
) -> Result<u64, String> {
    if parent_hwnd == 0 {
        return Err("attach_editor_into_parent: null parent handle".to_string());
    }
    let path = to_cstring("plugin_path", plugin_path.to_string())?;
    let class = to_cstring("class_id", class_id.to_string())?;
    let handle = unsafe {
        sphere_plugin_editor_embed_attach(
            parent_hwnd as c_ulonglong,
            path.as_ptr(),
            class.as_ptr(),
            region.x as c_int,
            region.y as c_int,
            region.width as c_int,
            region.height as c_int,
        )
    };
    if handle == 0 {
        Err("embedded plugin editor failed to attach".to_string())
    } else {
        Ok(handle)
    }
}

/// Reposition / resize the embedded host child region (physical pixels).
pub fn set_editor_region_bounds(handle: u64, region: EmbedRegion) {
    if handle == 0 {
        return;
    }
    unsafe {
        sphere_plugin_editor_embed_set_bounds(
            handle as c_ulonglong,
            region.x as c_int,
            region.y as c_int,
            region.width as c_int,
            region.height as c_int,
        )
    };
}

/// Detach the IPlugView and destroy the host child window. Idempotent.
pub fn detach_editor(handle: u64) {
    if handle == 0 {
        return;
    }
    unsafe { sphere_plugin_editor_embed_detach(handle as c_ulonglong) };
}

/// Tear down every embedded editor session (call on application quit).
pub fn detach_all_embedded_editors() {
    unsafe { sphere_plugin_editor_embed_detach_all() };
}

pub fn editor_is_valid(handle: u64) -> bool {
    if handle == 0 {
        return false;
    }
    unsafe { sphere_plugin_editor_embed_is_valid(handle as c_ulonglong) != 0 }
}

/// Returns true when the embedded session has a visible host child and either
/// plugin-owned sub-windows or a non-trivial `IPlugView::getSize` after attach.
pub fn editor_has_visible_ui(handle: u64) -> bool {
    if handle == 0 {
        return false;
    }
    unsafe { sphere_plugin_editor_embed_has_visible_ui(handle as c_ulonglong) != 0 }
}

/// Pump paint/size messages and re-sync host geometry (tool window screen position).
/// Call on the GPUI UI thread each frame while the editor is attached.
pub fn refresh_editor_host(handle: u64) {
    if handle == 0 {
        return;
    }
    unsafe { sphere_plugin_editor_embed_refresh(handle as c_ulonglong) };
}

/// Options accepted by the native editor window. `width`/`height` default
/// to a conservative 560×380 if unset.
#[derive(Debug, Clone, Default)]
pub struct NativeEditorWindowOptions {
    pub window_id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub plugin_path: Option<String>,
    pub class_id: Option<String>,
    /// Set to "VST3" to also call `attach_vst3_editor_view` after open.
    pub format: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NativeEditorParamEvent {
    pub window_id: String,
    pub param_id: f64,
    pub value: f64,
}

#[derive(serde::Deserialize)]
struct ParamEventRaw {
    #[serde(rename = "windowId")]
    window_id: String,
    #[serde(rename = "paramId")]
    param_id: f64,
    value: f64,
}

fn to_cstring(label: &str, value: String) -> Result<CString, String> {
    CString::new(value).map_err(|e| format!("{label}: {e}"))
}

/// Open a native plugin editor window. Returns the opaque host handle
/// (non-zero on success) which subsequent `close`/`focus`/`resize`/
/// `attach` calls must reference.
pub fn open_plugin_editor_window(options: NativeEditorWindowOptions) -> Result<u64, String> {
    let window_id = to_cstring("window_id", options.window_id)?;
    let title = to_cstring("title", options.title)?;
    let subtitle_text = options
        .subtitle
        .clone()
        .unwrap_or_else(|| "Native plugin editor window".to_string());
    let subtitle = to_cstring("subtitle", subtitle_text)?;
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
        return Err("plugin editor window failed to open".to_string());
    }
    if options
        .format
        .as_deref()
        .map(|f| f.eq_ignore_ascii_case("VST3"))
        .unwrap_or(false)
    {
        if let (Some(plugin_path), Some(class_id)) = (options.plugin_path, options.class_id) {
            let _ = attach_vst3_editor_view(handle, plugin_path, class_id);
        }
    }
    Ok(handle)
}

pub fn get_plugin_editor_attach_handle(handle: u64) -> u64 {
    if handle == 0 {
        return 0;
    }
    unsafe { sphere_plugin_editor_get_attach_handle(handle as c_ulonglong) }
}

pub fn attach_vst3_editor_view(
    handle: u64,
    plugin_path: String,
    class_id: String,
) -> Result<bool, String> {
    if handle == 0 {
        return Ok(false);
    }
    let plugin_path = to_cstring("plugin_path", plugin_path)?;
    let class_id = to_cstring("class_id", class_id)?;
    let ok = unsafe {
        sphere_plugin_editor_attach_vst3_view(
            handle as c_ulonglong,
            plugin_path.as_ptr(),
            class_id.as_ptr(),
        )
    };
    Ok(ok != 0)
}

pub fn close_plugin_editor_window(handle: u64) {
    if handle == 0 {
        return;
    }
    unsafe { sphere_plugin_editor_close_window(handle as c_ulonglong) };
}

pub fn focus_plugin_editor_window(handle: u64) {
    if handle == 0 {
        return;
    }
    unsafe { sphere_plugin_editor_focus_window(handle as c_ulonglong) };
}

pub fn resize_plugin_editor_window(handle: u64, width: u32, height: u32) {
    if handle == 0 {
        return;
    }
    unsafe {
        sphere_plugin_editor_resize_window(handle as c_ulonglong, width as c_int, height as c_int)
    };
}

/// Drain any pending parameter-change events emitted by the native
/// editor view. Callers should poll this on the UI thread at ~30 Hz.
pub fn drain_plugin_editor_param_events() -> Result<Vec<NativeEditorParamEvent>, String> {
    let native = unsafe { sphere_plugin_editor_drain_param_events_json() };
    if native.data.is_null() {
        return Ok(Vec::new());
    }
    let json = unsafe { CStr::from_ptr(native.data) }
        .to_string_lossy()
        .into_owned();
    unsafe { sphere_plugin_host_free_string(native) };
    let parsed: Vec<ParamEventRaw> =
        serde_json::from_str(&json).map_err(|e| format!("param event json: {e}"))?;
    Ok(parsed
        .into_iter()
        .map(|p| NativeEditorParamEvent {
            window_id: p.window_id,
            param_id: p.param_id,
            value: p.value,
        })
        .collect())
}

/// FNV-1a stable id for path-keyed window ids. Matches the helper used
/// by the N-API wrapper.
pub fn stable_id(input: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}
