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
};

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
    std::fprintf(stderr, "[SpherePluginHost] VST3 editor removed handle=%llu\n", state->handle);
    vst3.view->removed();
    vst3.attached = false;
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

  const auto attach_result = attachment->view->attached(reinterpret_cast<void*>(state->attach_hwnd), Steinberg::kPlatformTypeHWND);
  if (attach_result != Steinberg::kResultTrue) {
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

  attachment->view = Steinberg::IPtr<Steinberg::IPlugView>::adopt(
      attachment->controller->createView(Steinberg::Vst::ViewType::kEditor));
  if (!attachment->view) {
    error = "controller did not create editor view";
    return nullptr;
  }
  if (attachment->view->isPlatformTypeSupported(Steinberg::kPlatformTypeHWND) != Steinberg::kResultTrue) {
    error = "editor view does not support HWND platform type";
    return nullptr;
  }
  return attachment;
}

struct EmbedSession {
  HWND child = nullptr;   // WS_CHILD host region parented to the GPUI window
  HWND parent = nullptr;  // GPUI PluginView top-level HWND (the WS_CHILD parent)
  std::unique_ptr<Vst3EditorAttachment> vst3;
  std::string window_id;
};

std::unordered_map<unsigned long long, std::unique_ptr<EmbedSession>> g_embed_sessions; // guarded by g_windows_mutex

bool embed_debug() {
  static const bool enabled = std::getenv("FUTUREBOARD_PLUGIN_VIEW_DEBUG") != nullptr;
  return enabled;
}

LRESULT CALLBACK embed_child_wndproc(HWND hwnd, UINT msg, WPARAM wparam, LPARAM lparam) {
  // The plugin parents its own view window inside this child; we just host it.
  // Paint a solid black backing so there is no flash before the plugin draws,
  // and so anything outside the plugin's own view stays inside our bounds.
  if (msg == WM_ERASEBKGND) {
    HDC hdc = reinterpret_cast<HDC>(wparam);
    RECT rc{};
    GetClientRect(hwnd, &rc);
    FillRect(hdc, &rc, reinterpret_cast<HBRUSH>(GetStockObject(BLACK_BRUSH)));
    return 1;
  }
  return DefWindowProcW(hwnd, msg, wparam, lparam);
}

const wchar_t* kEmbedChildClass = L"SpherePluginEmbedHost";

void ensure_embed_child_class() {
  static std::once_flag once;
  std::call_once(once, []() {
    WNDCLASSEXW wc{};
    wc.cbSize = sizeof(wc);
    wc.lpfnWndProc = embed_child_wndproc;
    wc.hInstance = GetModuleHandleW(nullptr);
    wc.hCursor = LoadCursorW(nullptr, reinterpret_cast<LPCWSTR>(IDC_ARROW));
    wc.hbrBackground = reinterpret_cast<HBRUSH>(GetStockObject(BLACK_BRUSH));
    wc.lpszClassName = kEmbedChildClass;
    RegisterClassExW(&wc);
  });
}

