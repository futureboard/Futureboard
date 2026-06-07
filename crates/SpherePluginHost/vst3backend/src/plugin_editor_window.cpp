#include "sphere_plugin_host_vst3.h"

#include <algorithm>
#include <atomic>
#include <cassert>
#include <chrono>
#include <cmath>
#include <condition_variable>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <mutex>
#include <new>
#include <sstream>
#include <string>
#include <thread>
#include <vector>
#include <unordered_map>
#include <utility>

#ifdef _WIN32
#  define WIN32_LEAN_AND_MEAN
#  include <windows.h>
#  include <windowsx.h>
#  include <libloaderapi.h>
#  include <objbase.h>
#  include <dwmapi.h>
#  include <d3d11.h>
#  include <dxgi.h>
#  include <wrl/client.h>
#  include "pluginterfaces/base/ipluginbase.h"
#  include "pluginterfaces/gui/iplugview.h"
#  include "pluginterfaces/vst/ivstcomponent.h"
#  include "pluginterfaces/vst/ivsteditcontroller.h"
#  include "public.sdk/source/vst/hosting/hostclasses.h"
#  include "public.sdk/source/vst/hosting/module.h"
#  include "public.sdk/source/vst/utility/uid.h"
#  include "nanovg.h"
#  include "sphere_plugin_editor_embedded_assets.h"
#  include <yoga/Yoga.h>
#  define NANOVG_D3D11_IMPLEMENTATION
#  include "nanovg_d3d11.h"
#  pragma comment(lib, "d3d11.lib")
#  pragma comment(lib, "dxgi.lib")
#  pragma comment(lib, "dwmapi.lib")
using Microsoft::WRL::ComPtr;
#endif

namespace {

std::atomic<unsigned long long> g_next_handle{1};
std::mutex g_windows_mutex;

SpherePluginHostString make_string_local(std::string value) {
  auto* data = new (std::nothrow) char[value.size() + 1];
  if (!data) return {nullptr, 0};
  std::memcpy(data, value.data(), value.size());
  data[value.size()] = '\0';
  return {data, static_cast<unsigned long long>(value.size())};
}

#ifdef _WIN32
constexpr COLORREF kTitlebarDark = RGB(14, 19, 25);
constexpr UINT_PTR kRedrawTimer = 1;
constexpr UINT WM_ATTACH_VST3_EDITOR = WM_APP + 41;
constexpr const char* kVst3AudioModuleClass = "Audio Module Class";

enum class ButtonAction { Close, ReloadEditor, ReloadPlugin, GenericParams, Bypass, More };

struct ButtonRect {
  ButtonAction action;
  RECT rect{};
  const char* label = "";
};

enum class ShellIcon { Settings, Bypass, Cpu, Bolt, Database, Refresh, ChevronDown, Dots };

struct D3DState {
  ComPtr<ID3D11Device> device;
  ComPtr<ID3D11DeviceContext> context;
  ComPtr<IDXGISwapChain> swap_chain;
  ComPtr<ID3D11RenderTargetView> rtv;
  NVGcontext* vg = nullptr;
  int width = 0;
  int height = 0;
};

struct EditorWindowState;

struct YogaNodeDeleter {
  void operator()(YGNodeRef node) const {
    if (node) YGNodeFree(node);
  }
};

using YogaNodePtr = std::unique_ptr<YGNode, YogaNodeDeleter>;

struct EditorFlexLayout {
  RECT header{};
  RECT attach{};
  RECT preset{};
  RECT bypass{};
  RECT reload{};
  RECT more{};
  RECT close{};
};

YogaNodePtr make_yoga_node() { return YogaNodePtr(YGNodeNew()); }

RECT rect_from_node(YGNodeRef node, int offset_x = 0, int offset_y = 0) {
  const int left = offset_x + static_cast<int>(std::round(YGNodeLayoutGetLeft(node)));
  const int top = offset_y + static_cast<int>(std::round(YGNodeLayoutGetTop(node)));
  const int width = static_cast<int>(std::round(YGNodeLayoutGetWidth(node)));
  const int height = static_cast<int>(std::round(YGNodeLayoutGetHeight(node)));
  return RECT{left, top, left + (std::max)(1, width), top + (std::max)(1, height)};
}

class MinimalComponentHandler : public Steinberg::Vst::IComponentHandler {
 public:
  explicit MinimalComponentHandler(std::string window_id) : window_id_(std::move(window_id)) {}
  ~MinimalComponentHandler() = default;

  Steinberg::tresult PLUGIN_API beginEdit(Steinberg::Vst::ParamID) override { return Steinberg::kResultOk; }
  Steinberg::tresult PLUGIN_API performEdit(Steinberg::Vst::ParamID, Steinberg::Vst::ParamValue) override;
  Steinberg::tresult PLUGIN_API endEdit(Steinberg::Vst::ParamID) override { return Steinberg::kResultOk; }
  Steinberg::tresult PLUGIN_API restartComponent(Steinberg::int32) override { return Steinberg::kResultOk; }

  Steinberg::tresult PLUGIN_API queryInterface(const Steinberg::TUID _iid, void** obj) override {
    QUERY_INTERFACE(_iid, obj, Steinberg::FUnknown::iid, Steinberg::FUnknown)
    QUERY_INTERFACE(_iid, obj, Steinberg::Vst::IComponentHandler::iid, Steinberg::Vst::IComponentHandler)
    *obj = nullptr;
    return Steinberg::kNoInterface;
  }

  Steinberg::uint32 PLUGIN_API addRef() override { return ++ref_count_; }
  Steinberg::uint32 PLUGIN_API release() override {
    const auto next = --ref_count_;
    if (next == 0) delete this;
    return next;
  }

 private:
  std::atomic<Steinberg::uint32> ref_count_{1};
  std::string window_id_;
};

struct EditorParamEvent {
  std::string window_id;
  Steinberg::Vst::ParamID id = 0;
  Steinberg::Vst::ParamValue value = 0.0;
};

std::mutex g_param_events_mutex;
std::vector<EditorParamEvent> g_param_events;

std::string escape_json_local(const std::string& value) {
  std::string out;
  out.reserve(value.size() + 8);
  for (char c : value) {
    switch (c) {
      case '\\': out += "\\\\"; break;
      case '"': out += "\\\""; break;
      case '\n': out += "\\n"; break;
      case '\r': out += "\\r"; break;
      case '\t': out += "\\t"; break;
      default: out += c; break;
    }
  }
  return out;
}


Steinberg::tresult PLUGIN_API MinimalComponentHandler::performEdit(
    Steinberg::Vst::ParamID id,
    Steinberg::Vst::ParamValue value) {
  std::lock_guard<std::mutex> lock(g_param_events_mutex);
  g_param_events.push_back(EditorParamEvent{window_id_, id, value});
  return Steinberg::kResultOk;
}

struct AttachVst3Request {
  const char* plugin_path = nullptr;
  const char* class_id = nullptr;
  int result = 0;
};

struct EmbedPluginWebViewBasedScope;
struct Vst3EditorAttachment;

bool vst3_editor_debug() {
  static const bool enabled = std::getenv("FUTUREBOARD_VST3_EDITOR_DEBUG") != nullptr;
  return enabled;
}

inline bool vst3_view_rect_equal(const Steinberg::ViewRect& a, const Steinberg::ViewRect& b) {
  return a.left == b.left && a.top == b.top && a.right == b.right && a.bottom == b.bottom;
}

// IPlugFrame for VST3 editor hosting — mirrors SDK editorhost lifecycle.
class PluginEditorFrame final : public Steinberg::IPlugFrame {
 public:
  void bind(Steinberg::IPlugView* view, HWND host, HWND shell = nullptr) {
    bound_view_ = view;
    host_hwnd_ = host;
    shell_hwnd_ = shell;
  }
  void unbind() {
    bound_view_ = nullptr;
    host_hwnd_ = nullptr;
    shell_hwnd_ = nullptr;
  }

