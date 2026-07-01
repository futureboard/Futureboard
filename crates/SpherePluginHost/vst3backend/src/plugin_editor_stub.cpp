// Non-Windows editor backend stub.
//
// plugin_editor_window.cpp hosts VST3 plug-in editor windows via native
// Win32 (HWND) APIs and is Windows-only (see the guard at the top of that
// file). On Linux/macOS there is no native editor window host yet, so this
// stub implements the same C ABI (see sphere_plugin_host_vst3.h) and reports
// "unsupported" cleanly instead of failing to build or link.
//
// Plugin scanning/loading is unaffected: it does not depend on editor
// hosting, so it keeps working even though editor windows return 0/failure
// here.

#include "sphere_plugin_host_vst3.h"

#include <cstring>

namespace {

SpherePluginHostString make_empty_json_array() {
  static const char kEmpty[] = "[]";
  auto* data = new (std::nothrow) char[sizeof(kEmpty)];
  if (!data) return {nullptr, 0};
  std::memcpy(data, kEmpty, sizeof(kEmpty));
  return {data, sizeof(kEmpty) - 1};
}

}  // namespace

extern "C" SpherePluginHostString sphere_plugin_editor_drain_param_events_json() {
  return make_empty_json_array();
}

extern "C" unsigned long long sphere_plugin_editor_embed_attach(
    unsigned long long parent_hwnd,
    const char* plugin_path,
    const char* class_id,
    int x,
    int y,
    int width,
    int height) {
  (void)parent_hwnd;
  (void)plugin_path;
  (void)class_id;
  (void)x;
  (void)y;
  (void)width;
  (void)height;
  return 0;  // Unsupported on this platform.
}

extern "C" void sphere_plugin_editor_embed_set_bounds(
    unsigned long long handle,
    int x,
    int y,
    int width,
    int height) {
  (void)handle;
  (void)x;
  (void)y;
  (void)width;
  (void)height;
}

extern "C" void sphere_plugin_editor_embed_detach(unsigned long long handle) {
  (void)handle;
}

extern "C" void sphere_plugin_editor_embed_detach_all() {}

extern "C" int sphere_plugin_editor_embed_is_valid(unsigned long long handle) {
  (void)handle;
  return 0;
}

extern "C" int sphere_plugin_editor_embed_has_visible_ui(unsigned long long handle) {
  (void)handle;
  return 0;
}

extern "C" int sphere_plugin_editor_embed_host_kind(unsigned long long handle) {
  (void)handle;
  return -1;  // No such session.
}

extern "C" void sphere_plugin_editor_embed_refresh(unsigned long long handle) {
  (void)handle;
}

extern "C" int sphere_plugin_editor_embed_preferred_size(
    unsigned long long handle,
    int* out_width,
    int* out_height) {
  (void)handle;
  (void)out_width;
  (void)out_height;
  return 0;
}

extern "C" unsigned long long sphere_plugin_editor_embed_prepare(
    const char* plugin_path,
    const char* class_id,
    int* out_width,
    int* out_height) {
  (void)plugin_path;
  (void)class_id;
  (void)out_width;
  (void)out_height;
  return 0;
}

extern "C" void sphere_plugin_editor_embed_cancel_prepare(unsigned long long prepare_id) {
  (void)prepare_id;
}

extern "C" unsigned long long sphere_plugin_editor_embed_attach_prepared(
    unsigned long long prepare_id,
    unsigned long long parent_hwnd,
    int x,
    int y,
    int width,
    int height) {
  (void)prepare_id;
  (void)parent_hwnd;
  (void)x;
  (void)y;
  (void)width;
  (void)height;
  return 0;
}

extern "C" unsigned long long sphere_plugin_editor_embed_host_hwnd(unsigned long long handle) {
  (void)handle;
  return 0;
}

extern "C" void sphere_plugin_editor_embed_delayed_gpu_refresh(unsigned long long handle) {
  (void)handle;
}
