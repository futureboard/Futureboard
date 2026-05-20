#include "sphere_plugin_host_vst3.h"

#include "public.sdk/source/vst/hosting/hostclasses.h"
#include "public.sdk/source/vst/hosting/module.h"
#include "public.sdk/source/vst/utility/stringconvert.h"

#include <cctype>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <new>
#include <string>
#include <vector>

namespace {

// Match VST3 SDK string literals exactly.
constexpr const char* kAudioModuleClass = "Audio Module Class";
constexpr const char* kControllerClass  = "Component Controller Class";

SpherePluginHostString make_string(std::string value) {
  auto* data = new (std::nothrow) char[value.size() + 1];
  if (!data) {
    return {nullptr, 0};
  }
  std::memcpy(data, value.data(), value.size());
  data[value.size()] = '\0';
  return {data, static_cast<unsigned long long>(value.size())};
}

std::string escape_json(const std::string& value) {
  std::string out;
  out.reserve(value.size() + 8);
  for (char c : value) {
    if (c == '\\' || c == '"') {
      out.push_back('\\');
    }
    out.push_back(c);
  }
  return out;
}

bool is_vst3_bundle(const std::filesystem::path& path) {
  auto ext = path.extension().string();
  for (auto& c : ext) {
    c = static_cast<char>(std::tolower(static_cast<unsigned char>(c)));
  }
  return ext == ".vst3";
}

std::string uid_to_string(const Steinberg::TUID cid) {
  return VST3::UID::fromTUID(cid).toString();
}

template <typename T>
std::string char_array_to_string(const T* value, Steinberg::uint32 max) {
  return Steinberg::Vst::StringConvert::convert(value, max);
}

struct ClassEntry {
  std::string name;
  std::string vendor;
  std::string category;
  std::string sub_categories;
  std::string class_id;
  std::string version;
  std::string sdk_version;
};

} // namespace