  Steinberg::tresult PLUGIN_API resizeView(Steinberg::IPlugView* view,
                                           Steinberg::ViewRect* newSize) override {
    const bool debug = vst3_editor_debug();
    if (debug) {
      std::fprintf(stderr, "[vst3-editor] resizeView called view=0x%p\n", static_cast<void*>(view));
    }
    if (newSize == nullptr || view == nullptr || view != bound_view_) {
      if (debug) std::fprintf(stderr, "[vst3-editor] resizeView rejected (invalid args)\n");
      return Steinberg::kInvalidArgument;
    }
    if (resize_recursion_guard_) {
      if (debug) std::fprintf(stderr, "[vst3-editor] resizeView rejected (recursion guard)\n");
      return Steinberg::kResultFalse;
    }

    Steinberg::ViewRect current{};
    if (view->getSize(&current) != Steinberg::kResultTrue) {
      if (debug) std::fprintf(stderr, "[vst3-editor] resizeView rejected (getSize failed)\n");
      return Steinberg::kInternalError;
    }
    if (vst3_view_rect_equal(current, *newSize)) {
      if (debug) {
        std::fprintf(stderr,
                     "[vst3-editor] resizeView accepted (no-op) rect=(%d,%d,%d,%d)\n",
                     newSize->left,
                     newSize->top,
                     newSize->right,
                     newSize->bottom);
      }
      return Steinberg::kResultTrue;
    }

    if (debug) {
      std::fprintf(stderr,
                   "[vst3-editor] resizeView requested rect=(%d,%d,%d,%d) size=%dx%d\n",
                   newSize->left,
                   newSize->top,
                   newSize->right,
                   newSize->bottom,
                   newSize->right - newSize->left,
                   newSize->bottom - newSize->top);
    }

    resize_recursion_guard_ = true;
    const int w = newSize->right - newSize->left;
    const int h = newSize->bottom - newSize->top;
    if (host_hwnd_ && IsWindow(host_hwnd_) && w > 0 && h > 0) {
      SetWindowPos(host_hwnd_, nullptr, 0, 0, w, h, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
      if (shell_hwnd_ && IsWindow(shell_hwnd_)) {
        RECT wr{0, 0, w, h};
        AdjustWindowRectEx(&wr,
                           static_cast<DWORD>(GetWindowLongPtrW(shell_hwnd_, GWL_STYLE)),
                           FALSE,
                           static_cast<DWORD>(GetWindowLongPtrW(shell_hwnd_, GWL_EXSTYLE)));
        SetWindowPos(shell_hwnd_,
                     nullptr,
                     0,
                     0,
                     wr.right - wr.left,
                     wr.bottom - wr.top,
                     SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
      }
    }
    resize_recursion_guard_ = false;

    Steinberg::ViewRect after{};
    if (view->getSize(&after) != Steinberg::kResultTrue) {
      if (debug) std::fprintf(stderr, "[vst3-editor] resizeView rejected (getSize after resize failed)\n");
      return Steinberg::kInternalError;
    }
    if (!vst3_view_rect_equal(after, *newSize)) {
      const auto on_size_res = view->onSize(newSize);
      if (debug) {
        std::fprintf(stderr,
                     "[vst3-editor] resizeView onSize result=0x%x\n",
                     static_cast<unsigned>(on_size_res));
      }
    }
    if (debug) std::fprintf(stderr, "[vst3-editor] resizeView accepted\n");
    return Steinberg::kResultOk;
  }

  Steinberg::tresult PLUGIN_API queryInterface(const Steinberg::TUID iid, void** obj) override {
    if (Steinberg::FUnknownPrivate::iidEqual(iid, Steinberg::INLINE_UID_OF(IPlugFrame)) ||
        Steinberg::FUnknownPrivate::iidEqual(iid, Steinberg::FUnknown::iid)) {
      *obj = static_cast<Steinberg::IPlugFrame*>(this);
      addRef();
      return Steinberg::kResultTrue;
    }
    *obj = nullptr;
    return Steinberg::kNoInterface;
  }
  Steinberg::uint32 PLUGIN_API addRef() override { return 1000; }
  Steinberg::uint32 PLUGIN_API release() override { return 1000; }

 private:
  Steinberg::IPlugView* bound_view_{nullptr};
  HWND host_hwnd_{nullptr};
  HWND shell_hwnd_{nullptr};
  bool resize_recursion_guard_{false};
};

struct Vst3EditorAttachment {
  VST3::Hosting::Module::Ptr module;
  Steinberg::Vst::HostApplication host_context;
  Steinberg::IPtr<MinimalComponentHandler> component_handler;
  Steinberg::IPtr<Steinberg::Vst::IComponent> component;
  Steinberg::IPtr<Steinberg::Vst::IEditController> controller;
  Steinberg::IPtr<Steinberg::Vst::IConnectionPoint> component_connection;
  Steinberg::IPtr<Steinberg::Vst::IConnectionPoint> controller_connection;
  Steinberg::IPtr<Steinberg::IPlugView> view;
  bool controller_is_component = false;
  bool attached = false;
  std::string plugin_path;
  std::string class_id;
  std::unique_ptr<EmbedPluginWebViewBasedScope> plugin_webview_based_scope;
  std::unique_ptr<PluginEditorFrame> editor_frame;

  ~Vst3EditorAttachment();
};

void vst3_install_plug_frame(Vst3EditorAttachment& att, HWND host, HWND shell) {
  if (!att.view) return;
  if (!att.editor_frame) {
    att.editor_frame = std::make_unique<PluginEditorFrame>();
  }
  att.editor_frame->bind(att.view.get(), host, shell);
  if (vst3_editor_debug()) {
    std::fprintf(stderr,
                 "[vst3-editor] setFrame called view=0x%p frame=0x%p host=0x%p\n",
                 static_cast<void*>(att.view.get()),
                 static_cast<void*>(att.editor_frame.get()),
                 static_cast<void*>(host));
  }
  const auto res = att.view->setFrame(att.editor_frame.get());
  std::fprintf(stderr, "[vst3-editor] setFrame result=0x%x\n", static_cast<unsigned>(res));
}

void vst3_clear_plug_frame(Vst3EditorAttachment& att) {
  if (att.view) {
    if (vst3_editor_debug()) {
      std::fprintf(stderr,
                   "[vst3-editor] setFrame null view=0x%p\n",
                   static_cast<void*>(att.view.get()));
    }
    att.view->setFrame(nullptr);
  }
  if (att.editor_frame) att.editor_frame->unbind();
  att.editor_frame.reset();
}

struct EditorWindowConfig {
  unsigned long long handle = 0;
  std::string window_id;
  std::wstring title;
  std::wstring subtitle;
  int width = 820;
  int height = 560;
};

struct EditorWindowState {
  unsigned long long handle = 0;
  std::string window_id;
  std::wstring title;
  std::wstring subtitle;
  HWND hwnd = nullptr;
  HWND attach_hwnd = nullptr;
  std::unique_ptr<Vst3EditorAttachment> vst3;
  D3DState d3d;
  float scale = 1.0f;
  ButtonRect buttons[4]{};
  bool has_error = false;
  std::string error;
  bool close_requested = false;
};

std::condition_variable g_windows_cv;
std::unordered_map<unsigned long long, EditorWindowState*> g_windows;
std::unordered_map<std::string, unsigned long long> g_window_ids;

std::wstring utf8_to_wide(const char* value) {
  if (!value || !*value) return L"";
  const int len = MultiByteToWideChar(CP_UTF8, 0, value, -1, nullptr, 0);
  if (len <= 0) return L"";
  std::wstring out(static_cast<std::size_t>(len - 1), L'\0');
  MultiByteToWideChar(CP_UTF8, 0, value, -1, out.data(), len);
  return out;
}

std::string wide_to_utf8(const std::wstring& value) {
  if (value.empty()) return "";
  const int len = WideCharToMultiByte(CP_UTF8, 0, value.c_str(), -1, nullptr, 0, nullptr, nullptr);
  if (len <= 0) return "";
  std::string out(static_cast<std::size_t>(len - 1), '\0');
  WideCharToMultiByte(CP_UTF8, 0, value.c_str(), -1, out.data(), len, nullptr, nullptr);
  return out;
}

float dpi_scale(HWND hwnd) {
  const UINT dpi = hwnd ? GetDpiForWindow(hwnd) : GetDpiForSystem();
  return static_cast<float>(dpi ? dpi : 96) / 96.0f;
}

int sx(EditorWindowState* state, int value) {
  return static_cast<int>(static_cast<float>(value) * (state ? state->scale : 1.0f));
}

NVGcolor nvg_rgb(unsigned char r, unsigned char g, unsigned char b) { return nvgRGB(r, g, b); }
NVGcolor nvg_rgba(unsigned char r, unsigned char g, unsigned char b, unsigned char a) { return nvgRGBA(r, g, b, a); }

bool ensure_render_target(EditorWindowState* state) {
  if (!state || !state->d3d.swap_chain || !state->d3d.device) return false;
  state->d3d.rtv.Reset();
  ComPtr<ID3D11Texture2D> back_buffer;
  if (FAILED(state->d3d.swap_chain->GetBuffer(0, IID_PPV_ARGS(&back_buffer)))) return false;
  return SUCCEEDED(state->d3d.device->CreateRenderTargetView(back_buffer.Get(), nullptr, &state->d3d.rtv));
}

bool resize_d3d(EditorWindowState* state, int width, int height) {
  if (!state || !state->d3d.swap_chain) return false;
  width = width > 1 ? width : 1;
  height = height > 1 ? height : 1;
  if (state->d3d.width == width && state->d3d.height == height && state->d3d.rtv) return true;
  state->d3d.rtv.Reset();
  if (FAILED(state->d3d.swap_chain->ResizeBuffers(0, width, height, DXGI_FORMAT_UNKNOWN, 0))) return false;
  state->d3d.width = width;
  state->d3d.height = height;
  return ensure_render_target(state);
}

bool init_d3d(EditorWindowState* state) {
  RECT rc{};
  GetClientRect(state->hwnd, &rc);
  const UINT flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT;
  D3D_FEATURE_LEVEL levels[] = {D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_0};
  D3D_FEATURE_LEVEL selected{};

  DXGI_SWAP_CHAIN_DESC desc{};
  desc.BufferCount = 2;
  desc.BufferDesc.Width = rc.right - rc.left;
  desc.BufferDesc.Height = rc.bottom - rc.top;
  desc.BufferDesc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
  desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
  desc.OutputWindow = state->hwnd;
  desc.SampleDesc.Count = 1;
  desc.Windowed = TRUE;
  desc.SwapEffect = DXGI_SWAP_EFFECT_DISCARD;

  HRESULT hr = D3D11CreateDeviceAndSwapChain(
      nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, flags, levels, ARRAYSIZE(levels),
      D3D11_SDK_VERSION, &desc, &state->d3d.swap_chain, &state->d3d.device,
      &selected, &state->d3d.context);
  if (FAILED(hr)) return false;

  state->d3d.vg = nvgCreateD3D11(state->d3d.device.Get(), NVG_ANTIALIAS | NVG_STENCIL_STROKES);
  if (!state->d3d.vg) return false;
  nvgCreateFontMem(state->d3d.vg, "sans", const_cast<unsigned char*>(kInterRegularTtf), static_cast<int>(kInterRegularTtf_len), 0);
  nvgCreateFontMem(state->d3d.vg, "sans-semibold", const_cast<unsigned char*>(kInterSemiBoldTtf), static_cast<int>(kInterSemiBoldTtf_len), 0);
  // Keep packaged icon SVG bytes referenced so the host shell assets are embedded in PluginHost.node.
  (void)kIconSettingsSvg; (void)kIconSettingsSvg_len; (void)kIconPowerSvg; (void)kIconPowerSvg_len;
  (void)kIconCpuSvg; (void)kIconCpuSvg_len; (void)kIconBoltSvg; (void)kIconBoltSvg_len;
  (void)kIconDatabaseSvg; (void)kIconDatabaseSvg_len; (void)kIconRefreshSvg; (void)kIconRefreshSvg_len;
  (void)kIconChevronDownSvg; (void)kIconChevronDownSvg_len; (void)kIconDotsSvg; (void)kIconDotsSvg_len;
  state->d3d.width = desc.BufferDesc.Width;
  state->d3d.height = desc.BufferDesc.Height;
  return ensure_render_target(state);
}

void draw_text_wide(NVGcontext* vg, float x, float y, const std::wstring& text) {
  const auto utf8 = wide_to_utf8(text);
  nvgText(vg, x, y, utf8.c_str(), nullptr);
}

void draw_icon(NVGcontext* vg, ShellIcon icon, float x, float y, float size, NVGcolor color) {
  const float cx = x + size * 0.5f;
  const float cy = y + size * 0.5f;
  nvgStrokeWidth(vg, 1.6f);
  nvgStrokeColor(vg, color);
  nvgFillColor(vg, color);
  nvgLineCap(vg, NVG_ROUND);
  nvgLineJoin(vg, NVG_ROUND);
  switch (icon) {
    case ShellIcon::Settings:
      nvgBeginPath(vg);
      nvgCircle(vg, cx, cy, size * 0.20f);
      nvgStroke(vg);
      for (int i = 0; i < 6; ++i) {
        const float a = static_cast<float>(i) * 1.0471976f;
        nvgBeginPath(vg);
        nvgMoveTo(vg, cx + cosf(a) * size * 0.32f, cy + sinf(a) * size * 0.32f);
        nvgLineTo(vg, cx + cosf(a) * size * 0.43f, cy + sinf(a) * size * 0.43f);
        nvgStroke(vg);
      }
      break;
    case ShellIcon::Bypass:
      nvgBeginPath(vg);
      nvgMoveTo(vg, cx, y + size * 0.18f);
      nvgLineTo(vg, cx, y + size * 0.48f);
      nvgStroke(vg);
      nvgBeginPath(vg);
      nvgArc(vg, cx, cy + size * 0.05f, size * 0.34f, -0.85f, 3.98f, NVG_CW);
      nvgStroke(vg);
      break;
    case ShellIcon::Cpu:
      nvgBeginPath(vg);
      nvgRoundedRect(vg, x + size * 0.28f, y + size * 0.28f, size * 0.44f, size * 0.44f, 2.0f);
      nvgStroke(vg);
      for (int i = 0; i < 3; ++i) {
        const float p = y + size * (0.28f + 0.14f * i);
        nvgBeginPath(vg); nvgMoveTo(vg, x + size * 0.16f, p); nvgLineTo(vg, x + size * 0.24f, p); nvgMoveTo(vg, x + size * 0.76f, p); nvgLineTo(vg, x + size * 0.84f, p); nvgStroke(vg);
      }
      break;
    case ShellIcon::Bolt:
      nvgBeginPath(vg);
      nvgMoveTo(vg, x + size * 0.56f, y + size * 0.12f);
      nvgLineTo(vg, x + size * 0.30f, y + size * 0.55f);
      nvgLineTo(vg, x + size * 0.52f, y + size * 0.55f);
      nvgLineTo(vg, x + size * 0.42f, y + size * 0.88f);
      nvgLineTo(vg, x + size * 0.72f, y + size * 0.42f);
      nvgLineTo(vg, x + size * 0.50f, y + size * 0.42f);
      nvgClosePath(vg);
      nvgFill(vg);
      break;
    case ShellIcon::Database:
      nvgBeginPath(vg);
      nvgEllipse(vg, cx, y + size * 0.28f, size * 0.30f, size * 0.12f);
      nvgMoveTo(vg, x + size * 0.20f, y + size * 0.28f);
      nvgLineTo(vg, x + size * 0.20f, y + size * 0.70f);
      nvgEllipse(vg, cx, y + size * 0.70f, size * 0.30f, size * 0.12f);
      nvgMoveTo(vg, x + size * 0.80f, y + size * 0.28f);
      nvgLineTo(vg, x + size * 0.80f, y + size * 0.70f);
      nvgStroke(vg);
      break;
    case ShellIcon::Refresh:
      nvgBeginPath(vg);
      nvgArc(vg, cx, cy, size * 0.30f, 0.2f, 5.1f, NVG_CW);
      nvgStroke(vg);
      nvgBeginPath(vg);
      nvgMoveTo(vg, x + size * 0.76f, y + size * 0.30f);
      nvgLineTo(vg, x + size * 0.84f, y + size * 0.18f);
      nvgLineTo(vg, x + size * 0.88f, y + size * 0.34f);
      nvgStroke(vg);
      break;
    case ShellIcon::ChevronDown:
      nvgBeginPath(vg);
      nvgMoveTo(vg, x + size * 0.28f, y + size * 0.40f);
      nvgLineTo(vg, cx, y + size * 0.62f);
      nvgLineTo(vg, x + size * 0.72f, y + size * 0.40f);
      nvgStroke(vg);
      break;
    case ShellIcon::Dots:
      for (int i = 0; i < 3; ++i) {
        nvgBeginPath(vg);
        nvgCircle(vg, x + size * (0.30f + i * 0.20f), cy, 1.4f);
        nvgFill(vg);
      }
      break;
  }
}

void draw_button(NVGcontext* vg, const ButtonRect& button, bool destructive = false) {
  const float x = static_cast<float>(button.rect.left);
  const float y = static_cast<float>(button.rect.top);
  const float w = static_cast<float>(button.rect.right - button.rect.left);
  const float h = static_cast<float>(button.rect.bottom - button.rect.top);
  nvgBeginPath(vg);
  nvgRoundedRect(vg, x, y, w, h, 6.0f);
  nvgFillColor(vg, destructive ? nvg_rgba(244, 135, 127, 34) : nvg_rgba(255, 255, 255, 10));
  nvgFill(vg);
  nvgStrokeWidth(vg, 1.0f);
  nvgStrokeColor(vg, destructive ? nvg_rgba(244, 135, 127, 96) : nvg_rgba(255, 255, 255, 24));
  nvgStroke(vg);
  nvgFillColor(vg, destructive ? nvg_rgb(244, 135, 127) : nvg_rgb(210, 219, 230));
  nvgFontSize(vg, 12.0f);
  nvgTextAlign(vg, NVG_ALIGN_CENTER | NVG_ALIGN_MIDDLE);
  nvgText(vg, x + w * 0.5f, y + h * 0.52f, button.label, nullptr);
}

bool editor_view_attached(const EditorWindowState* state) {
  return state && !state->has_error && state->vst3 && state->vst3->attached;
}

EditorFlexLayout calculate_editor_flex_layout(EditorWindowState* state) {
  EditorFlexLayout out{};
  RECT rc{};
  if (!state || !state->hwnd) return out;
  GetClientRect(state->hwnd, &rc);
  const float scale = state->scale;
  const float width = static_cast<float>((std::max)(1L, rc.right - rc.left));
  const float height = static_cast<float>((std::max)(1L, rc.bottom - rc.top));
  const bool attached = editor_view_attached(state);

  auto root = make_yoga_node();
  auto header = make_yoga_node();
  auto attach = make_yoga_node();
  YGNodeStyleSetFlexDirection(root.get(), YGFlexDirectionColumn);
  YGNodeStyleSetWidth(root.get(), width);
  YGNodeStyleSetHeight(root.get(), height);

  YGNodeStyleSetHeight(header.get(), 32.0f * scale);
  YGNodeStyleSetFlexGrow(attach.get(), 1.0f);
  YGNodeStyleSetMargin(attach.get(), YGEdgeLeft, (attached ? 1.0f : 8.0f) * scale);
  YGNodeStyleSetMargin(attach.get(), YGEdgeRight, (attached ? 1.0f : 8.0f) * scale);
  YGNodeStyleSetMargin(attach.get(), YGEdgeTop, (attached ? 2.0f : 10.0f) * scale);
  YGNodeStyleSetMargin(attach.get(), YGEdgeBottom, (attached ? 1.0f : 8.0f) * scale);
  YGNodeInsertChild(root.get(), header.get(), 0);
  YGNodeInsertChild(root.get(), attach.get(), 1);
  YGNodeCalculateLayout(root.get(), width, height, YGDirectionLTR);

  out.header = rect_from_node(header.get());
  out.attach = rect_from_node(attach.get());

  auto header_row = make_yoga_node();
  auto title_spacer = make_yoga_node();
  auto preset = make_yoga_node();
  auto bypass = make_yoga_node();
  auto reload = make_yoga_node();
  auto more = make_yoga_node();
  auto close = make_yoga_node();
  YGNodeStyleSetFlexDirection(header_row.get(), YGFlexDirectionRow);
  YGNodeStyleSetAlignItems(header_row.get(), YGAlignCenter);
  YGNodeStyleSetWidth(header_row.get(), width);
  YGNodeStyleSetHeight(header_row.get(), 32.0f * scale);
  YGNodeStyleSetPadding(header_row.get(), YGEdgeLeft, 8.0f * scale);
  YGNodeStyleSetPadding(header_row.get(), YGEdgeRight, 8.0f * scale);

  YGNodeStyleSetFlexGrow(title_spacer.get(), 1.0f);
  YGNodeStyleSetWidth(preset.get(), (std::min)(230.0f * scale, (std::max)(140.0f * scale, width * 0.28f)));
  YGNodeStyleSetHeight(preset.get(), 24.0f * scale);
  YGNodeStyleSetMargin(preset.get(), YGEdgeRight, 12.0f * scale);
  YGNodeStyleSetWidth(bypass.get(), 66.0f * scale);
  YGNodeStyleSetHeight(bypass.get(), 22.0f * scale);
  YGNodeStyleSetMargin(bypass.get(), YGEdgeRight, 8.0f * scale);
  YGNodeStyleSetWidth(reload.get(), 88.0f * scale);
  YGNodeStyleSetHeight(reload.get(), 22.0f * scale);
  YGNodeStyleSetMargin(reload.get(), YGEdgeRight, 8.0f * scale);
  YGNodeStyleSetWidth(more.get(), 32.0f * scale);
  YGNodeStyleSetHeight(more.get(), 22.0f * scale);
  YGNodeStyleSetMargin(more.get(), YGEdgeRight, 8.0f * scale);
  YGNodeStyleSetWidth(close.get(), 24.0f * scale);
  YGNodeStyleSetHeight(close.get(), 22.0f * scale);

  YGNodeInsertChild(header_row.get(), title_spacer.get(), 0);
  YGNodeInsertChild(header_row.get(), preset.get(), 1);
  YGNodeInsertChild(header_row.get(), bypass.get(), 2);
  YGNodeInsertChild(header_row.get(), reload.get(), 3);
  YGNodeInsertChild(header_row.get(), more.get(), 4);
  YGNodeInsertChild(header_row.get(), close.get(), 5);
  YGNodeCalculateLayout(header_row.get(), width, 32.0f * scale, YGDirectionLTR);
  out.preset = rect_from_node(preset.get());
  out.bypass = rect_from_node(bypass.get());
  out.reload = rect_from_node(reload.get());
  out.more = rect_from_node(more.get());
  out.close = rect_from_node(close.get());

  YGNodeRemoveAllChildren(header_row.get());
  YGNodeRemoveAllChildren(root.get());

  if (out.attach.right <= out.attach.left) out.attach.right = out.attach.left + 1;
  if (out.attach.bottom <= out.attach.top) out.attach.bottom = out.attach.top + 1;
  return out;
}

RECT attach_rect(EditorWindowState* state) {
  return calculate_editor_flex_layout(state).attach;
}

void cleanup_vst3_editor(EditorWindowState* state) {
  if (!state || !state->vst3) return;
  auto& vst3 = *state->vst3;
  if (vst3.view && vst3.attached) {
    vst3_clear_plug_frame(vst3);
    const auto removed_res = vst3.view->removed();
    std::fprintf(stderr,
                 "[SpherePluginHost] VST3 editor removed handle=%llu result=0x%x\n",
                 state->handle,
                 static_cast<unsigned>(removed_res));
    if (vst3_editor_debug()) {
      std::fprintf(stderr, "[vst3-editor] removed result=0x%x\n",
                   static_cast<unsigned>(removed_res));
    }
    vst3.attached = false;
  } else {
    vst3_clear_plug_frame(vst3);
  }
  if (vst3.controller) {
    vst3.controller->setComponentHandler(nullptr);
  }
  if (vst3.component_connection && vst3.controller_connection) {
    vst3.component_connection->disconnect(vst3.controller_connection);
    vst3.controller_connection->disconnect(vst3.component_connection);
  }
  vst3.component_connection = nullptr;
  vst3.controller_connection = nullptr;
  if (vst3.component) {
    if (auto plug_base = Steinberg::FUnknownPtr<Steinberg::IPluginBase>(vst3.component)) {
      plug_base->terminate();
    }
  }
  if (vst3.controller && !vst3.controller_is_component) {
    if (auto plug_base = Steinberg::FUnknownPtr<Steinberg::IPluginBase>(vst3.controller)) {
      plug_base->terminate();
    }
  }
  state->vst3.reset();
}

void set_attach_failed(EditorWindowState* state, const std::string& message) {
  if (!state) return;
  state->has_error = true;
  state->error = "VST3 editor attach failed: " + message;
  std::fprintf(stderr, "[SpherePluginHost] %s handle=%llu\n", state->error.c_str(), state->handle);
  if (state->attach_hwnd) ShowWindow(state->attach_hwnd, SW_HIDE);
  InvalidateRect(state->hwnd, nullptr, FALSE);
}

bool looks_like_zero_class_id(const std::string& value) {
  if (value.empty()) return true;
  for (char c : value) {
    if (c != '0' && c != '-' && c != '{' && c != '}') return false;
  }
  return true;
}

VST3::Optional<VST3::UID> first_audio_module_uid(const VST3::Hosting::PluginFactory& factory, std::string* resolved_name) {
  for (const auto& info : factory.classInfos()) {
    if (info.category() != kVst3AudioModuleClass) continue;
    if (resolved_name) *resolved_name = info.name();
    return VST3::Optional<VST3::UID>(info.ID());
  }
  return {};
}

void resize_vst3_view(EditorWindowState* state) {
  if (!state || !state->vst3 || !state->vst3->view || !state->vst3->attached) return;
  const RECT host_rc = attach_rect(state);
  Steinberg::ViewRect size{};
  size.left = 0;
  size.top = 0;
  size.right = host_rc.right - host_rc.left;
  size.bottom = host_rc.bottom - host_rc.top;
  state->vst3->view->onSize(&size);
}

bool attach_vst3_view_on_window_thread(EditorWindowState* state, const char* plugin_path, const char* class_id) {
  if (!state || !state->attach_hwnd || !plugin_path || !*plugin_path) {
    set_attach_failed(state, "missing editor state or plugin path");
    return false;
  }

  const std::string requested_path = plugin_path;
  const std::string requested_class_id = class_id ? class_id : "";
  if (state->vst3 && state->vst3->attached && state->vst3->plugin_path == requested_path) {
    const bool same_class =
        requested_class_id.empty() ||
        state->vst3->class_id == requested_class_id ||
        looks_like_zero_class_id(requested_class_id);
    if (same_class) {
      state->has_error = false;
      state->error.clear();
      if (state->attach_hwnd) ShowWindow(state->attach_hwnd, SW_SHOW);
      resize_vst3_view(state);
      InvalidateRect(state->hwnd, nullptr, FALSE);
      std::fprintf(
          stderr,
          "[SpherePluginHost] VST3 editor attach reused handle=%llu plugin=%s classId=%s\n",
          state->handle,
          state->vst3->plugin_path.c_str(),
          state->vst3->class_id.c_str());
      return true;
    }
  }

  cleanup_vst3_editor(state);

  auto attachment = std::make_unique<Vst3EditorAttachment>();
  attachment->plugin_path = requested_path;
  attachment->class_id = requested_class_id;

  std::string error;
  attachment->module = VST3::Hosting::Module::create(attachment->plugin_path, error);
  if (!attachment->module) {
    set_attach_failed(state, error.empty() ? "module load failed" : error);
    return false;
  }

  const auto factory = attachment->module->getFactory();
  factory.setHostContext(&attachment->host_context);

  VST3::Optional<VST3::UID> uid;
  if (!looks_like_zero_class_id(attachment->class_id)) {
    uid = VST3::UID::fromString(attachment->class_id);
  }

  std::string fallback_name;
  if (uid) {
    attachment->component = factory.createInstance<Steinberg::Vst::IComponent>(*uid);
    if (!attachment->component) {
      std::fprintf(
          stderr,
          "[SpherePluginHost] VST3 create component failed for supplied classId=%s; trying first Audio Module Class fallback\n",
          attachment->class_id.c_str());
      uid = first_audio_module_uid(factory, &fallback_name);
      if (uid) attachment->component = factory.createInstance<Steinberg::Vst::IComponent>(*uid);
    }
  } else {
    std::fprintf(
        stderr,
        "[SpherePluginHost] VST3 classId missing/zero/invalid; resolving first Audio Module Class fallback\n");
    uid = first_audio_module_uid(factory, &fallback_name);
    if (uid) attachment->component = factory.createInstance<Steinberg::Vst::IComponent>(*uid);
  }

  if (!attachment->component) {
    set_attach_failed(state, "failed to create VST3 component; no usable Audio Module Class found");
    return false;
  }

  if (uid) {
    attachment->class_id = uid->toString();
  }
  if (!fallback_name.empty()) {
    std::fprintf(
        stderr,
        "[SpherePluginHost] VST3 resolved fallback editor class name=%s classId=%s\n",
        fallback_name.c_str(),
        attachment->class_id.c_str());
  }

  if (auto component_base = Steinberg::FUnknownPtr<Steinberg::IPluginBase>(attachment->component)) {
    if (component_base->initialize(&attachment->host_context) != Steinberg::kResultOk) {
      set_attach_failed(state, "component initialize() failed");
      return false;
    }
  } else {
    set_attach_failed(state, "component does not implement IPluginBase");
    return false;
  }

  Steinberg::Vst::IEditController* raw_controller = nullptr;
  if (attachment->component->queryInterface(Steinberg::Vst::IEditController::iid, reinterpret_cast<void**>(&raw_controller)) == Steinberg::kResultTrue) {
    attachment->controller = Steinberg::IPtr<Steinberg::Vst::IEditController>::adopt(raw_controller);
    attachment->controller_is_component = true;
  } else {
    Steinberg::TUID controller_cid{};
    if (attachment->component->getControllerClassId(controller_cid) != Steinberg::kResultTrue) {
      set_attach_failed(state, "component did not provide controller classId");
      return false;
    }
    attachment->controller = factory.createInstance<Steinberg::Vst::IEditController>(VST3::UID(controller_cid));
    if (!attachment->controller) {
      set_attach_failed(state, "failed to create edit controller");
      return false;
    }
    if (auto controller_base = Steinberg::FUnknownPtr<Steinberg::IPluginBase>(attachment->controller)) {
      if (controller_base->initialize(&attachment->host_context) != Steinberg::kResultOk) {
        set_attach_failed(state, "controller initialize() failed");
        return false;
      }
    } else {
      set_attach_failed(state, "controller does not implement IPluginBase");
      return false;
    }
  }

  attachment->component_handler =
      Steinberg::IPtr<MinimalComponentHandler>::adopt(new MinimalComponentHandler(state->window_id));
  const auto component_handler_result = attachment->controller->setComponentHandler(attachment->component_handler);
  std::fprintf(
      stderr,
      "[SpherePluginHost] VST3 editor setComponentHandler result=%d windowId=%s\n",
      (int)component_handler_result,
      state->window_id.c_str());

  attachment->component_connection =
      Steinberg::FUnknownPtr<Steinberg::Vst::IConnectionPoint>(attachment->component);
  attachment->controller_connection =
      Steinberg::FUnknownPtr<Steinberg::Vst::IConnectionPoint>(attachment->controller);
  if (attachment->component_connection && attachment->controller_connection) {
    const auto component_connect = attachment->component_connection->connect(attachment->controller_connection);
    const auto controller_connect = attachment->controller_connection->connect(attachment->component_connection);
    std::fprintf(
        stderr,
        "[SpherePluginHost] VST3 editor component/controller connect componentResult=%d controllerResult=%d windowId=%s\n",
        (int)component_connect,
        (int)controller_connect,
        state->window_id.c_str());
  } else {
    std::fprintf(
        stderr,
        "[SpherePluginHost] VST3 editor component/controller connection unavailable windowId=%s\n",
        state->window_id.c_str());
  }

  attachment->view = Steinberg::IPtr<Steinberg::IPlugView>::adopt(
      attachment->controller->createView(Steinberg::Vst::ViewType::kEditor));
  if (!attachment->view) {
    set_attach_failed(state, "controller did not create editor view");
    return false;
  }

  if (attachment->view->isPlatformTypeSupported(Steinberg::kPlatformTypeHWND) != Steinberg::kResultTrue) {
    set_attach_failed(state, "editor view does not support HWND platform type");
    return false;
  }

  Steinberg::ViewRect preferred{};
  if (attachment->view->getSize(&preferred) == Steinberg::kResultTrue) {
    std::fprintf(
        stderr,
        "[SpherePluginHost] VST3 editor preferred size handle=%llu left=%d top=%d right=%d bottom=%d\n",
        state->handle,
        preferred.left,
        preferred.top,
        preferred.right,
        preferred.bottom);
  }

  vst3_install_plug_frame(*attachment, state->attach_hwnd, state->hwnd);

  const auto attach_result = attachment->view->attached(reinterpret_cast<void*>(state->attach_hwnd), Steinberg::kPlatformTypeHWND);
  std::fprintf(stderr,
               "[vst3-editor] attached result=0x%x host=0x%p\n",
               static_cast<unsigned>(attach_result),
               static_cast<void*>(state->attach_hwnd));
  if (attach_result != Steinberg::kResultTrue && attach_result != Steinberg::kResultOk) {
    vst3_clear_plug_frame(*attachment);
    set_attach_failed(state, "IPlugView::attached(HWND) failed");
    return false;
  }

  attachment->attached = true;
  state->vst3 = std::move(attachment);
  state->has_error = false;
  state->error.clear();
  resize_vst3_view(state);
  InvalidateRect(state->hwnd, nullptr, FALSE);
  std::fprintf(
      stderr,
      "[SpherePluginHost] VST3 editor attached handle=%llu mainHWND=0x%p childAttachHWND=0x%p plugin=%s classId=%s\n",
      state->handle,
      static_cast<void*>(state->hwnd),
      static_cast<void*>(state->attach_hwnd),
      plugin_path,
      state->vst3 ? state->vst3->class_id.c_str() : (class_id ? class_id : ""));
  return true;
}

void set_button(EditorWindowState* state, int index, ButtonAction action, float x, float y, float w, float h, const char* label) {
  if (!state || index < 0 || index >= 4) return;
  state->buttons[index] = ButtonRect{action, RECT{static_cast<LONG>(x), static_cast<LONG>(y), static_cast<LONG>(x + w), static_cast<LONG>(y + h)}, label};
}

void clear_buttons(EditorWindowState* state) {
  if (!state) return;
  for (auto& button : state->buttons) button = ButtonRect{};
}

void draw_pill(NVGcontext* vg, float x, float y, float w, float h, const char* label, ShellIcon icon, bool accent = false) {
  nvgBeginPath(vg);
  nvgRoundedRect(vg, x, y, w, h, 6.0f);
  nvgFillColor(vg, accent ? nvg_rgba(95, 206, 208, 34) : nvg_rgba(255, 255, 255, 10));
  nvgFill(vg);
  nvgStrokeWidth(vg, 1.0f);
  nvgStrokeColor(vg, accent ? nvg_rgba(95, 206, 208, 90) : nvg_rgba(255, 255, 255, 24));
  nvgStroke(vg);
  draw_icon(vg, icon, x + 7.0f, y + (h - 15.0f) * 0.5f, 15.0f, accent ? nvg_rgb(95, 206, 208) : nvg_rgb(154, 167, 184));
  nvgFontFace(vg, "sans-semibold");
  nvgFontSize(vg, 11.0f);
  nvgFillColor(vg, accent ? nvg_rgb(210, 245, 246) : nvg_rgb(210, 219, 230));
  nvgTextAlign(vg, NVG_ALIGN_LEFT | NVG_ALIGN_MIDDLE);
  nvgText(vg, x + 27.0f, y + h * 0.53f, label, nullptr);
}

void draw_shell_headerbar(EditorWindowState* state, NVGcontext* vg) {
  const EditorFlexLayout layout = calculate_editor_flex_layout(state);
  const float x = static_cast<float>(layout.header.left);
  const float y = static_cast<float>(layout.header.top);
  const float w = static_cast<float>(layout.header.right - layout.header.left);
  const float h = static_cast<float>(layout.header.bottom - layout.header.top);
  nvgBeginPath(vg);
  nvgRect(vg, x, y, w, h);
  nvgFillColor(vg, nvg_rgba(17, 22, 29, 250));
  nvgFill(vg);
  nvgBeginPath(vg);
  nvgRect(vg, x, h - 1.0f, w, 1.0f);
  nvgFillColor(vg, nvg_rgba(255, 255, 255, 22));
  nvgFill(vg);

  nvgFontFace(vg, "sans-semibold");
  nvgFontSize(vg, 12.0f);
  nvgFillColor(vg, nvg_rgb(241, 245, 249));
  nvgTextAlign(vg, NVG_ALIGN_LEFT | NVG_ALIGN_MIDDLE);
  draw_text_wide(vg, 10.0f, h * 0.42f, state->title);
  nvgFontFace(vg, "sans");
  nvgFontSize(vg, 10.0f);
  nvgFillColor(vg, nvg_rgb(154, 167, 184));
  draw_text_wide(vg, 10.0f, h * 0.75f, state->subtitle);

  const float preset_x = static_cast<float>(layout.preset.left);
  const float preset_y = static_cast<float>(layout.preset.top);
  const float preset_w = static_cast<float>(layout.preset.right - layout.preset.left);
  const float preset_h = static_cast<float>(layout.preset.bottom - layout.preset.top);
  nvgBeginPath(vg);
  nvgRoundedRect(vg, preset_x, preset_y, preset_w, preset_h, 5.0f);
  nvgFillColor(vg, nvg_rgba(255, 255, 255, 8));
  nvgFill(vg);
  nvgStrokeWidth(vg, 1.0f);
  nvgStrokeColor(vg, nvg_rgba(255, 255, 255, 22));
  nvgStroke(vg);
  nvgFontFace(vg, "sans-semibold");
  nvgFontSize(vg, 11.0f);
  nvgFillColor(vg, nvg_rgb(210, 219, 230));
  nvgTextAlign(vg, NVG_ALIGN_CENTER | NVG_ALIGN_MIDDLE);
  nvgText(vg, preset_x + preset_w * 0.5f, preset_y + preset_h * 0.52f, "Default Setting", nullptr);
  draw_icon(vg, ShellIcon::ChevronDown, preset_x + preset_w - 23.0f, preset_y + 5.0f, 12.0f, nvg_rgb(154, 167, 184));

  set_button(state, 0, ButtonAction::Close, static_cast<float>(layout.close.left), static_cast<float>(layout.close.top), static_cast<float>(layout.close.right - layout.close.left), static_cast<float>(layout.close.bottom - layout.close.top), "×");
  draw_button(vg, state->buttons[0], true);
  set_button(state, 1, ButtonAction::More, static_cast<float>(layout.more.left), static_cast<float>(layout.more.top), static_cast<float>(layout.more.right - layout.more.left), static_cast<float>(layout.more.bottom - layout.more.top), "•••");
  draw_button(vg, state->buttons[1]);
  set_button(state, 2, ButtonAction::ReloadEditor, static_cast<float>(layout.reload.left), static_cast<float>(layout.reload.top), static_cast<float>(layout.reload.right - layout.reload.left), static_cast<float>(layout.reload.bottom - layout.reload.top), "Reload Editor");
  draw_button(vg, state->buttons[2]);
  set_button(state, 3, ButtonAction::Bypass, static_cast<float>(layout.bypass.left), static_cast<float>(layout.bypass.top), static_cast<float>(layout.bypass.right - layout.bypass.left), static_cast<float>(layout.bypass.bottom - layout.bypass.top), "Bypass");
  draw_button(vg, state->buttons[3]);
}

void layout_attach_area(EditorWindowState* state) {
  if (!state || !state->attach_hwnd) return;
  const RECT attach = attach_rect(state);
  const int width = attach.right - attach.left;
  const int height = attach.bottom - attach.top;
  if (state->has_error) {
    ShowWindow(state->attach_hwnd, SW_HIDE);
  } else {
    SetWindowPos(
        state->attach_hwnd, nullptr, attach.left, attach.top, width, height,
        SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW);
  }
  resize_vst3_view(state);
  std::fprintf(
      stderr,
      "[SpherePluginHost] PluginEditor attach rect handle=%llu mainHWND=0x%p childAttachHWND=0x%p x=%d y=%d w=%d h=%d\n",
      state->handle,
      static_cast<void*>(state->hwnd),
      static_cast<void*>(state->attach_hwnd),
      attach.left,
      attach.top,
      width,
      height);
}

void draw_editor(EditorWindowState* state) {
  if (!state || !state->d3d.context || !state->d3d.rtv || !state->d3d.vg) return;
  RECT rc{};
  GetClientRect(state->hwnd, &rc);
  resize_d3d(state, rc.right - rc.left, rc.bottom - rc.top);
  ID3D11RenderTargetView* rtvs[] = {state->d3d.rtv.Get()};
  state->d3d.context->OMSetRenderTargets(1, rtvs, nullptr);
  D3D11_VIEWPORT vp{};
  vp.Width = static_cast<float>(state->d3d.width);
  vp.Height = static_cast<float>(state->d3d.height);
  vp.MinDepth = 0.0f;
  vp.MaxDepth = 1.0f;
  state->d3d.context->RSSetViewports(1, &vp);
  const float clear[4] = {14.0f / 255.0f, 19.0f / 255.0f, 25.0f / 255.0f, 1.0f};
  state->d3d.context->ClearRenderTargetView(state->d3d.rtv.Get(), clear);

  NVGcontext* vg = state->d3d.vg;
  nvgBeginFrame(vg, state->d3d.width, state->d3d.height, state->scale);

  nvgBeginPath(vg);
  nvgRect(vg, 0, 0, static_cast<float>(state->d3d.width), static_cast<float>(state->d3d.height));
  nvgFillColor(vg, nvg_rgb(14, 19, 25));
  nvgFill(vg);

  clear_buttons(state);
  draw_shell_headerbar(state, vg);

  if (state->has_error) {
    const int panel_w = (std::min)(state->d3d.width - sx(state, 48), sx(state, 720));
    const int panel_h = sx(state, 120);
    const int panel_x = (state->d3d.width - panel_w) / 2;
    const int panel_y = sx(state, 58);
    nvgBeginPath(vg);
    nvgRoundedRect(vg, static_cast<float>(panel_x), static_cast<float>(panel_y), static_cast<float>(panel_w), static_cast<float>(panel_h), 10.0f);
    nvgFillColor(vg, nvg_rgba(32, 38, 49, 245));
    nvgFill(vg);
    nvgStrokeWidth(vg, 1.0f);
    nvgStrokeColor(vg, nvg_rgba(244, 135, 127, 100));
    nvgStroke(vg);
    nvgFontFace(vg, "sans-semibold");
    nvgFontSize(vg, 12.0f);
    nvgFillColor(vg, nvg_rgb(244, 135, 127));
    nvgTextAlign(vg, NVG_ALIGN_LEFT | NVG_ALIGN_MIDDLE);
    nvgText(vg, static_cast<float>(panel_x + sx(state, 14)), static_cast<float>(panel_y + sx(state, 20)), "VST3 editor attach failed", nullptr);
    nvgFontFace(vg, "sans");
    nvgFontSize(vg, 10.5f);
    nvgFillColor(vg, nvg_rgb(210, 219, 230));
    nvgText(vg, static_cast<float>(panel_x + sx(state, 14)), static_cast<float>(panel_y + sx(state, 44)), state->error.c_str(), nullptr);

    const float button_y = static_cast<float>(panel_y + sx(state, 78));
    float button_x = static_cast<float>(panel_x + sx(state, 14));
    const float gap = static_cast<float>(sx(state, 8));
    const float widths[4] = {108.0f, 108.0f, 122.0f, 72.0f};
    const char* labels[4] = {"Reload Editor", "Reload Plugin", "Generic Params", "Close"};
    const ButtonAction actions[4] = {ButtonAction::ReloadEditor, ButtonAction::ReloadPlugin, ButtonAction::GenericParams, ButtonAction::Close};
    for (int i = 0; i < 4; ++i) {
      set_button(state, i, actions[i], button_x, button_y, widths[i], 24.0f, labels[i]);
      draw_button(vg, state->buttons[i], actions[i] == ButtonAction::Close);
      button_x += widths[i] + gap;
    }
  }

  const RECT attach = attach_rect(state);
  const int attach_x = attach.left;
  const int attach_y = attach.top;
  const int attach_w = attach.right - attach.left;
  const int attach_h = attach.bottom - attach.top;
  if (!state->has_error) {
    nvgBeginPath(vg);
    if (editor_view_attached(state)) {
      nvgRect(vg, static_cast<float>(attach_x), static_cast<float>(attach_y), static_cast<float>(attach_w), static_cast<float>(attach_h));
      nvgStrokeWidth(vg, 1.0f);
      nvgStrokeColor(vg, nvg_rgba(255, 255, 255, 20));
      nvgStroke(vg);
    } else {
      nvgRoundedRect(vg, static_cast<float>(attach_x), static_cast<float>(attach_y), static_cast<float>(attach_w), static_cast<float>(attach_h), 8.0f);
      nvgFillColor(vg, nvg_rgba(255, 255, 255, 8));
      nvgFill(vg);
      nvgStrokeWidth(vg, 1.0f);
      nvgStrokeColor(vg, nvg_rgba(255, 255, 255, 24));
      nvgStroke(vg);
      nvgFontFace(vg, "sans");
      nvgFontSize(vg, 12.0f);
      nvgFillColor(vg, nvg_rgb(107, 120, 136));
      nvgTextAlign(vg, NVG_ALIGN_CENTER | NVG_ALIGN_MIDDLE);
      nvgText(vg, attach_x + attach_w * 0.5f, attach_y + attach_h * 0.5f, "Vendor editor attach HWND reserved", nullptr);
    }
  }

  nvgEndFrame(vg);
  state->d3d.swap_chain->Present(1, 0);
}

void set_dark_titlebar(HWND hwnd) {
  BOOL dark = TRUE;
  DwmSetWindowAttribute(hwnd, 20, &dark, sizeof(dark));
  DwmSetWindowAttribute(hwnd, 19, &dark, sizeof(dark));
  DwmSetWindowAttribute(hwnd, DWMWA_CAPTION_COLOR, &kTitlebarDark, sizeof(kTitlebarDark));
}

bool point_in_rect(const RECT& rc, int x, int y) {
  return x >= rc.left && x <= rc.right && y >= rc.top && y <= rc.bottom;
}

void handle_button(EditorWindowState* state, ButtonAction action) {
  switch (action) {
    case ButtonAction::Close:
      ShowWindow(state->hwnd, SW_HIDE);
      break;
    case ButtonAction::ReloadEditor:
      std::fprintf(stderr, "[SpherePluginHost] Reload Editor requested for handle=%llu\n", state->handle);
      break;
    case ButtonAction::ReloadPlugin:
      std::fprintf(stderr, "[SpherePluginHost] Reload Plugin requested for handle=%llu\n", state->handle);
      break;
    case ButtonAction::GenericParams:
      std::fprintf(stderr, "[SpherePluginHost] Generic Params requested for handle=%llu\n", state->handle);
      break;
    case ButtonAction::Bypass:
      std::fprintf(stderr, "[SpherePluginHost] Bypass toggle requested for handle=%llu (placeholder)\n", state->handle);
      break;
    case ButtonAction::More:
      std::fprintf(stderr, "[SpherePluginHost] More menu requested for handle=%llu (placeholder)\n", state->handle);
      break;
  }
}

LRESULT CALLBACK editor_window_proc(HWND hwnd, UINT msg, WPARAM wparam, LPARAM lparam) {
  auto* state = reinterpret_cast<EditorWindowState*>(GetWindowLongPtrW(hwnd, GWLP_USERDATA));
  switch (msg) {
    case WM_NCCREATE: {
      auto* create = reinterpret_cast<CREATESTRUCTW*>(lparam);
      state = reinterpret_cast<EditorWindowState*>(create->lpCreateParams);
      state->hwnd = hwnd;
      SetWindowLongPtrW(hwnd, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(state));
      return TRUE;
    }
    case WM_CREATE:
      set_dark_titlebar(hwnd);
      if (state) {
        state->scale = dpi_scale(hwnd);
        state->attach_hwnd = CreateWindowExW(
            WS_EX_CONTROLPARENT,
            L"PluginViewHost",
            L"PluginViewHost",
            WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
            0,
            0,
            1,
            1,
            hwnd,
            nullptr,
            GetModuleHandleW(nullptr),
            nullptr);
        layout_attach_area(state);
        std::fprintf(
            stderr,
            "[SpherePluginHost] PluginEditor created handle=%llu windowId=%s mainHWND=0x%p childAttachHWND=0x%p\n",
            state->handle,
            state->window_id.c_str(),
            static_cast<void*>(state->hwnd),
            static_cast<void*>(state->attach_hwnd));
        g_windows_cv.notify_all();
        if (!init_d3d(state)) {
          state->has_error = true;
          state->error = "D3D11/NanoVG initialization failed.";
        }
        SetTimer(hwnd, kRedrawTimer, 1000 / 30, nullptr);
      }
      return 0;
    case WM_DPICHANGED:
      if (state) {
        state->scale = static_cast<float>(HIWORD(wparam)) / 96.0f;
        const RECT* suggested = reinterpret_cast<RECT*>(lparam);
        SetWindowPos(hwnd, nullptr, suggested->left, suggested->top, suggested->right - suggested->left, suggested->bottom - suggested->top, SWP_NOZORDER | SWP_NOACTIVATE);
        layout_attach_area(state);
      }
      return 0;
    case WM_SIZE:
      if (state) {
        resize_d3d(state, LOWORD(lparam), HIWORD(lparam));
        layout_attach_area(state);
        InvalidateRect(hwnd, nullptr, FALSE);
      }
      return 0;
    case WM_ATTACH_VST3_EDITOR:
      if (state) {
        auto* request = reinterpret_cast<AttachVst3Request*>(lparam);
        request->result = attach_vst3_view_on_window_thread(state, request->plugin_path, request->class_id) ? 1 : 0;
      }
      return 0;
    case WM_LBUTTONUP:
      if (state) {
        const int x = GET_X_LPARAM(lparam);
        const int y = GET_Y_LPARAM(lparam);
        for (const auto& button : state->buttons) {
          if (!button.label || !*button.label) continue;
          if (point_in_rect(button.rect, x, y)) {
            handle_button(state, button.action);
            return 0;
          }
        }
      }
      return 0;
    case WM_SETFOCUS:
      if (state && state->attach_hwnd) SetFocus(state->attach_hwnd);
      return 0;
    case WM_TIMER:
      if (wparam == kRedrawTimer) InvalidateRect(hwnd, nullptr, FALSE);
      return 0;
    case WM_PAINT: {
      PAINTSTRUCT ps{};
      BeginPaint(hwnd, &ps);
      if (state) draw_editor(state);
      EndPaint(hwnd, &ps);
      return 0;
    }
    case WM_CLOSE:
      if (state && !state->close_requested) {
        ShowWindow(hwnd, SW_HIDE);
        return 0;
      }
      DestroyWindow(hwnd);
      return 0;
    case WM_DESTROY:
      KillTimer(hwnd, kRedrawTimer);
      if (state) {
        std::fprintf(
            stderr,
            "[SpherePluginHost] PluginEditor destroying handle=%llu windowId=%s mainHWND=0x%p childAttachHWND=0x%p\n",
            state->handle,
            state->window_id.c_str(),
            static_cast<void*>(state->hwnd),
            static_cast<void*>(state->attach_hwnd));
        cleanup_vst3_editor(state);
        if (state->attach_hwnd && IsWindow(state->attach_hwnd)) {
          DestroyWindow(state->attach_hwnd);
          state->attach_hwnd = nullptr;
        }
        if (state->d3d.vg) {
          nvgDeleteD3D11(state->d3d.vg);
          state->d3d.vg = nullptr;
        }
        std::lock_guard<std::mutex> lock(g_windows_mutex);
        g_window_ids.erase(state->window_id);
        g_windows.erase(state->handle);
        g_windows_cv.notify_all();
      }
      return 0;
    default:
      return DefWindowProcW(hwnd, msg, wparam, lparam);
  }
}

void run_win32_editor(EditorWindowConfig* config) {
  SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
  const wchar_t* class_name = L"FutureboardPluginEditorWindow";
  const wchar_t* attach_class_name = L"PluginViewHost";
  WNDCLASSEXW wc{};
  wc.cbSize = sizeof(WNDCLASSEXW);
  wc.lpfnWndProc = editor_window_proc;
  wc.hInstance = GetModuleHandleW(nullptr);
  wc.hCursor = LoadCursor(nullptr, IDC_ARROW);
  wc.lpszClassName = class_name;
  RegisterClassExW(&wc);

  WNDCLASSEXW attach_wc{};
  attach_wc.cbSize = sizeof(WNDCLASSEXW);
  attach_wc.lpfnWndProc = DefWindowProcW;
  attach_wc.hInstance = GetModuleHandleW(nullptr);
  attach_wc.hCursor = LoadCursor(nullptr, IDC_ARROW);
  attach_wc.hbrBackground = reinterpret_cast<HBRUSH>(COLOR_WINDOW + 1);
  attach_wc.lpszClassName = attach_class_name;
  RegisterClassExW(&attach_wc);

  auto* state = new EditorWindowState();
  state->handle = config->handle;
  state->window_id = config->window_id;
  state->title = config->title;
  state->subtitle = config->subtitle;

  const UINT dpi = GetDpiForSystem();
  RECT window_rect{0, 0, MulDiv(config->width, dpi, 96), MulDiv(config->height, dpi, 96)};
  AdjustWindowRectExForDpi(&window_rect, WS_OVERLAPPEDWINDOW, FALSE, WS_EX_APPWINDOW, dpi);

  HWND hwnd = CreateWindowExW(
      WS_EX_APPWINDOW,
      class_name,
      config->title.c_str(),
      WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
      CW_USEDEFAULT,
      CW_USEDEFAULT,
      window_rect.right - window_rect.left,
      window_rect.bottom - window_rect.top,
      nullptr,
      nullptr,
      GetModuleHandleW(nullptr),
      state);

  if (!hwnd) {
    delete state;
    delete config;
    return;
  }

  {
    std::lock_guard<std::mutex> lock(g_windows_mutex);
    g_windows[config->handle] = state;
    g_window_ids[config->window_id] = config->handle;
    g_windows_cv.notify_all();
  }

  ShowWindow(hwnd, SW_SHOW);
  UpdateWindow(hwnd);
  // Pin always-on-top so the plugin editor floats above the DAW window.
  SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);

  MSG msg{};
  while (IsWindow(hwnd)) {
    while (PeekMessageW(&msg, nullptr, 0, 0, PM_REMOVE)) {
      TranslateMessage(&msg);
      DispatchMessageW(&msg);
    }
    MsgWaitForMultipleObjects(0, nullptr, FALSE, 16, QS_ALLINPUT);
  }

  delete state;
  delete config;
}
#endif

#ifdef _WIN32
// ── Embedded (GPUI-hosted) editor path ──────────────────────────────────────
//
// Instead of the C++ NanoVG top-level window above, GPUI owns a borderless
// external window and draws the shell/header. This path creates a WS_CHILD
// host region under the GPUI window's HWND and attaches the VST3 IPlugView into
// it. No NanoVG/D3D shell, no extra thread/message-pump — the child rides the
// GPUI window's event loop. Must be called on the GPUI UI thread (the thread
// that owns the parent HWND), never the audio thread.

// Build a VST3 attachment (module → component → controller → IPlugView) without
// attaching it to any window yet. Reuses the same helpers + shared param queue
// as the legacy path so Phase 5 drain works for either. Returns null + `error`
// on failure; never throws across the C ABI.

// Generic browser/WebView runtime compatibility layer (mirror of the DAUx
// detection in SphereDirectAudioEngine). Keyed off bundled marker files, not
// vendor names. See `daux_detect_editor_runtime` for the canonical reference.

enum class EmbedEditorRuntimeKind {
  Native = 0,
  WebView2 = 1,
  Cef = 2,
  Chromium = 3,
  BrowserUnknown = 4,
};

const char* embed_editor_runtime_kind_name(EmbedEditorRuntimeKind kind) {
  switch (kind) {
    case EmbedEditorRuntimeKind::WebView2: return "WebView2";
    case EmbedEditorRuntimeKind::Cef: return "Cef";
    case EmbedEditorRuntimeKind::Chromium: return "Chromium";
    case EmbedEditorRuntimeKind::BrowserUnknown: return "BrowserUnknown";
    case EmbedEditorRuntimeKind::Native:
    default: return "Native";
  }
}

bool embed_plugin_webview_based_debug() {
  static const bool enabled =
      std::getenv("FUTUREBOARD_PLUGIN_WEBVIEW_DEBUG") != nullptr;
  return enabled;
}

std::wstring embed_webview_runtime_arch_subdir() {
#if defined(_M_ARM64)
  return L"win-arm64";
#else
  return L"win-x64";
#endif
}

bool embed_path_exists_w(const std::wstring& path) {
  if (path.empty()) return false;
  const DWORD attrs = GetFileAttributesW(path.c_str());
  return attrs != INVALID_FILE_ATTRIBUTES && (attrs & FILE_ATTRIBUTE_DIRECTORY) == 0;
}

bool embed_dir_exists_w(const std::wstring& path) {
  if (path.empty()) return false;
  const DWORD attrs = GetFileAttributesW(path.c_str());
  return attrs != INVALID_FILE_ATTRIBUTES && (attrs & FILE_ATTRIBUTE_DIRECTORY) != 0;
}

std::wstring embed_join_path_w(std::wstring base, const wchar_t* suffix) {
  if (base.empty()) return suffix ? suffix : L"";
  while (!base.empty() && (base.back() == L'\\' || base.back() == L'/')) base.pop_back();
  if (!suffix || !*suffix) return base;
  std::wstring out = std::move(base);
  out.push_back(L'\\');
  out += suffix;
  return out;
}

bool embed_file_in_dir(const std::wstring& dir, const wchar_t* file) {
  return embed_path_exists_w(embed_join_path_w(dir, file));
}

void embed_push_dir_unique(std::vector<std::wstring>& dirs, const std::wstring& dir) {
  if (dir.empty()) return;
  for (const auto& e : dirs) {
    if (_wcsicmp(e.c_str(), dir.c_str()) == 0) return;
  }
  dirs.push_back(dir);
}

struct EmbedEditorRuntimeDetection {
  EmbedEditorRuntimeKind kind = EmbedEditorRuntimeKind::Native;
  std::vector<std::wstring> dll_dirs;
  std::wstring webview2_loader;
};

EmbedEditorRuntimeDetection embed_detect_editor_runtime(const std::string& plugin_path) {
  EmbedEditorRuntimeDetection out;
  if (plugin_path.empty()) return out;
  const std::wstring root = utf8_to_wide(plugin_path.c_str());
  const std::wstring arch = embed_webview_runtime_arch_subdir();

  static const wchar_t* kBaseRel[] = {
      L"", L"Contents\\Resources", L"Contents\\x86_64-win",
      L"Contents\\Resources\\WebView2", L"Contents\\Resources\\CEF",
      L"Contents\\Resources\\Chromium", L"Contents\\Resources\\Browser",
      L"Contents\\Resources\\runtimes", L"Contents\\Resources\\bin",
  };

  bool wv2 = false, cef = false, chromium = false, browser = false;
  for (const wchar_t* rel : kBaseRel) {
    const std::wstring base = (*rel) ? embed_join_path_w(root, rel) : root;
    if (!embed_dir_exists_w(base)) continue;

    const std::wstring runtimes_native = embed_join_path_w(
        embed_join_path_w(embed_join_path_w(base, L"runtimes"), arch.c_str()), L"native");
    const std::wstring arch_native =
        embed_join_path_w(embed_join_path_w(base, arch.c_str()), L"native");
    const std::wstring wv2_candidates[] = {base, runtimes_native, arch_native};
    for (const std::wstring& nd : wv2_candidates) {
      if (nd.empty() || !embed_dir_exists_w(nd)) continue;
      const std::wstring loader = embed_join_path_w(nd, L"WebViewLoader.dll");
      if (embed_path_exists_w(loader)) {
        wv2 = true;
        embed_push_dir_unique(out.dll_dirs, nd);
        if (out.webview2_loader.empty()) out.webview2_loader = loader;
      }
      if (embed_file_in_dir(nd, L"Microsoft.Web.WebView2.Core.dll")) {
        wv2 = true;
        embed_push_dir_unique(out.dll_dirs, nd);
      }
    }

    const bool has_libcef = embed_file_in_dir(base, L"libcef.dll");
    const bool has_chrome_elf = embed_file_in_dir(base, L"chrome_elf.dll");
    const bool has_cef_pak = embed_file_in_dir(base, L"cef.pak") ||
                             embed_file_in_dir(base, L"cef_100_percent.pak") ||
                             embed_file_in_dir(base, L"cef_200_percent.pak");
    const bool has_icu = embed_file_in_dir(base, L"icudtl.dat");
    const bool has_v8 = embed_file_in_dir(base, L"snapshot_blob.bin") ||
                        embed_file_in_dir(base, L"v8_context_snapshot.bin");
    const bool has_respak = embed_file_in_dir(base, L"resources.pak");

    if (has_libcef) {
      cef = true;
      embed_push_dir_unique(out.dll_dirs, base);
    }
    if (has_cef_pak) cef = true;
    if (has_chrome_elf) {
      embed_push_dir_unique(out.dll_dirs, base);
      if (!has_libcef && !has_cef_pak) chromium = true;
    }
    if (has_icu || has_v8 || has_respak) browser = true;
  }

  if (wv2) out.kind = EmbedEditorRuntimeKind::WebView2;
  else if (cef) out.kind = EmbedEditorRuntimeKind::Cef;
  else if (chromium) out.kind = EmbedEditorRuntimeKind::Chromium;
  else if (browser) out.kind = EmbedEditorRuntimeKind::BrowserUnknown;
  else out.kind = EmbedEditorRuntimeKind::Native;
  return out;
}

bool embed_plugin_is_browser_based(const std::string& plugin_path) {
  return embed_detect_editor_runtime(plugin_path).kind != EmbedEditorRuntimeKind::Native;
}

void embed_webview2_ensure_dll_search_policy() {
  static std::once_flag once;
  std::call_once(once, [] {
    if (!SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_DEFAULT_DIRS |
                                  LOAD_LIBRARY_SEARCH_USER_DIRS)) {
      std::fprintf(stderr, "[plugin-webview-based] SetDefaultDllDirectories failed err=%lu\n", GetLastError());
    } else if (embed_plugin_webview_based_debug()) {
      std::fprintf(stderr, "[plugin-webview-based] SetDefaultDllDirectories ok\n");
    }
  });
}

struct EmbedPluginWebViewBasedScope {
  std::vector<DLL_DIRECTORY_COOKIE> dll_cookies;
  HMODULE loader_module = nullptr;

