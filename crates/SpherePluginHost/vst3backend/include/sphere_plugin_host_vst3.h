#pragma once

#ifdef _WIN32
#  define SPHERE_PLUGIN_HOST_API __declspec(dllexport)
#else
#  define SPHERE_PLUGIN_HOST_API __attribute__((visibility("default")))
#endif

extern "C" {

struct SpherePluginHostString {
  const char* data;
  unsigned long long len;
};

SPHERE_PLUGIN_HOST_API SpherePluginHostString sphere_vst3_scan_path_json(const char* path);
SPHERE_PLUGIN_HOST_API SpherePluginHostString sphere_clap_scan_path_json(const char* path);
SPHERE_PLUGIN_HOST_API unsigned long long sphere_plugin_editor_open_window(
    const char* window_id,
    const char* title,
    const char* subtitle,
    int width,
    int height);
SPHERE_PLUGIN_HOST_API unsigned long long sphere_plugin_editor_get_attach_handle(unsigned long long handle);
SPHERE_PLUGIN_HOST_API int sphere_plugin_editor_attach_vst3_view(
    unsigned long long handle,
    const char* plugin_path,
    const char* class_id);
SPHERE_PLUGIN_HOST_API void sphere_plugin_editor_close_window(unsigned long long handle);
SPHERE_PLUGIN_HOST_API void sphere_plugin_editor_focus_window(unsigned long long handle);
SPHERE_PLUGIN_HOST_API void sphere_plugin_editor_resize_window(unsigned long long handle, int width, int height);
SPHERE_PLUGIN_HOST_API SpherePluginHostString sphere_plugin_editor_drain_param_events_json();
SPHERE_PLUGIN_HOST_API void sphere_plugin_host_free_string(SpherePluginHostString value);

// ── Embedded editor (GPUI-hosted) ───────────────────────────────────────────
// Attach a VST3 IPlugView into a WS_CHILD region under a caller-provided parent
// window (the GPUI borderless editor window's native HWND). No NanoVG shell.
// Must be called on the thread that owns `parent_hwnd`. Returns a non-zero
// session handle on success, 0 on failure (never throws).
SPHERE_PLUGIN_HOST_API unsigned long long sphere_plugin_editor_embed_attach(
    unsigned long long parent_hwnd,
    const char* plugin_path,
    const char* class_id,
    int x,
    int y,
    int width,
    int height);
SPHERE_PLUGIN_HOST_API void sphere_plugin_editor_embed_set_bounds(
    unsigned long long handle,
    int x,
    int y,
    int width,
    int height);
SPHERE_PLUGIN_HOST_API void sphere_plugin_editor_embed_detach(unsigned long long handle);
SPHERE_PLUGIN_HOST_API void sphere_plugin_editor_embed_detach_all();
SPHERE_PLUGIN_HOST_API int sphere_plugin_editor_embed_is_valid(unsigned long long handle);
SPHERE_PLUGIN_HOST_API int sphere_plugin_editor_embed_has_visible_ui(unsigned long long handle);
// Presentation mode currently backing this embed session:
//   0 = ChildHwndEmbed (WS_CHILD), 1 = OwnedToolWindowFallback (WS_POPUP tool),
//  -1 = no such session. Exactly one mode is ever active per session.
SPHERE_PLUGIN_HOST_API int sphere_plugin_editor_embed_host_kind(unsigned long long handle);
// Reposition (if needed), onSize, WM_SIZE/WM_SHOWWINDOW to plugin children, and pump
// pending paint messages. Call from the GPUI UI thread while Attached.
SPHERE_PLUGIN_HOST_API void sphere_plugin_editor_embed_refresh(unsigned long long handle);

}