// Push the host content rect into the plugin view. The child window is sized to
// the host region; the plugin view fills the child (origin 0,0). Always called
// on the thread owning the parent HWND, never the audio thread. Returns the
// IPlugView::onSize tresult (or kResultFalse when there is nothing to size).
Steinberg::tresult embed_resize_view(EmbedSession* session) {
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
  if (embed_debug()) {
    std::fprintf(
        stderr,
        "[vst3-editor] onSize result=%d size=%dx%d\n",
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

// Log the full Win32 state of the host child window after attach (Phase 1).
void embed_log_child_state(HWND child, HWND parent) {
  if (!embed_debug()) return;
  RECT wr{};
  RECT cr{};
  GetWindowRect(child, &wr);
  GetClientRect(child, &cr);
  const LONG_PTR style = GetWindowLongPtr(child, GWL_STYLE);
  const LONG_PTR exstyle = GetWindowLongPtr(child, GWL_EXSTYLE);
  std::fprintf(stderr, "[plugin-view] attach ok child_hwnd=0x%p\n", static_cast<void*>(child));
  std::fprintf(
      stderr,
      "[plugin-view] child visible=%d IsWindow=%d parent=0x%p (expect 0x%p) parent_match=%d\n",
      IsWindowVisible(child) ? 1 : 0,
      IsWindow(child) ? 1 : 0,
      static_cast<void*>(GetParent(child)),
      static_cast<void*>(parent),
      GetParent(child) == parent ? 1 : 0);
  std::fprintf(
      stderr,
      "[plugin-view] child rect window=(%ld,%ld,%ld,%ld) client=(%ld,%ld,%ld,%ld)\n",
      wr.left, wr.top, wr.right, wr.bottom,
      cr.left, cr.top, cr.right, cr.bottom);
  std::fprintf(
      stderr,
      "[plugin-view] child style=0x%08lx (WS_CHILD=%d WS_VISIBLE=%d WS_CLIPSIBLINGS=%d "
      "WS_CLIPCHILDREN=%d WS_POPUP=%d) exstyle=0x%08lx\n",
      static_cast<unsigned long>(style),
      (style & WS_CHILD) ? 1 : 0,
      (style & WS_VISIBLE) ? 1 : 0,
      (style & WS_CLIPSIBLINGS) ? 1 : 0,
      (style & WS_CLIPCHILDREN) ? 1 : 0,
      (style & WS_POPUP) ? 1 : 0,
      static_cast<unsigned long>(exstyle));
}
#endif // _WIN32

} // namespace

extern "C" unsigned long long sphere_plugin_editor_open_window(
    const char* window_id,
    const char* title,
    const char* subtitle,
    int width,
    int height) {
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
  ensure_embed_child_class();

  const int region_w = width > 1 ? width : 1;
  const int region_h = height > 1 ? height : 1;

  // Create a dedicated WS_CHILD host region INSIDE the GPUI PluginView window.
  // The plugin's IPlugView is attached into this child so it moves, clips and
  // resizes with the GPUI window and never floats as an independent top-level
  // window. Coordinates are relative to the parent's client area (physical px).
  //   parent  = GPUI PluginView top-level HWND
  //   style   = WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | WS_CLIPCHILDREN
  //   NOT WS_POPUP (would detach it into its own top-level window)
  HWND child = CreateWindowExW(
      0,
      kEmbedChildClass,
      L"",
      WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
      x,
      y,
      region_w,
      region_h,
      parent, // real parent — child rides the GPUI window's lifetime / z-order
      nullptr,
      GetModuleHandleW(nullptr),
      nullptr);
  if (!child) {
    std::fprintf(stderr, "[SpherePluginHost] embed_attach: CreateWindowEx WS_CHILD failed\n");
    return 0;
  }

  // Validate the child really is a non-popup child of the GPUI window.
  const LONG_PTR child_style = GetWindowLongPtr(child, GWL_STYLE);
  assert(child_style & WS_CHILD);
  assert(!(child_style & WS_POPUP));
  assert(GetParent(child) == parent);

  if (embed_debug()) {
    std::fprintf(
        stderr,
        "[vst3-editor] child hwnd=0x%p parent=0x%p style=0x%08lx WS_CHILD=%d WS_POPUP=%d "
        "GetParent(child)=0x%p IsWindow(child)=%d region=(x=%d y=%d w=%d h=%d)\n",
        static_cast<void*>(child),
        static_cast<void*>(parent),
        static_cast<unsigned long>(child_style),
        (child_style & WS_CHILD) ? 1 : 0,
        (child_style & WS_POPUP) ? 1 : 0,
        static_cast<void*>(GetParent(child)),
        IsWindow(child) ? 1 : 0,
        x,
        y,
        region_w,
        region_h);
  }

  std::string error;
  auto attachment = build_vst3_attachment(path, cid, window_id, error);
  if (!attachment) {
    std::fprintf(stderr, "[vst3-editor] attach failed error=%s\n", error.c_str());
    DestroyWindow(child);
    return 0;
  }
  if (embed_debug()) {
    std::fprintf(stderr, "[vst3-editor] view ptr=0x%p\n",
                 static_cast<void*>(attachment->view.get()));
  }

  // Report the plugin's preferred size for logging only; the host region size
  // is authoritative so the editor clips/resizes with the GPUI window.
  Steinberg::ViewRect preferred{};
  const auto get_size_result = attachment->view->getSize(&preferred);
  const bool have_preferred = get_size_result == Steinberg::kResultTrue;
  if (embed_debug()) {
    std::fprintf(
        stderr,
        "[vst3-editor] getSize result=%d preferred=(%d,%d,%d,%d)\n",
        (int)get_size_result,
        preferred.left,
        preferred.top,
        preferred.right,
        preferred.bottom);
  }

  // Attach the IPlugView to the CHILD HWND (never the parent / never a popup).
  const auto attach_result =
      attachment->view->attached(reinterpret_cast<void*>(child), Steinberg::kPlatformTypeHWND);
  if (embed_debug()) {
    std::fprintf(stderr, "[vst3-editor] attached result=%d\n", (int)attach_result);
  }
  if (attach_result != Steinberg::kResultTrue) {
    std::fprintf(stderr, "[vst3-editor] attach failed error=IPlugView::attached(HWND) returned %d\n",
                 (int)attach_result);
    DestroyWindow(child);
    return 0;
  }
  attachment->attached = true;

  auto session = std::make_unique<EmbedSession>();
  session->child = child;
  session->parent = parent;
  session->window_id = window_id;
  session->vst3 = std::move(attachment);

  // Phase 2 — force the child fully shown/sized after attach, then repaint.
  // SetWindowPos child coords are relative to the GPUI window client area; the
  // IPlugView onSize below receives the editor client rect starting at (0,0).
  SetWindowPos(child, HWND_TOP, x, y, region_w, region_h,
               SWP_SHOWWINDOW | SWP_NOACTIVATE);
  ShowWindow(child, SW_SHOW);
  EnableWindow(child, TRUE);
  embed_resize_view(session.get());
  InvalidateRect(child, nullptr, TRUE);
  UpdateWindow(child);

  // Phase 3 — log the plugin's reported size *after* attach (some editors only
  // report a meaningful size once attached).
  if (embed_debug()) {
    Steinberg::ViewRect after{};
    const auto after_result = session->vst3->view->getSize(&after);
    std::fprintf(
        stderr,
        "[vst3-editor] getSize after attach result=%d size=(%d,%d,%d,%d)\n",
        (int)after_result,
        after.left,
        after.top,
        after.right,
        after.bottom);
  }

  // Phase 1 — full Win32 verification of the host child window.
  embed_log_child_state(child, parent);

  // Phase 5 — enumerate windows the plugin parented under our child.
  if (embed_debug()) {
    int child_count = 0;
    EnumChildWindows(child, embed_enum_child_log, reinterpret_cast<LPARAM>(&child_count));
    std::fprintf(stderr, "[vst3-editor] child windows count=%d\n", child_count);
    std::fprintf(stderr, "[plugin-view] show/update child done\n");
  }

  // Phase 9 — never report ok if the child window is not actually visible.
  if (!IsWindowVisible(child)) {
    std::fprintf(
        stderr,
        "[vst3-editor] attach failed error=child HWND not visible after show (0x%p)\n",
        static_cast<void*>(child));
    if (session->vst3 && session->vst3->view && session->vst3->attached) {
      session->vst3->view->removed();
      session->vst3->attached = false;
    }
    session->vst3.reset();
    DestroyWindow(child);
    return 0;
  }

  const auto handle = g_next_handle.fetch_add(1);
  {
    std::lock_guard<std::mutex> lock(g_windows_mutex);
    g_embed_sessions[handle] = std::move(session);
  }
  std::fprintf(
      stderr,
      "[SpherePluginHost] embed_attach ok handle=%llu plugin_view_hwnd=0x%p child_hwnd=0x%p "
      "region=(%d,%d,%d,%d) path=%s\n",
      handle,
      static_cast<void*>(parent),
      static_cast<void*>(child),
      x,
      y,
      region_w,
      region_h,
      path.c_str());
  (void)have_preferred;
  return handle;
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
  HWND child = nullptr;
  {
    std::lock_guard<std::mutex> lock(g_windows_mutex);
    auto it = g_embed_sessions.find(handle);
    if (it == g_embed_sessions.end() || !it->second) return;
    child = it->second->child;
  }
  if (!child || !IsWindow(child)) return;

  // The child is a WS_CHILD of the GPUI window, so its coordinates are already
  // relative to the parent's client area (physical px) — NO screen conversion.
  // Reposition AND resize so the editor moves/clips/resizes with the GPUI host
  // region, then push the new content rect into the plugin via onSize.
  const int region_w = width > 1 ? width : 1;
  const int region_h = height > 1 ? height : 1;
  SetWindowPos(child, nullptr, x, y, region_w, region_h,
               SWP_NOZORDER | SWP_NOACTIVATE);
  {
    std::lock_guard<std::mutex> lock(g_windows_mutex);
    auto it = g_embed_sessions.find(handle);
    if (it != g_embed_sessions.end() && it->second) embed_resize_view(it->second.get());
  }
  if (embed_debug()) {
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
  if (!session) return;
  // Detach the IPlugView from the child before destroying it, then release the
  // attachment (which terminates the controller/component) and the child window.
  if (embed_debug()) {
    std::fprintf(
        stderr,
        "[vst3-editor] close editor_id=%llu plugin_view_hwnd=0x%p child_hwnd=0x%p\n",
        handle,
        static_cast<void*>(session->parent),
        static_cast<void*>(session->child));
  }
  if (session->vst3 && session->vst3->view && session->vst3->attached) {
    const auto removed_result = session->vst3->view->removed();
    if (embed_debug()) {
      std::fprintf(stderr, "[vst3-editor] removed result=%d\n", (int)removed_result);
    }
    session->vst3->attached = false;
  }
  session->vst3.reset();
  if (session->child && IsWindow(session->child)) {
    DestroyWindow(session->child);
    session->child = nullptr;
  }
  std::fprintf(stderr, "[SpherePluginHost] embed_detach handle=%llu\n", handle);
#else
  (void)handle;
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