  ~EmbedPluginWebViewBasedScope() {
    if (loader_module) FreeLibrary(loader_module);
    for (DLL_DIRECTORY_COOKIE c : dll_cookies) {
      if (c) RemoveDllDirectory(c);
    }
  }

  static std::unique_ptr<EmbedPluginWebViewBasedScope> try_create(const std::string& plugin_path,
                                                                  std::string& error) {
    const EmbedEditorRuntimeDetection det = embed_detect_editor_runtime(plugin_path);
    if (det.kind == EmbedEditorRuntimeKind::Native) {
      return nullptr;  // normal native UI plug-in
    }

    const bool debug =
        embed_plugin_webview_based_debug() || std::getenv("FUTUREBOARD_PLUGIN_VIEW_DEBUG") != nullptr;
    if (debug) {
      std::fprintf(stderr, "[plugin-webview-based] runtime=%s dll_dirs=%zu path=%s\n",
                   embed_editor_runtime_kind_name(det.kind), det.dll_dirs.size(), plugin_path.c_str());
    }

    if (det.dll_dirs.empty()) {
      return nullptr;  // detected via resources only — nothing to add, not fatal
    }

    embed_webview2_ensure_dll_search_policy();
    auto scope = std::make_unique<EmbedPluginWebViewBasedScope>();
    for (const std::wstring& dir : det.dll_dirs) {
      DLL_DIRECTORY_COOKIE cookie = AddDllDirectory(dir.c_str());
      if (!cookie) {
        error = "Failed to configure plugin browser runtime search path (AddDllDirectory err=" +
                std::to_string(GetLastError()) + ")";
        return nullptr;  // ~scope rolls back already-added dirs
      }
      scope->dll_cookies.push_back(cookie);
    }
    if (debug) std::fprintf(stderr, "[plugin-webview-based] AddDllDirectory ok\n");

    if (!det.webview2_loader.empty()) {
      scope->loader_module = LoadLibraryW(det.webview2_loader.c_str());
      if (!scope->loader_module) {
        error = std::string("Failed to load plugin WebView2 runtime (GetLastError=") +
                std::to_string(GetLastError()) + ")";
        return nullptr;
      }
      if (debug) std::fprintf(stderr, "[plugin-webview-based] LoadLibrary WebViewLoader.dll ok\n");
    }
    return scope;
  }
};

bool embed_prepare_plugin_webview_based(Vst3EditorAttachment* attachment, std::string& error) {
  if (!attachment) return false;
  auto scope = EmbedPluginWebViewBasedScope::try_create(attachment->plugin_path, error);
  if (!scope) return error.empty(); // not browser-based, or detected-via-resources — ok
  attachment->plugin_webview_based_scope = std::move(scope);
  return true;
}

Vst3EditorAttachment::~Vst3EditorAttachment() = default;

std::unique_ptr<Vst3EditorAttachment> build_vst3_attachment(
    const std::string& plugin_path,
    const std::string& class_id,
    const std::string& window_id,
    std::string& error) {
  auto attachment = std::make_unique<Vst3EditorAttachment>();
  attachment->plugin_path = plugin_path;
  attachment->class_id = class_id;

  attachment->module = VST3::Hosting::Module::create(attachment->plugin_path, error);
  if (!attachment->module) {
    if (error.empty()) error = "module load failed";
    return nullptr;
  }

  const auto factory = attachment->module->getFactory();
  factory.setHostContext(&attachment->host_context);

  VST3::Optional<VST3::UID> uid;
  std::string fallback_name;
  if (!looks_like_zero_class_id(attachment->class_id)) {
    uid = VST3::UID::fromString(attachment->class_id);
  }
  if (uid) {
    attachment->component = factory.createInstance<Steinberg::Vst::IComponent>(*uid);
    if (!attachment->component) {
      uid = first_audio_module_uid(factory, &fallback_name);
      if (uid) attachment->component = factory.createInstance<Steinberg::Vst::IComponent>(*uid);
    }
  } else {
    uid = first_audio_module_uid(factory, &fallback_name);
    if (uid) attachment->component = factory.createInstance<Steinberg::Vst::IComponent>(*uid);
  }
  if (!attachment->component) {
    error = "failed to create VST3 component; no usable Audio Module Class found";
    return nullptr;
  }
  if (uid) attachment->class_id = uid->toString();

  if (auto component_base = Steinberg::FUnknownPtr<Steinberg::IPluginBase>(attachment->component)) {
    if (component_base->initialize(&attachment->host_context) != Steinberg::kResultOk) {
      error = "component initialize() failed";
      return nullptr;
    }
  } else {
    error = "component does not implement IPluginBase";
    return nullptr;
  }

  Steinberg::Vst::IEditController* raw_controller = nullptr;
  if (attachment->component->queryInterface(Steinberg::Vst::IEditController::iid, reinterpret_cast<void**>(&raw_controller)) == Steinberg::kResultTrue) {
    attachment->controller = Steinberg::IPtr<Steinberg::Vst::IEditController>::adopt(raw_controller);
    attachment->controller_is_component = true;
  } else {
    Steinberg::TUID controller_cid{};
    if (attachment->component->getControllerClassId(controller_cid) != Steinberg::kResultTrue) {
      error = "component did not provide controller classId";
      return nullptr;
    }
    attachment->controller = factory.createInstance<Steinberg::Vst::IEditController>(VST3::UID(controller_cid));
    if (!attachment->controller) {
      error = "failed to create edit controller";
      return nullptr;
    }
    if (auto controller_base = Steinberg::FUnknownPtr<Steinberg::IPluginBase>(attachment->controller)) {
      if (controller_base->initialize(&attachment->host_context) != Steinberg::kResultOk) {
        error = "controller initialize() failed";
        return nullptr;
      }
    } else {
      error = "controller does not implement IPluginBase";
      return nullptr;
    }
  }

  attachment->component_handler =
      Steinberg::IPtr<MinimalComponentHandler>::adopt(new MinimalComponentHandler(window_id));
  attachment->controller->setComponentHandler(attachment->component_handler);

  attachment->component_connection =
      Steinberg::FUnknownPtr<Steinberg::Vst::IConnectionPoint>(attachment->component);
  attachment->controller_connection =
      Steinberg::FUnknownPtr<Steinberg::Vst::IConnectionPoint>(attachment->controller);
  if (attachment->component_connection && attachment->controller_connection) {
    attachment->component_connection->connect(attachment->controller_connection);
    attachment->controller_connection->connect(attachment->component_connection);
  }

  if (!embed_prepare_plugin_webview_based(attachment.get(), error)) {
    return nullptr;
  }

  attachment->view = Steinberg::IPtr<Steinberg::IPlugView>::adopt(
      attachment->controller->createView(Steinberg::Vst::ViewType::kEditor));
  if (!attachment->view) {
    if (embed_plugin_is_browser_based(attachment->plugin_path)) {
      error = "Browser/WebView-based plugin editor createView failed (controller returned null view)";
    } else if (error.empty()) {
      error = "controller did not create editor view";
    }
    return nullptr;
  }
  if (attachment->view->isPlatformTypeSupported(Steinberg::kPlatformTypeHWND) != Steinberg::kResultTrue) {
    error = "editor view does not support HWND platform type";
    return nullptr;
  }
  return attachment;
}

enum class EmbedHostKind : std::uint8_t {
  // WS_CHILD under the GPUI HWND (often blank when gpui swapchain covers children).
  WsChild = 0,
  // Owned WS_POPUP tool window at screen coords — avoids gpui compositor stacking.
  OwnedToolWindow = 1,
};

struct EmbedSession {
  EmbedHostKind host_kind = EmbedHostKind::WsChild;
  HWND child = nullptr;   // host surface passed to IPlugView::attached
  HWND parent = nullptr;  // GPUI PluginView top-level HWND (owner for tool mode)
  int host_x = 0;
  int host_y = 0;
  int host_w = 0;
  int host_h = 0;
  int preferred_w = 0;
  int preferred_h = 0;
  // Last applied window rect (screen coords for tool mode, client coords for
  // WsChild). Used to skip redundant SetWindowPos/onSize/raise so idle frames
  // never re-flush geometry (Part D — no flicker / no resize spam).
  bool geometry_valid = false;
  RECT last_applied{};
  unsigned long long reposition_count = 0;
  std::unique_ptr<Vst3EditorAttachment> vst3;
  std::string window_id;
};

std::unordered_map<unsigned long long, std::unique_ptr<EmbedSession>> g_embed_sessions; // guarded by g_windows_mutex

struct PrepareSession {
  std::unique_ptr<Vst3EditorAttachment> vst3;
  int preferred_w = 0;
  int preferred_h = 0;
  bool have_preferred = false;
  std::string window_id;
};

std::unordered_map<unsigned long long, std::unique_ptr<PrepareSession>> g_prepare_sessions;
std::atomic<unsigned long long> g_next_prepare_handle{1};

bool embed_debug() {
  static const bool enabled = std::getenv("FUTUREBOARD_PLUGIN_VIEW_DEBUG") != nullptr;
  return enabled;
}

const char* embed_vst3_result_name(Steinberg::tresult result) {
  if (result == Steinberg::kResultOk) return "kResultOk";
  if (result == Steinberg::kResultTrue) return "kResultTrue";
  if (result == Steinberg::kResultFalse) return "kResultFalse";
  return "other";
}

LRESULT CALLBACK embed_child_wndproc(HWND hwnd, UINT msg, WPARAM wparam, LPARAM lparam) {
  // The plugin parents its own view window inside this child; we just host it.
  // Paint a solid black backing so there is no flash before the plugin draws,
  // and so anything outside the plugin's own view stays inside our bounds.
  if (msg == WM_ERASEBKGND) {
    // GPU/OpenGL plugin children paint their own pixels — erasing the host
    // background can flash over or fight nested GL/DComp child HWNDs.
    if (GetWindow(hwnd, GW_CHILD) != nullptr) {
      return 1;
    }
    HDC hdc = reinterpret_cast<HDC>(wparam);
    RECT rc{};
    GetClientRect(hwnd, &rc);
    FillRect(hdc, &rc, reinterpret_cast<HBRUSH>(GetStockObject(BLACK_BRUSH)));
    return 1;
  }
  return DefWindowProcW(hwnd, msg, wparam, lparam);
}

const wchar_t* kEmbedChildClass = L"SpherePluginEmbedHost";
const wchar_t* kEmbedToolClass = L"SpherePluginEmbedTool";

void register_embed_window_class(const wchar_t* class_name) {
  WNDCLASSEXW wc{};
  wc.cbSize = sizeof(wc);
  wc.lpfnWndProc = embed_child_wndproc;
  wc.hInstance = GetModuleHandleW(nullptr);
  wc.hCursor = LoadCursorW(nullptr, reinterpret_cast<LPCWSTR>(IDC_ARROW));
  wc.hbrBackground = reinterpret_cast<HBRUSH>(GetStockObject(BLACK_BRUSH));
  wc.lpszClassName = class_name;
  RegisterClassExW(&wc);
}

void ensure_embed_child_class() {
  static std::once_flag once;
  std::call_once(once, []() { register_embed_window_class(kEmbedChildClass); });
}

void ensure_embed_tool_class() {
  static std::once_flag once;
  std::call_once(once, []() { register_embed_window_class(kEmbedToolClass); });
}

EmbedHostKind embed_resolve_host_kind() {
  const char* mode = std::getenv("FUTUREBOARD_PLUGIN_EDITOR_MODE");
  if (mode && *mode) {
    if (_stricmp(mode, "child") == 0 || _stricmp(mode, "ws_child") == 0) {
      return EmbedHostKind::WsChild;
    }
    if (_stricmp(mode, "tool") == 0 || _stricmp(mode, "owned") == 0 ||
        _stricmp(mode, "popup") == 0) {
      return EmbedHostKind::OwnedToolWindow;
    }
  }
  // Default for the external bridge's main-owned mode: a real child HWND under
  // the main-app-owned content HWND. Tool/popup modes are explicit fallbacks.
  return EmbedHostKind::WsChild;
}

const char* embed_host_kind_name(EmbedHostKind kind) {
  return kind == EmbedHostKind::OwnedToolWindow ? "OwnedToolWindowFallback" : "ChildHwndEmbed";
}

void embed_log_window_styles(const char* label, HWND hwnd) {
  if (!embed_debug() || !hwnd || !IsWindow(hwnd)) return;
  const LONG_PTR style = GetWindowLongPtr(hwnd, GWL_STYLE);
  const LONG_PTR exstyle = GetWindowLongPtr(hwnd, GWL_EXSTYLE);
  const HWND owner = reinterpret_cast<HWND>(GetWindowLongPtrW(hwnd, GWLP_HWNDPARENT));
  RECT wr{};
  GetWindowRect(hwnd, &wr);
  std::fprintf(
      stderr,
      "[plugin-view] %s hwnd=0x%p owner=0x%p style=0x%08lx exstyle=0x%08lx "
      "rect=(%ld,%ld,%ld,%ld) APPWINDOW=%d TOOLWINDOW=%d\n",
      label,
      static_cast<void*>(hwnd),
      static_cast<void*>(owner),
      static_cast<unsigned long>(style),
      static_cast<unsigned long>(exstyle),
      wr.left,
      wr.top,
      wr.right,
      wr.bottom,
      (exstyle & WS_EX_APPWINDOW) ? 1 : 0,
      (exstyle & WS_EX_TOOLWINDOW) ? 1 : 0);
}

void embed_apply_owned_overlay_styles(HWND overlay, HWND owner) {
  if (!overlay || !IsWindow(overlay)) return;
  LONG_PTR ex = GetWindowLongPtr(overlay, GWL_EXSTYLE);
  ex &= ~WS_EX_APPWINDOW;
  ex |= WS_EX_TOOLWINDOW;
  SetWindowLongPtr(overlay, GWL_EXSTYLE, ex);
  if (owner && IsWindow(owner)) {
    SetWindowLongPtrW(overlay, GWLP_HWNDPARENT, reinterpret_cast<LONG_PTR>(owner));
  }
  SetWindowLongPtrW(overlay, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(owner));
}

void embed_apply_toolwindow_to_descendants(HWND root) {
  if (!root || !IsWindow(root)) return;
  embed_apply_owned_overlay_styles(root, nullptr);
  EnumChildWindows(
      root,
      [](HWND hwnd, LPARAM) -> BOOL {
        LONG_PTR ex = GetWindowLongPtr(hwnd, GWL_EXSTYLE);
        ex &= ~WS_EX_APPWINDOW;
        ex |= WS_EX_TOOLWINDOW;
        SetWindowLongPtr(hwnd, GWL_EXSTYLE, ex);
        return TRUE;
      },
      0);
}

bool embed_content_screen_rect(HWND parent, int x, int y, int w, int h, RECT* out) {
  if (!parent || !IsWindow(parent) || !out || w <= 0 || h <= 0) return false;
  POINT top_left{x, y};
  POINT bottom_right{x + w, y + h};
  if (!ClientToScreen(parent, &top_left) || !ClientToScreen(parent, &bottom_right)) {
    return false;
  }
  out->left = top_left.x;
  out->top = top_left.y;
  out->right = bottom_right.x;
  out->bottom = bottom_right.y;
  return true;
}

DWORD embed_window_thread_id(HWND hwnd) {
  if (!hwnd || !IsWindow(hwnd)) return 0;
  DWORD tid = 0;
  GetWindowThreadProcessId(hwnd, &tid);
  return tid;
}

void embed_audit_log_threads(HWND parent, HWND child) {
  const DWORD parent_tid = embed_window_thread_id(parent);
  const DWORD child_tid = embed_window_thread_id(child);
  const DWORD attach_tid = GetCurrentThreadId();
  std::fprintf(
      stderr,
      "[vst3-editor-audit] threads parent_tid=%lu child_tid=%lu attach_tid=%lu "
      "parent_match=%d child_match=%d\n",
      static_cast<unsigned long>(parent_tid),
      static_cast<unsigned long>(child_tid),
      static_cast<unsigned long>(attach_tid),
      parent_tid == attach_tid ? 1 : 0,
      child_tid == attach_tid ? 1 : 0);
  if (parent_tid != attach_tid) {
    std::fprintf(
        stderr,
        "[vst3-editor-audit] WARNING: attach called off the GPUI window thread — "
        "HWND/UI calls may not paint until the main message loop runs\n");
  }
}

void embed_ensure_parent_clip_children(HWND parent) {
  if (!parent || !IsWindow(parent)) return;
  const LONG_PTR style = GetWindowLongPtr(parent, GWL_STYLE);
  if (!(style & WS_CLIPCHILDREN)) {
    SetWindowLongPtr(parent, GWL_STYLE, style | WS_CLIPCHILDREN);
    std::fprintf(stderr, "[vst3-editor-audit] parent WS_CLIPCHILDREN added\n");
  }
  const LONG_PTR exstyle = GetWindowLongPtr(parent, GWL_EXSTYLE);
  if (exstyle & WS_EX_LAYERED) {
    std::fprintf(
        stderr,
        "[vst3-editor-audit] WARNING: parent has WS_EX_LAYERED — child embed may not paint\n");
  }
  // The GPUI flag is a main-app-only renderer workaround and must NOT be
  // inherited here (spec Part 1/2) — it should now read <unset> in the host. A
  // PluginHost-specific opt-in re-enables the workaround for the host alone,
  // without reusing the GPUI flag.
  const BOOL gpui_dcomp = std::getenv("GPUI_DISABLE_DIRECT_COMPOSITION") != nullptr;
  std::fprintf(
      stderr,
      "[vst3-editor-audit] GPUI_DISABLE_DIRECT_COMPOSITION=%s\n",
      gpui_dcomp ? "set" : "<unset>");
  const BOOL host_dcomp =
      std::getenv("FUTUREBOARD_PLUGIN_HOST_DISABLE_DIRECT_COMPOSITION") != nullptr;
  std::fprintf(
      stderr,
      "[vst3-editor-audit] FUTUREBOARD_PLUGIN_HOST_DISABLE_DIRECT_COMPOSITION=%s\n",
      host_dcomp ? "set" : "<unset>");
}

HWND embed_create_host_window(
    HWND parent,
    EmbedHostKind kind,
    int x,
    int y,
    int w,
    int h) {
  if (kind == EmbedHostKind::OwnedToolWindow) {
    ensure_embed_tool_class();
    POINT origin{x, y};
    ClientToScreen(parent, &origin);
    RECT screen{};
    if (!embed_content_screen_rect(parent, x, y, w, h, &screen)) {
      return nullptr;
    }
    const int screen_w = screen.right - screen.left;
    const int screen_h = screen.bottom - screen.top;
    HWND tool = CreateWindowExW(
        WS_EX_TOOLWINDOW,
        kEmbedToolClass,
        L"",
        WS_POPUP | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
        screen.left,
        screen.top,
        screen_w,
        screen_h,
        parent, // owner — keeps z-order with PluginView, no taskbar entry
        nullptr,
        GetModuleHandleW(nullptr),
        nullptr);
    if (tool) {
      embed_apply_owned_overlay_styles(tool, parent);
      ShowWindow(tool, SW_SHOWNA);
    }
    if (embed_debug()) {
      std::fprintf(
          stderr,
          "[plugin-view] create OwnedToolWindowFallback owner=0x%p overlay=0x%p "
          "content_screen=(%ld,%ld,%ld,%ld)\n",
          static_cast<void*>(parent),
          static_cast<void*>(tool),
          screen.left,
          screen.top,
          screen.right,
          screen.bottom);
      embed_log_window_styles("overlay", tool);
      embed_log_window_styles("plugin_view", parent);
    }
    return tool;
  }

  ensure_embed_child_class();
  return CreateWindowExW(
      0,
      kEmbedChildClass,
      L"",
      WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
      x,
      y,
      w,
      h,
      parent,
      nullptr,
      GetModuleHandleW(nullptr),
      nullptr);
}

// Push the host content rect into the plugin view. The child window is sized to
// the host region; the plugin view fills the child (plugin-local origin 0,0).
// Always called on the thread owning the parent HWND, never the audio thread.
// Returns the IPlugView::onSize tresult (or kResultFalse when nothing to size).
Steinberg::tresult embed_resize_view(EmbedSession* session, bool audit_log) {
  if (!session || !session->child || !session->vst3 || !session->vst3->view ||
      !session->vst3->attached) {
    return Steinberg::kResultFalse;
  }
  RECT rc{};
  GetClientRect(session->child, &rc);
  Steinberg::ViewRect size{};
  size.left = 0;
  size.top = 0;
  size.right = rc.right - rc.left;
  size.bottom = rc.bottom - rc.top;
  const auto result = session->vst3->view->onSize(&size);
  std::fprintf(
      stderr,
      "[plugin-host] onSize result=%d rect=(0,0,%d,%d)\n",
      (int)result,
      size.right - size.left,
      size.bottom - size.top);
  if (audit_log || embed_debug()) {
    std::fprintf(
        stderr,
        "[vst3-editor-audit] onSize plugin-local rect=(%d,%d,%d,%d) result=%s(%d) "
        "client=%dx%d\n",
        size.left,
        size.top,
        size.right,
        size.bottom,
        embed_vst3_result_name(result),
        (int)result,
        size.right - size.left,
        size.bottom - size.top);
  }
  return result;
}

// Enumerate windows the plugin parented under our host child and log each one.
// If the plugin draws directly into our child (no sub-windows), the count is 0,
// which is expected for some editors. A non-zero count whose GetParent != child
// would indicate the plugin parented its view to the wrong HWND.
BOOL CALLBACK embed_enum_child_log(HWND hwnd, LPARAM lparam) {
  auto* count = reinterpret_cast<int*>(lparam);
  *count += 1;
  wchar_t cls[160] = {0};
  wchar_t txt[160] = {0};
  GetClassNameW(hwnd, cls, 160);
  GetWindowTextW(hwnd, txt, 160);
  RECT r{};
  GetWindowRect(hwnd, &r);
  const LONG_PTR style = GetWindowLongPtr(hwnd, GWL_STYLE);
  std::fprintf(
      stderr,
      "[vst3-editor] child window #%d hwnd=0x%p parent=0x%p class=%ls text=%ls "
      "rect=(%ld,%ld,%ld,%ld) visible=%d style=0x%08lx\n",
      *count,
      static_cast<void*>(hwnd),
      static_cast<void*>(GetParent(hwnd)),
      cls,
      txt,
      r.left,
      r.top,
      r.right,
      r.bottom,
      IsWindowVisible(hwnd) ? 1 : 0,
      static_cast<unsigned long>(style));
  return TRUE;
}

// Post-attach Win32 visibility audit (always logged to stderr).
void embed_audit_log_child_state(HWND child, HWND parent) {
  RECT wr{};
  RECT cr{};
  GetWindowRect(child, &wr);
  GetClientRect(child, &cr);
  const LONG_PTR style = GetWindowLongPtr(child, GWL_STYLE);
  const LONG_PTR exstyle = GetWindowLongPtr(child, GWL_EXSTYLE);
  const HWND owner = reinterpret_cast<HWND>(GetWindowLongPtrW(child, GWLP_HWNDPARENT));
  std::fprintf(
      stderr,
      "[vst3-editor-audit] host_hwnd=0x%p IsWindow=%d IsWindowVisible=%d "
      "GetParent=0x%p owner=0x%p (gpui parent=0x%p) owner_match=%d\n",
      static_cast<void*>(child),
      IsWindow(child) ? 1 : 0,
      IsWindowVisible(child) ? 1 : 0,
      static_cast<void*>(GetParent(child)),
      static_cast<void*>(owner),
      static_cast<void*>(parent),
      owner == parent ? 1 : 0);
  std::fprintf(
      stderr,
      "[vst3-editor-audit] GetWindowRect=(%ld,%ld,%ld,%ld) GetClientRect=(%ld,%ld,%ld,%ld)\n",
      wr.left,
      wr.top,
      wr.right,
      wr.bottom,
      cr.left,
      cr.top,
      cr.right,
      cr.bottom);
  std::fprintf(
      stderr,
      "[vst3-editor-audit] GWL_STYLE=0x%08lx GWL_EXSTYLE=0x%08lx "
      "(WS_CHILD=%d WS_VISIBLE=%d WS_CLIPSIBLINGS=%d WS_CLIPCHILDREN=%d WS_POPUP=%d)\n",
      static_cast<unsigned long>(style),
      static_cast<unsigned long>(exstyle),
      (style & WS_CHILD) ? 1 : 0,
      (style & WS_VISIBLE) ? 1 : 0,
      (style & WS_CLIPSIBLINGS) ? 1 : 0,
      (style & WS_CLIPCHILDREN) ? 1 : 0,
      (style & WS_POPUP) ? 1 : 0);
}

// Raise the host child and any plugin-owned HWNDs above sibling windows.
void embed_raise_plugin_hwnds(HWND child) {
  SetWindowPos(child, HWND_TOP, 0, 0, 0, 0,
               SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
  EnumChildWindows(
      child,
      [](HWND hwnd, LPARAM) -> BOOL {
        if (!IsWindow(hwnd)) return TRUE;
        ShowWindow(hwnd, SW_SHOW);
        SetWindowPos(hwnd, HWND_TOP, 0, 0, 0, 0,
                     SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
        return TRUE;
      },
      0);
}

struct EmbedRefreshChildCtx {
  int width = 0;
  int height = 0;
};

BOOL CALLBACK embed_refresh_plugin_child(HWND hwnd, LPARAM lparam) {
  const auto* ctx = reinterpret_cast<const EmbedRefreshChildCtx*>(lparam);
  const LPARAM size_lp = MAKELPARAM(ctx->width, ctx->height);
  ShowWindow(hwnd, SW_SHOW);
  SendMessageW(hwnd, WM_SHOWWINDOW, TRUE, 0);
  SendMessageW(hwnd, WM_SIZE, SIZE_RESTORED, size_lp);
  InvalidateRect(hwnd, nullptr, TRUE);
  UpdateWindow(hwnd);
  return TRUE;
}

void embed_post_attach_refresh(HWND child, int w, int h) {
  if (!child || !IsWindow(child)) return;
  const LPARAM size_lp = MAKELPARAM(w, h);
  SetWindowPos(child, HWND_TOP, 0, 0, w, h, SWP_SHOWWINDOW | SWP_NOACTIVATE);
  ShowWindow(child, SW_SHOW);
  SendMessageW(child, WM_SHOWWINDOW, TRUE, 0);
  SendMessageW(child, WM_SIZE, SIZE_RESTORED, size_lp);
  embed_raise_plugin_hwnds(child);
  EmbedRefreshChildCtx ctx{w, h};
  EnumChildWindows(child, embed_refresh_plugin_child, reinterpret_cast<LPARAM>(&ctx));
  InvalidateRect(child, nullptr, TRUE);
  UpdateWindow(child);
  RedrawWindow(child, nullptr, nullptr, RDW_INVALIDATE | RDW_UPDATENOW | RDW_ALLCHILDREN);
  std::fprintf(stderr, "[gpu-editor] post_attach_show_resize_redraw\n");
}

bool embed_class_looks_gpu(const wchar_t* class_name) {
  if (!class_name || !class_name[0]) return false;
  auto contains = [&](const wchar_t* token) {
    return wcsstr(class_name, token) != nullptr;
  };
  return contains(L"JUCE") || contains(L"OpenGL") || contains(L"Chrome") ||
         contains(L"WebView") || contains(L"CEF") || contains(L"ANGLE");
}

struct EmbedGpuDetectCtx {
  bool detected = false;
  int child_count = 0;
};

BOOL CALLBACK embed_gpu_detect_child(HWND hwnd, LPARAM lparam) {
  auto* ctx = reinterpret_cast<EmbedGpuDetectCtx*>(lparam);
  if (!hwnd || !IsWindow(hwnd)) return TRUE;
  ctx->child_count++;
  wchar_t class_name[256]{};
  const int len = GetClassNameW(hwnd, class_name, static_cast<int>(std::size(class_name)));
  if (len > 0 && embed_class_looks_gpu(class_name)) {
    ctx->detected = true;
  }
  EnumChildWindows(hwnd, embed_gpu_detect_child, lparam);
  return TRUE;
}

bool embed_detect_gpu_children(HWND root) {
  if (!root || !IsWindow(root)) return false;
  EmbedGpuDetectCtx ctx;
  EnumChildWindows(root, embed_gpu_detect_child, reinterpret_cast<LPARAM>(&ctx));
  if (ctx.detected) {
    std::fprintf(stderr, "[gpu-editor] gpu_editor_detected=true child_count=%d\n", ctx.child_count);
  }
  return ctx.detected;
}

void embed_sync_parent_visibility(EmbedSession* session) {
  if (!session || session->host_kind != EmbedHostKind::OwnedToolWindow) return;
  const HWND parent = session->parent;
  const HWND overlay = session->child;
  if (!parent || !overlay || !IsWindow(parent) || !IsWindow(overlay)) return;
  const bool parent_visible = IsWindowVisible(parent) != FALSE && !IsIconic(parent);
  ShowWindow(overlay, parent_visible ? SW_SHOWNA : SW_HIDE);
}

// Sync the host window's position/size to the requested region. Returns true
// only when the applied rect actually changed (so callers can skip the
// expensive onSize/pump/raise work on idle frames). Repositioning every GPUI
// render pass — even when nothing moved — is what produced the flicker, the
// apparent double-overlay, and the constant CPU/resize-log spam (Part A + D).
bool embed_sync_host_geometry(EmbedSession* session, int x, int y, int w, int h, bool log_reposition) {
  if (!session || !session->child || !IsWindow(session->child)) return false;
  session->host_x = x;
  session->host_y = y;
  session->host_w = w;
  session->host_h = h;

  if (session->host_kind == EmbedHostKind::OwnedToolWindow && session->parent) {
    embed_sync_parent_visibility(session);
    RECT screen{};
    if (!embed_content_screen_rect(session->parent, x, y, w, h, &screen)) {
      return false;
    }
    // Idle frame: parent has not moved and region has not changed. Skip the
    // SetWindowPos / raise / onSize entirely — no flicker, no spam.
    if (session->geometry_valid && EqualRect(&screen, &session->last_applied)) {
      if (log_reposition && embed_debug()) {
        std::fprintf(
            stderr,
            "[plugin-view] skipped reposition (unchanged) overlay=0x%p\n",
            static_cast<void*>(session->child));
      }
      return false;
    }
    session->last_applied = screen;
    session->geometry_valid = true;
    session->reposition_count++;
    const int screen_w = screen.right - screen.left;
    const int screen_h = screen.bottom - screen.top;
    // Place overlay directly above the GPUI PluginView in z-order (not HWND_TOP desktop-wide).
    SetWindowPos(
        session->child,
        session->parent,
        screen.left,
        screen.top,
        screen_w,
        screen_h,
        SWP_NOACTIVATE | SWP_SHOWWINDOW);
    embed_apply_owned_overlay_styles(session->child, session->parent);
    if (log_reposition && embed_debug()) {
      std::fprintf(
          stderr,
          "[plugin-view] reposition #%llu overlay=0x%p owner=0x%p content_screen=(%ld,%ld,%ld,%ld)\n",
          session->reposition_count,
          static_cast<void*>(session->child),
          static_cast<void*>(session->parent),
          screen.left,
          screen.top,
          screen.right,
          screen.bottom);
    }
  } else {
    RECT want{x, y, x + w, y + h};
    if (session->geometry_valid && EqualRect(&want, &session->last_applied)) {
      if (log_reposition && embed_debug()) {
        std::fprintf(
            stderr,
            "[plugin-view] skipped reposition (unchanged) child=0x%p\n",
            static_cast<void*>(session->child));
      }
      return false;
    }
    session->last_applied = want;
    session->geometry_valid = true;
    session->reposition_count++;
    SetWindowPos(session->child, HWND_TOP, x, y, w, h, SWP_SHOWWINDOW | SWP_NOACTIVATE);
  }
  EnableWindow(session->child, TRUE);
  embed_raise_plugin_hwnds(session->child);
  return true;
}

void embed_force_show_child(EmbedSession* session, int x, int y, int w, int h) {
  if (!session || !session->child) return;
  embed_sync_host_geometry(session, x, y, w, h, true);
  embed_post_attach_refresh(session->child, w, h);
  if (session->host_kind == EmbedHostKind::OwnedToolWindow) {
    embed_apply_toolwindow_to_descendants(session->child);
  }
}

int embed_pump_child_messages(HWND child) {
  int pumped = 0;
  MSG msg{};
  auto pump_queue = [&](HWND filter) {
    while (PeekMessageW(&msg, filter, 0, 0, PM_REMOVE)) {
      TranslateMessage(&msg);
      DispatchMessageW(&msg);
      pumped++;
    }
  };
  pump_queue(child);
  EnumChildWindows(
      child,
      [](HWND hwnd, LPARAM lparam) -> BOOL {
        MSG m{};
        auto* count = reinterpret_cast<int*>(lparam);
        while (PeekMessageW(&m, hwnd, 0, 0, PM_REMOVE)) {
          TranslateMessage(&m);
          DispatchMessageW(&m);
          (*count)++;
        }
        return TRUE;
      },
      reinterpret_cast<LPARAM>(&pumped));
  return pumped;
}

void embed_refresh_session(EmbedSession* session, bool audit_log) {
  if (!session || !session->child || !IsWindow(session->child) || !session->vst3 ||
      !session->vst3->attached) {
    return;
  }
  if (session->parent && !IsWindow(session->parent)) {
    return;
  }
  // Only push geometry/onSize/pump when the host actually moved or resized.
  // On idle frames embed_sync_host_geometry returns false and we do nothing —
  // this is the core Part D fix (no per-frame SetWindowPos/onSize/pump).
  const bool changed = embed_sync_host_geometry(
      session, session->host_x, session->host_y, session->host_w, session->host_h, audit_log);
  if (!changed) {
    return;
  }
  embed_resize_view(session, false);
  const int pumped = embed_pump_child_messages(session->child);
  if (audit_log) {
    std::fprintf(stderr, "[vst3-editor-audit] refresh pump drained=%d messages\n", pumped);
  }
}

// Initialize COM on the editor (UI) thread before any IPlugView::attached call.
// Some WebView/CEF-backed VST3 editors need a live STA on the thread that owns
// their parent HWND, otherwise the
// embedded WebView/CEF host never spins up child windows and the editor stays
// blank. Idempotent and safe to call multiple times: if the thread is already
// initialized to a different apartment we log the HRESULT and keep going (the
// host will likely still attach, just without our STA hint).
//
// We deliberately do NOT pair this with `CoUninitialize` — the editor lifetime
// extends past this function and the thread typically lives for the duration
// of the app. Tearing down COM mid-editor would crash WebView hosts.
void embed_ensure_com_initialized() {
  static thread_local HRESULT s_last_hr = S_FALSE;
  const HRESULT hr = CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
  if (hr != s_last_hr) {
    s_last_hr = hr;
    const char* tag = "ok";
    if (hr == S_FALSE) {
      tag = "already initialized (STA)";
    } else if (hr == RPC_E_CHANGED_MODE) {
      tag = "RPC_E_CHANGED_MODE (thread already in MTA)";
    } else if (FAILED(hr)) {
      tag = "FAILED";
    }
    std::fprintf(
        stderr,
        "[vst3-editor] COM init hr=0x%08lx (%s) tid=%lu\n",
        static_cast<unsigned long>(hr),
        tag,
        GetCurrentThreadId());
  }
}

bool embed_has_visible_plugin_ui(HWND child, Steinberg::IPlugView* view) {
  if (!child || !IsWindow(child) || !IsWindowVisible(child)) return false;
  RECT cr{};
  GetClientRect(child, &cr);
  const int cw = cr.right - cr.left;
  const int ch = cr.bottom - cr.top;
  if (cw < 4 || ch < 4) return false;

  struct Ctx {
    int visible_children = 0;
  } ctx{};
  EnumChildWindows(
      child,
      [](HWND hwnd, LPARAM lparam) -> BOOL {
        if (!IsWindowVisible(hwnd)) return TRUE;
        RECT r{};
        GetWindowRect(hwnd, &r);
        if (r.right > r.left && r.bottom > r.top) {
          reinterpret_cast<Ctx*>(lparam)->visible_children++;
        }
        return TRUE;
      },
      reinterpret_cast<LPARAM>(&ctx));
  if (ctx.visible_children > 0) return true;

  if (view) {
    Steinberg::ViewRect sz{};
    const auto gs = view->getSize(&sz);
    if (gs == Steinberg::kResultTrue || gs == Steinberg::kResultOk) {
      const int w = sz.right - sz.left;
      const int h = sz.bottom - sz.top;
      if (w > 16 && h > 16) return true;
    }
  }
  return false;
}

void embed_audit_enum_children(HWND child) {
  int child_count = 0;
  EnumChildWindows(child, embed_enum_child_log, reinterpret_cast<LPARAM>(&child_count));
  std::fprintf(stderr, "[vst3-editor-audit] EnumChildWindows count=%d\n", child_count);
}

void embed_destroy_session(std::unique_ptr<EmbedSession> session, unsigned long long handle) {
  if (!session) return;
  if (embed_debug()) {
    std::fprintf(
        stderr,
        "[vst3-editor] close editor_id=%llu plugin_view_hwnd=0x%p child_hwnd=0x%p\n",
        handle,
        static_cast<void*>(session->parent),
        static_cast<void*>(session->child));
  }
  if (session->vst3 && session->vst3->view && session->vst3->attached) {
    vst3_clear_plug_frame(*session->vst3);
    const auto removed_result = session->vst3->view->removed();
    if (embed_debug() || vst3_editor_debug()) {
      std::fprintf(stderr, "[vst3-editor] removed result=0x%x\n",
                   static_cast<unsigned>(removed_result));
    }
    session->vst3->attached = false;
  } else if (session->vst3) {
    vst3_clear_plug_frame(*session->vst3);
  }
  session->vst3.reset();
  if (session->child && IsWindow(session->child)) {
    DestroyWindow(session->child);
    session->child = nullptr;
  }
  if (embed_debug()) {
    std::fprintf(
        stderr,
        "[plugin-view] detach editor hwnd=0x%p owner hwnd=0x%p handle=%llu\n",
        static_cast<void*>(session->child),
        static_cast<void*>(session->parent),
        handle);
  }
  if (handle != 0) {
    std::fprintf(stderr, "[SpherePluginHost] embed_detach handle=%llu\n", handle);
  }
}
#endif // _WIN32

} // namespace

extern "C" unsigned long long sphere_plugin_editor_open_window(
    const char* window_id,
    const char* title,
    const char* subtitle,
    int width,
    int height) {
  (void)width;
  (void)height;
  const std::string id = window_id && *window_id ? window_id : "plugin-editor";
#ifdef _WIN32
  {
    std::lock_guard<std::mutex> lock(g_windows_mutex);
    const auto existing = g_window_ids.find(id);
    if (existing != g_window_ids.end()) {
      const auto state_it = g_windows.find(existing->second);
      if (state_it != g_windows.end() && state_it->second && IsWindow(state_it->second->hwnd)) {
        state_it->second->close_requested = false;
        ShowWindow(state_it->second->hwnd, SW_RESTORE);
        SetForegroundWindow(state_it->second->hwnd);
        PostMessageW(state_it->second->hwnd, WM_SETFOCUS, 0, 0);
        std::fprintf(
            stderr,
            "[SpherePluginHost] PluginEditor dedupe windowId=%s handle=%llu mainHWND=0x%p childAttachHWND=0x%p\n",
            id.c_str(),
            existing->second,
            static_cast<void*>(state_it->second->hwnd),
            static_cast<void*>(state_it->second->attach_hwnd));
        return existing->second;
      }
    }
  }

  const auto handle = g_next_handle.fetch_add(1);
  auto* config = new EditorWindowConfig();
  config->handle = handle;
  config->window_id = id;
  config->title = utf8_to_wide(title && *title ? title : "Plugin Editor");
  config->subtitle = utf8_to_wide(subtitle && *subtitle ? subtitle : "Native plugin editor window");
  config->width = width > 320 ? width : 820;
  config->height = height > 240 ? height : 560;
  std::thread(run_win32_editor, config).detach();
  {
    std::unique_lock<std::mutex> lock(g_windows_mutex);
    g_windows_cv.wait_for(lock, std::chrono::seconds(2), [handle]() {
      const auto it = g_windows.find(handle);
      return it != g_windows.end() && it->second && it->second->hwnd && it->second->attach_hwnd;
    });
  }
  return handle;
#elif defined(__APPLE__)
  const auto handle = g_next_handle.fetch_add(1);
  std::fprintf(stderr, "[SpherePluginHost] NSWindow plugin editor backend is declared but not linked in this build. title=%s\n", title ? title : "");
  return handle;
#else
  const auto handle = g_next_handle.fetch_add(1);
  std::fprintf(stderr, "[SpherePluginHost] GTK4 plugin editor backend is declared but not linked in this build. title=%s\n", title ? title : "");
  return handle;
#endif
}

extern "C" unsigned long long sphere_plugin_editor_get_attach_handle(unsigned long long handle) {
#ifdef _WIN32
  std::lock_guard<std::mutex> lock(g_windows_mutex);
  const auto it = g_windows.find(handle);
  if (it == g_windows.end() || !it->second || !it->second->attach_hwnd) return 0;
  return static_cast<unsigned long long>(reinterpret_cast<std::uintptr_t>(it->second->attach_hwnd));
#else
  (void)handle;
  return 0;
#endif
}

extern "C" int sphere_plugin_editor_attach_vst3_view(
    unsigned long long handle,
    const char* plugin_path,
    const char* class_id) {
#ifdef _WIN32
  HWND hwnd = nullptr;
  {
    std::lock_guard<std::mutex> lock(g_windows_mutex);
    const auto it = g_windows.find(handle);
    if (it != g_windows.end() && it->second) hwnd = it->second->hwnd;
  }
  if (!hwnd) return 0;
  AttachVst3Request request{plugin_path, class_id, 0};
  SendMessageW(hwnd, WM_ATTACH_VST3_EDITOR, 0, reinterpret_cast<LPARAM>(&request));
  return request.result;
#else
  (void)handle;
  (void)plugin_path;
  (void)class_id;
  return 0;
#endif
}

extern "C" void sphere_plugin_editor_close_window(unsigned long long handle) {
#ifdef _WIN32
  HWND hwnd = nullptr;
  {
    std::lock_guard<std::mutex> lock(g_windows_mutex);
    auto it = g_windows.find(handle);
    if (it != g_windows.end() && it->second) {
      it->second->close_requested = true;
      hwnd = it->second->hwnd;
    }
  }
  if (hwnd) PostMessageW(hwnd, WM_CLOSE, 0, 0);
#else
  (void)handle;
#endif
}

extern "C" void sphere_plugin_editor_focus_window(unsigned long long handle) {
#ifdef _WIN32
  HWND hwnd = nullptr;
  HWND attach = nullptr;
  {
    std::lock_guard<std::mutex> lock(g_windows_mutex);
    auto it = g_windows.find(handle);
    if (it != g_windows.end() && it->second) {
      hwnd = it->second->hwnd;
      attach = it->second->attach_hwnd;
    }
  }
  if (hwnd) {
    ShowWindow(hwnd, SW_RESTORE);
    SetForegroundWindow(hwnd);
    SetFocus(attach ? attach : hwnd);
  }
#else
  (void)handle;
#endif
}

extern "C" void sphere_plugin_editor_resize_window(unsigned long long handle, int width, int height) {
#ifdef _WIN32
  HWND hwnd = nullptr;
  {
    std::lock_guard<std::mutex> lock(g_windows_mutex);
    auto it = g_windows.find(handle);
    if (it != g_windows.end() && it->second) hwnd = it->second->hwnd;
  }
  if (hwnd) {
    const UINT dpi = GetDpiForWindow(hwnd);
    RECT rect{0, 0, MulDiv(width, dpi, 96), MulDiv(height, dpi, 96)};
    AdjustWindowRectExForDpi(&rect, WS_OVERLAPPEDWINDOW, FALSE, WS_EX_APPWINDOW, dpi);
    SetWindowPos(hwnd, nullptr, 0, 0, rect.right - rect.left, rect.bottom - rect.top, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
  }
#else
  (void)handle;
  (void)width;
  (void)height;
#endif
}

extern "C" SpherePluginHostString sphere_plugin_editor_drain_param_events_json() {
#ifdef _WIN32
  std::vector<EditorParamEvent> events;
  {
    std::lock_guard<std::mutex> lock(g_param_events_mutex);
    events.swap(g_param_events);
  }

  std::ostringstream json;
  json << "[";
  for (std::size_t i = 0; i < events.size(); ++i) {
    if (i > 0) json << ",";
    json << "{\"windowId\":\"" << escape_json_local(events[i].window_id)
         << "\",\"paramId\":" << static_cast<unsigned long long>(events[i].id)
         << ",\"value\":" << events[i].value << "}";
  }
  json << "]";
  return make_string_local(json.str());
#else
  return make_string_local("[]");
#endif
}

// Complete an embed attach using a prepared Vst3EditorAttachment (createView
// already done; getSize may have been queried before attach).
unsigned long long embed_complete_attach(
    HWND parent,
    int x,
    int y,
    int width,
    int height,
    std::unique_ptr<Vst3EditorAttachment> attachment,
    int preferred_w,
    int preferred_h,
    bool have_preferred) {
  if (!attachment || !attachment->view) return 0;

  embed_ensure_parent_clip_children(parent);
  embed_audit_log_threads(parent, nullptr);

  const EmbedHostKind host_kind = embed_resolve_host_kind();
  const int region_w = width > 1 ? width : 1;
  const int region_h = height > 1 ? height : 1;

  HWND child = embed_create_host_window(parent, host_kind, x, y, region_w, region_h);
  if (!child) {
    std::fprintf(stderr, "[SpherePluginHost] embed_attach: create host window failed\n");
    return 0;
  }

  embed_audit_log_threads(parent, child);

  vst3_install_plug_frame(*attachment, child, parent);

  const auto attach_result =
      attachment->view->attached(reinterpret_cast<void*>(child), Steinberg::kPlatformTypeHWND);
  std::fprintf(
      stderr,
      "[vst3-editor-audit] attached result=%s(%d)\n",
      embed_vst3_result_name(attach_result),
      (int)attach_result);
  if (attach_result != Steinberg::kResultTrue && attach_result != Steinberg::kResultOk) {
    vst3_clear_plug_frame(*attachment);
    DestroyWindow(child);
    return 0;
  }
  attachment->attached = true;

  auto session = std::make_unique<EmbedSession>();
  session->host_kind = host_kind;
  session->child = child;
  session->parent = parent;
  session->host_x = x;
  session->host_y = y;
  session->host_w = region_w;
  session->host_h = region_h;
  session->preferred_w = preferred_w;
  session->preferred_h = preferred_h;
  session->window_id = attachment->plugin_path;
  session->vst3 = std::move(attachment);

  embed_force_show_child(session.get(), x, y, region_w, region_h);
  embed_resize_view(session.get(), true);
  const int pumped = embed_pump_child_messages(child);
  std::fprintf(stderr, "[vst3-editor-audit] post-attach pump drained=%d messages\n", pumped);

  const bool gpu_detected = embed_detect_gpu_children(child);
  if (gpu_detected) {
    embed_post_attach_refresh(child, region_w, region_h);
  }

  Steinberg::ViewRect after{};
  const auto after_result = session->vst3->view->getSize(&after);
  if (after_result == Steinberg::kResultTrue && !have_preferred) {
    const int after_w = after.right - after.left;
    const int after_h = after.bottom - after.top;
    session->preferred_w = after_w > 1 ? after_w : 1;
    session->preferred_h = after_h > 1 ? after_h : 1;
  }

  embed_audit_log_child_state(child, parent);
  embed_audit_enum_children(child);

  if (!IsWindowVisible(child)) {
    if (session->vst3 && session->vst3->view && session->vst3->attached) {
      vst3_clear_plug_frame(*session->vst3);
      session->vst3->view->removed();
      session->vst3->attached = false;
    } else if (session->vst3) {
      vst3_clear_plug_frame(*session->vst3);
    }
    session->vst3.reset();
    DestroyWindow(child);
    return 0;
  }

  if (host_kind == EmbedHostKind::OwnedToolWindow) {
    embed_apply_toolwindow_to_descendants(child);
  }

  const auto handle = g_next_handle.fetch_add(1);
  {
    std::lock_guard<std::mutex> lock(g_windows_mutex);
    g_embed_sessions[handle] = std::move(session);
  }
  return handle;
}

// ── Embedded editor C ABI (GPUI-hosted) ─────────────────────────────────────

extern "C" unsigned long long sphere_plugin_editor_embed_attach(
    unsigned long long parent_hwnd,
    const char* plugin_path,
    const char* class_id,
    int x,
    int y,
    int width,
    int height) {
#ifdef _WIN32
  // Phase 4: ensure COM (STA) is live on the editor thread before any
  // IPlugView call. CEF/WebView plug-ins rely on this; benign for SDK-only
  // editors (idempotent / no-op when already initialized).
  embed_ensure_com_initialized();

  HWND parent = reinterpret_cast<HWND>(static_cast<std::uintptr_t>(parent_hwnd));
  const std::string path = plugin_path ? plugin_path : "";
  const std::string cid = class_id ? class_id : "";
  const std::string window_id = std::string("embed:") + path;

  if (embed_debug()) {
    std::fprintf(
        stderr,
        "[vst3-editor] attach begin instance=%s parent=0x%p platform=HWND region=(%d,%d,%d,%d)\n",
        window_id.c_str(),
        static_cast<void*>(parent),
        x,
        y,
        width,
        height);
    std::fprintf(stderr, "[vst3-editor] IsWindow(parent)=%d\n", IsWindow(parent) ? 1 : 0);
    if (IsWindow(parent)) {
      const LONG_PTR parent_style = GetWindowLongPtr(parent, GWL_STYLE);
      std::fprintf(
          stderr,
          "[vst3-editor] parent style=0x%08lx GetParent(parent)=0x%p\n",
          static_cast<unsigned long>(parent_style),
          static_cast<void*>(GetParent(parent)));
    }
  }

  // Before attach the parent (GPUI PluginView top-level HWND) must be valid,
  // and the requested region must have a real (>0) size — never attach into a
  // zero-sized or invalid host, never fail silently.
  assert(parent != nullptr);
  assert(IsWindow(parent));
  if (!parent || !IsWindow(parent)) {
    std::fprintf(stderr, "[vst3-editor] attach failed error=invalid parent HWND 0x%p\n",
                 static_cast<void*>(parent));
    return 0;
  }
  if (width <= 0 || height <= 0) {
    std::fprintf(stderr,
                 "[vst3-editor] attach failed error=non-positive host region %dx%d\n",
                 width, height);
    return 0;
  }

  std::string error;
  auto attachment = build_vst3_attachment(path, cid, window_id, error);
  if (!attachment) {
    std::fprintf(stderr, "[vst3-editor] attach failed error=%s\n", error.c_str());
    return 0;
  }
  std::fprintf(
      stderr,
      "[vst3-editor-audit] createView ptr=0x%p platform=HWND\n",
      static_cast<void*>(attachment->view.get()));

  Steinberg::ViewRect preferred{};
  const auto get_size_result = attachment->view->getSize(&preferred);
  const bool have_preferred = get_size_result == Steinberg::kResultTrue;
  const int raw_preferred_w = preferred.right - preferred.left;
  const int raw_preferred_h = preferred.bottom - preferred.top;
  const int preferred_w = have_preferred ? (raw_preferred_w > 1 ? raw_preferred_w : 1) : 0;
  const int preferred_h = have_preferred ? (raw_preferred_h > 1 ? raw_preferred_h : 1) : 0;
  std::fprintf(
      stderr,
      "[plugin-host] getSize result=%d width=%d height=%d\n",
      have_preferred ? 0 : (int)get_size_result,
      preferred_w,
      preferred_h);

  return embed_complete_attach(
      parent, x, y, width, height, std::move(attachment), preferred_w, preferred_h, have_preferred);
#else
  (void)parent_hwnd;
  (void)plugin_path;
  (void)class_id;
  (void)x;
  (void)y;
  (void)width;
  (void)height;
  return 0;
#endif
}

extern "C" void sphere_plugin_editor_embed_set_bounds(
    unsigned long long handle,
    int x,
    int y,
    int width,
    int height) {
#ifdef _WIN32
  const int region_w = width > 1 ? width : 1;
  const int region_h = height > 1 ? height : 1;
  {
    std::lock_guard<std::mutex> lock(g_windows_mutex);
    auto it = g_embed_sessions.find(handle);
    if (it == g_embed_sessions.end() || !it->second || !it->second->child ||
        !IsWindow(it->second->child)) {
      return;
    }
    embed_sync_host_geometry(
        it->second.get(), x, y, region_w, region_h, embed_debug());
    embed_resize_view(it->second.get(), false);
    embed_post_attach_refresh(it->second->child, region_w, region_h);
  }
  if (embed_debug()) {
    std::lock_guard<std::mutex> lock(g_windows_mutex);
    auto it = g_embed_sessions.find(handle);
    const HWND child = (it != g_embed_sessions.end() && it->second) ? it->second->child : nullptr;
    std::fprintf(
        stderr,
        "[vst3-editor] resize editor_id=%llu child_hwnd=0x%p size=(x=%d y=%d w=%d h=%d)\n",
        handle,
        static_cast<void*>(child),
        x,
        y,
        region_w,
        region_h);
  }
#else
  (void)handle;
  (void)x;
  (void)y;
  (void)width;
  (void)height;
#endif
}

extern "C" void sphere_plugin_editor_embed_refresh(unsigned long long handle) {
#ifdef _WIN32
  std::lock_guard<std::mutex> lock(g_windows_mutex);
  auto it = g_embed_sessions.find(handle);
  if (it == g_embed_sessions.end() || !it->second) return;
  embed_refresh_session(it->second.get(), embed_debug());
#else
  (void)handle;
#endif
}

extern "C" void sphere_plugin_editor_embed_detach(unsigned long long handle) {
#ifdef _WIN32
  std::unique_ptr<EmbedSession> session;
  {
    std::lock_guard<std::mutex> lock(g_windows_mutex);
    auto it = g_embed_sessions.find(handle);
    if (it != g_embed_sessions.end()) {
      session = std::move(it->second);
      g_embed_sessions.erase(it);
    }
  }
  embed_destroy_session(std::move(session), handle);
#else
  (void)handle;
#endif
}

extern "C" void sphere_plugin_editor_embed_detach_all() {
#ifdef _WIN32
  std::vector<std::unique_ptr<EmbedSession>> pending;
  {
    std::lock_guard<std::mutex> lock(g_windows_mutex);
    pending.reserve(g_embed_sessions.size());
    for (auto& entry : g_embed_sessions) {
      pending.push_back(std::move(entry.second));
    }
    g_embed_sessions.clear();
  }
  for (auto& session : pending) {
    embed_destroy_session(std::move(session), 0);
  }
  if (!pending.empty()) {
    std::fprintf(
        stderr,
        "[SpherePluginHost] embed_detach_all count=%zu\n",
        pending.size());
  }
#else
#endif
}

extern "C" int sphere_plugin_editor_embed_is_valid(unsigned long long handle) {
#ifdef _WIN32
  std::lock_guard<std::mutex> lock(g_windows_mutex);
  auto it = g_embed_sessions.find(handle);
  return (it != g_embed_sessions.end() && it->second && it->second->child &&
          IsWindow(it->second->child))
             ? 1
             : 0;
#else
  (void)handle;
  return 0;
#endif
}

extern "C" int sphere_plugin_editor_embed_host_kind(unsigned long long handle) {
#ifdef _WIN32
  std::lock_guard<std::mutex> lock(g_windows_mutex);
  auto it = g_embed_sessions.find(handle);
  if (it == g_embed_sessions.end() || !it->second) return -1;
  return it->second->host_kind == EmbedHostKind::OwnedToolWindow ? 1 : 0;
#else
  (void)handle;
  return -1;
#endif
}

extern "C" int sphere_plugin_editor_embed_has_visible_ui(unsigned long long handle) {
#ifdef _WIN32
  std::lock_guard<std::mutex> lock(g_windows_mutex);
  auto it = g_embed_sessions.find(handle);
  if (it == g_embed_sessions.end() || !it->second || !it->second->child ||
      !IsWindow(it->second->child)) {
    return 0;
  }
  Steinberg::IPlugView* view =
      it->second->vst3 ? it->second->vst3->view.get() : nullptr;
  return embed_has_visible_plugin_ui(it->second->child, view) ? 1 : 0;
#else
  (void)handle;
  return 0;
#endif
}

extern "C" int sphere_plugin_editor_embed_preferred_size(
    unsigned long long handle,
    int* out_width,
    int* out_height) {
#ifdef _WIN32
  std::lock_guard<std::mutex> lock(g_windows_mutex);
  auto it = g_embed_sessions.find(handle);
  if (it == g_embed_sessions.end() || !it->second || it->second->preferred_w <= 0 ||
      it->second->preferred_h <= 0) {
    return 0;
  }
  if (out_width) *out_width = it->second->preferred_w;
  if (out_height) *out_height = it->second->preferred_h;
  return 1;
#else
  (void)handle;
  (void)out_width;
  (void)out_height;
  return 0;
#endif
}

extern "C" unsigned long long sphere_plugin_editor_embed_prepare(
    const char* plugin_path,
    const char* class_id,
    int* out_width,
    int* out_height) {
#ifdef _WIN32
  embed_ensure_com_initialized();
  const std::string path = plugin_path ? plugin_path : "";
  const std::string cid = class_id ? class_id : "";
  const std::string window_id = std::string("embed:") + path;
  std::string error;
  auto attachment = build_vst3_attachment(path, cid, window_id, error);
  if (!attachment) {
    std::fprintf(stderr, "[vst3-editor] prepare failed error=%s\n", error.c_str());
    return 0;
  }
  Steinberg::ViewRect preferred{};
  const auto get_size_result = attachment->view->getSize(&preferred);
  const bool have_preferred = get_size_result == Steinberg::kResultTrue;
  const int raw_preferred_w = preferred.right - preferred.left;
  const int raw_preferred_h = preferred.bottom - preferred.top;
  const int preferred_w = have_preferred ? (raw_preferred_w > 1 ? raw_preferred_w : 1) : 0;
  const int preferred_h = have_preferred ? (raw_preferred_h > 1 ? raw_preferred_h : 1) : 0;
  std::fprintf(
      stderr,
      "[plugin-host] prepare getSize result=%d width=%d height=%d\n",
      have_preferred ? 0 : (int)get_size_result,
      preferred_w,
      preferred_h);
  if (out_width) *out_width = preferred_w;
  if (out_height) *out_height = preferred_h;

  auto prepare = std::make_unique<PrepareSession>();
  prepare->vst3 = std::move(attachment);
  prepare->preferred_w = preferred_w;
  prepare->preferred_h = preferred_h;
  prepare->have_preferred = have_preferred;
  prepare->window_id = window_id;
  const auto prepare_id = g_next_prepare_handle.fetch_add(1);
  {
    std::lock_guard<std::mutex> lock(g_windows_mutex);
    g_prepare_sessions[prepare_id] = std::move(prepare);
  }
  return prepare_id;
#else
  (void)plugin_path;
  (void)class_id;
  (void)out_width;
  (void)out_height;
  return 0;
#endif
}

extern "C" void sphere_plugin_editor_embed_cancel_prepare(unsigned long long prepare_id) {
#ifdef _WIN32
  std::lock_guard<std::mutex> lock(g_windows_mutex);
  g_prepare_sessions.erase(prepare_id);
#else
  (void)prepare_id;
#endif
}

extern "C" unsigned long long sphere_plugin_editor_embed_attach_prepared(
    unsigned long long prepare_id,
    unsigned long long parent_hwnd,
    int x,
    int y,
    int width,
    int height) {
#ifdef _WIN32
  HWND parent = reinterpret_cast<HWND>(static_cast<std::uintptr_t>(parent_hwnd));
  if (!parent || !IsWindow(parent) || width <= 0 || height <= 0) return 0;

  std::unique_ptr<PrepareSession> prepare;
  {
    std::lock_guard<std::mutex> lock(g_windows_mutex);
    auto it = g_prepare_sessions.find(prepare_id);
    if (it == g_prepare_sessions.end()) return 0;
    prepare = std::move(it->second);
    g_prepare_sessions.erase(it);
  }
  if (!prepare || !prepare->vst3) return 0;

  return embed_complete_attach(
      parent,
      x,
      y,
      width,
      height,
      std::move(prepare->vst3),
      prepare->preferred_w,
      prepare->preferred_h,
      prepare->have_preferred);
#else
  (void)prepare_id;
  (void)parent_hwnd;
  (void)x;
  (void)y;
  (void)width;
  (void)height;
  return 0;
#endif
}

extern "C" unsigned long long sphere_plugin_editor_embed_host_hwnd(unsigned long long handle) {
#ifdef _WIN32
  std::lock_guard<std::mutex> lock(g_windows_mutex);
  auto it = g_embed_sessions.find(handle);
  if (it == g_embed_sessions.end() || !it->second || !it->second->child) return 0;
  return static_cast<unsigned long long>(reinterpret_cast<std::uintptr_t>(it->second->child));
#else
  (void)handle;
  return 0;
#endif
}

extern "C" void sphere_plugin_editor_embed_delayed_gpu_refresh(unsigned long long handle) {
#ifdef _WIN32
  std::lock_guard<std::mutex> lock(g_windows_mutex);
  auto it = g_embed_sessions.find(handle);
  if (it == g_embed_sessions.end() || !it->second || !it->second->child ||
      !IsWindow(it->second->child)) {
    return;
  }
  const int w = it->second->host_w > 0 ? it->second->host_w : 1;
  const int h = it->second->host_h > 0 ? it->second->host_h : 1;
  embed_resize_view(it->second.get(), true);
  embed_post_attach_refresh(it->second->child, w, h);
  std::fprintf(stderr, "[gpu-editor] delayed_redraw_100ms\n");
#else
  (void)handle;
#endif
}