extern "C" SpherePluginHostString sphere_vst3_scan_path_json(const char* path) {
  if (!path) {
    return make_string("[]");
  }

  const bool debug = std::getenv("SPHERE_PLUGIN_HOST_DEBUG") != nullptr;

  // Phase 1 host scanner: load VST3 factory metadata. Does not instantiate
  // processors, open editors, or touch realtime audio.
  std::filesystem::path root(path);
  if (!std::filesystem::exists(root)) {
    return make_string("[]");
  }

  std::string json = "[";
  bool first = true;

  const auto append = [&](const std::filesystem::path& plugin_path) {
    if (debug) {
      std::fprintf(stderr, "[SpherePluginHost] Scanning: %s\n",
                   plugin_path.string().c_str());
    }

    std::string error;
    auto module = VST3::Hosting::Module::create(plugin_path.string(), error);
    if (!module) {
      if (debug) {
        std::fprintf(stderr, "[SpherePluginHost]   Module load failed: %s\n",
                     error.c_str());
      }
      // Fallback: emit a single path-only entry so the module is not invisible.
      if (!first) {
        json += ",";
      }
      first = false;
      const auto name = plugin_path.stem().string();
      const auto full_path = plugin_path.string();
      json += "{\"name\":\"" + escape_json(name) + "\",";
      json += "\"vendor\":\"Unknown Vendor\",";
      json += "\"category\":\"Uncategorized\",";
      json += "\"subCategories\":\"\",";
      json += "\"format\":\"VST3\",";
      json += "\"path\":\"" + escape_json(full_path) + "\",";
      json += "\"modulePath\":\"" + escape_json(full_path) + "\",";
      json += "\"classId\":null,\"version\":\"\",\"sdkVersion\":\"\",";
      json += "\"isShellChild\":false,\"sdkMetadataLoaded\":false}";
      return;
    }

    const auto factory = module->getFactory();
    Steinberg::Vst::HostApplication host_context;
    factory.setHostContext(&host_context);
    const auto factory_info = factory.info();
    const auto& raw_factory = factory.get();
    const auto raw_count = raw_factory ? raw_factory->countClasses() : 0;
    const auto class_count =
        raw_count > 0 ? static_cast<Steinberg::int32>(raw_count) : 0;

    if (debug) {
      std::fprintf(stderr, "[SpherePluginHost]   Factory class count: %d\n",
                   class_count);
    }

    Steinberg::FUnknownPtr<Steinberg::IPluginFactory3> f3(raw_factory);
    Steinberg::FUnknownPtr<Steinberg::IPluginFactory2> f2(raw_factory);

    // Collect audio/plugin classes first so we can compute isShellChild.
    std::vector<ClassEntry> audio_classes;
    int skipped = 0;

    for (Steinberg::int32 i = 0; i < class_count; ++i) {
      std::string name, vendor, category, sub_categories, class_id, version,
          sdk_version;
      bool ok = false;

      Steinberg::PClassInfoW ci3{};
      Steinberg::PClassInfo2 ci2{};
      Steinberg::PClassInfo ci{};

      if (f3 && f3->getClassInfoUnicode(i, &ci3) == Steinberg::kResultTrue) {
        name = char_array_to_string(ci3.name, Steinberg::PClassInfo::kNameSize);
        vendor =
            char_array_to_string(ci3.vendor, Steinberg::PClassInfo2::kVendorSize);
        category = char_array_to_string(ci3.category,
                                        Steinberg::PClassInfo::kCategorySize);
        sub_categories = char_array_to_string(
            ci3.subCategories, Steinberg::PClassInfo2::kSubCategoriesSize);
        version = char_array_to_string(ci3.version,
                                       Steinberg::PClassInfo2::kVersionSize);
        sdk_version = char_array_to_string(ci3.sdkVersion,
                                           Steinberg::PClassInfo2::kVersionSize);
        class_id = uid_to_string(ci3.cid);
        ok = true;
      } else if (f2 &&
                 f2->getClassInfo2(i, &ci2) == Steinberg::kResultTrue) {
        name = char_array_to_string(ci2.name, Steinberg::PClassInfo::kNameSize);
        vendor =
            char_array_to_string(ci2.vendor, Steinberg::PClassInfo2::kVendorSize);
        category = char_array_to_string(ci2.category,
                                        Steinberg::PClassInfo::kCategorySize);
        sub_categories = char_array_to_string(
            ci2.subCategories, Steinberg::PClassInfo2::kSubCategoriesSize);
        version = char_array_to_string(ci2.version,
                                       Steinberg::PClassInfo2::kVersionSize);
        class_id = uid_to_string(ci2.cid);
        ok = true;
      } else if (raw_factory->getClassInfo(i, &ci) == Steinberg::kResultTrue) {
        name = char_array_to_string(ci.name, Steinberg::PClassInfo::kNameSize);
        category = char_array_to_string(ci.category,
                                        Steinberg::PClassInfo::kCategorySize);
        class_id = uid_to_string(ci.cid);
        ok = true;
      }

      if (debug) {
        std::fprintf(stderr,
                     "[SpherePluginHost]   class[%d]: name=%s category=%s\n",
                     i, name.c_str(), category.c_str());
      }

      if (!ok) {
        ++skipped;
        continue;
      }

      // Skip internal controller classes — not user-visible plugin entries.
      if (category == kControllerClass) {
        if (debug) {
          std::fprintf(stderr,
                       "[SpherePluginHost]     -> skipped (controller class)\n");
        }
        ++skipped;
        continue;
      }

      if (vendor.empty()) {
        vendor = factory_info.vendor();
      }

      audio_classes.push_back(
          {name, vendor, category, sub_categories, class_id, version, sdk_version});
    }

    if (debug) {
      std::fprintf(stderr,
                   "[SpherePluginHost]   Accepted: %zu plugin classes, "
                   "skipped: %d\n",
                   audio_classes.size(), skipped);
    }

    // isShellChild: this module exposes more than one audio plugin class.
    const bool is_shell = (audio_classes.size() > 1);
    const std::string module_path = plugin_path.string();

    for (const auto& entry : audio_classes) {
      if (!first) {
        json += ",";
      }
      first = false;
      json += "{\"name\":\"" + escape_json(entry.name) + "\",";
      json += "\"vendor\":\"" + escape_json(entry.vendor) + "\",";
      json += "\"category\":\"" + escape_json(entry.category) + "\",";
      json += "\"subCategories\":\"" + escape_json(entry.sub_categories) + "\",";
      json += "\"format\":\"VST3\",";
      json += "\"path\":\"" + escape_json(module_path) + "\",";
      json += "\"modulePath\":\"" + escape_json(module_path) + "\",";
      json += "\"classId\":\"" + escape_json(entry.class_id) + "\",";
      json += "\"version\":\"" + escape_json(entry.version) + "\",";
      json += "\"sdkVersion\":\"" + escape_json(entry.sdk_version) + "\",";
      json += "\"isShellChild\":" +
              std::string(is_shell ? "true" : "false") + ",";
      json += "\"sdkMetadataLoaded\":true}";
    }
  };

  if (is_vst3_bundle(root)) {
    append(root);
  } else {
    for (const auto& entry : std::filesystem::recursive_directory_iterator(
             root, std::filesystem::directory_options::skip_permission_denied)) {
      if (is_vst3_bundle(entry.path())) {
        append(entry.path());
      }
    }
  }
  json += "]";
  return make_string(json);
}

extern "C" void sphere_plugin_host_free_string(SpherePluginHostString value) {
  delete[] value.data;
}
