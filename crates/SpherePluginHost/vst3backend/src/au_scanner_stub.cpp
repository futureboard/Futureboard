#include "sphere_plugin_host_vst3.h"

#include <cstdlib>
#include <cstring>

namespace {

SpherePluginHostString empty_json_array() {
  static const char payload[] = "[]";
  return SpherePluginHostString{payload, sizeof(payload) - 1};
}

}  // namespace

extern "C" {

SPHERE_PLUGIN_HOST_API SpherePluginHostString sphere_au_scan_json() {
  return empty_json_array();
}

SPHERE_PLUGIN_HOST_API SpherePluginHostString sphere_au_validate_component_json(
    const char* /*component_id*/) {
  return empty_json_array();
}

}  // extern "C"
