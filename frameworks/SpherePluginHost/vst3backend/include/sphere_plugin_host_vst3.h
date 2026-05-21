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
SPHERE_PLUGIN_HOST_API void sphere_plugin_editor_close_window(unsigned long long handle);
SPHERE_PLUGIN_HOST_API void sphere_plugin_host_free_string(SpherePluginHostString value);

}
