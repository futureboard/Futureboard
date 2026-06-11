#include "sphere_daux_vst3_processor.h"

#include <algorithm>
#include <array>
#include <atomic>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <chrono>
#include <memory>
#include <mutex>
#include <sstream>
#include <string>
#include <thread>
#include <vector>

// IPlugView is needed on all platforms for the editor bridge functions.
#include "pluginterfaces/gui/iplugview.h"

#ifdef _WIN32
#  define WIN32_LEAN_AND_MEAN
#  define NOMINMAX
#  include <windows.h>
#  include <libloaderapi.h>
#  include <objbase.h>
#  include <dwmapi.h>
#  pragma comment(lib, "dwmapi.lib")
#  pragma comment(lib, "ole32.lib")
#endif

#include "pluginterfaces/base/ipluginbase.h"
#include "pluginterfaces/vst/ivstaudioprocessor.h"
#include "pluginterfaces/vst/ivstcomponent.h"
#include "pluginterfaces/vst/ivsteditcontroller.h"
#include "pluginterfaces/vst/ivstevents.h"
#include "pluginterfaces/vst/ivstmidicontrollers.h"
#include "pluginterfaces/vst/ivstparameterchanges.h"
#include "pluginterfaces/vst/ivstprocesscontext.h"
#include "public.sdk/source/common/memorystream.h"
#include "public.sdk/source/vst/hosting/hostclasses.h"
#include "public.sdk/source/vst/hosting/module.h"
#include "public.sdk/source/vst/utility/uid.h"
#include "sphere_daux_editor_bridge.h"

// IPlugFrame is a GUI-layer interface whose class IID is not emitted by the
// SDK IID TUs we compile (coreiids.cpp / vstinitiids.cpp). Our IPlugFrame
// implementation's queryInterface references IPlugFrame::iid, so define the
// symbol here. The IPlugFrame_iid constant is provided by iplugview.h.
namespace Steinberg {
DEF_CLASS_IID(IPlugFrame)
}  // namespace Steinberg

namespace {

constexpr const char* kVst3AudioModuleClass = "Audio Module Class";
thread_local std::string g_last_error;
std::atomic<unsigned long long> g_next_editor_handle{1};

void set_last_error(std::string value) {
  g_last_error = std::move(value);
}

#ifdef _WIN32
constexpr const wchar_t* kDauxEditorWindowClass = L"FutureboardDauxVst3EditorWindow";
constexpr const wchar_t* kDauxEditorChildClass = L"FutureboardDauxVst3EditorAttach";
constexpr const wchar_t* kDauxEditorContentClass = L"FutureboardDauxVst3EditorContent";
// Detached top-level editor host (kind==2). Modeled on the VST3 SDK editorhost
// sample (public.sdk/samples/vst-hosting/editorhost): no background brush, host
// never paints its client area, WM_SIZE forwards onSize, WM_CLOSE asks the shell
// to tear down. A normal, independent OS window — no GPUI compositor over it.
constexpr const wchar_t* kDauxEditorDetachedClass = L"FutureboardDauxVst3EditorDetached";
constexpr COLORREF kDauxTitlebarDark = RGB(14, 19, 25);
constexpr int kDauxFallbackReloadId = 4101;
constexpr int kDauxFallbackGenericId = 4102;
constexpr int kDauxFallbackCloseId = 4103;

// Posted to the HWND's own thread when destroy is requested cross-thread.
constexpr UINT WM_DAUX_DESTROY = WM_APP + 50;

std::wstring widen_utf8(const char* value) {
  if (!value || !*value) return L"Plugin Editor";
  const int needed = MultiByteToWideChar(CP_UTF8, 0, value, -1, nullptr, 0);
  if (needed <= 0) return L"Plugin Editor";
  std::wstring out(static_cast<size_t>(needed), L'\0');
  MultiByteToWideChar(CP_UTF8, 0, value, -1, out.data(), needed);
  if (!out.empty() && out.back() == L'\0') out.pop_back();
  return out;
}

void set_daux_dark_titlebar(HWND hwnd) {
  BOOL dark = TRUE;
  DwmSetWindowAttribute(hwnd, 20, &dark, sizeof(dark)); // DWMWA_USE_IMMERSIVE_DARK_MODE pre-20H1
  DwmSetWindowAttribute(hwnd, 19, &dark, sizeof(dark)); // DWMWA_USE_IMMERSIVE_DARK_MODE_BEFORE_20H1
  DwmSetWindowAttribute(hwnd, DWMWA_CAPTION_COLOR, &kDauxTitlebarDark, sizeof(kDauxTitlebarDark));
}

void paint_dark_child(HWND hwnd) {
  HDC dc = GetDC(hwnd);
  if (!dc) return;
  RECT rc{};
  GetClientRect(hwnd, &rc);
  HBRUSH brush = CreateSolidBrush(RGB(11, 15, 20));
  FillRect(dc, &rc, brush);
  DeleteObject(brush);
  ReleaseDC(hwnd, dc);
}

bool daux_plugin_view_message_debug() {
  // FUTUREBOARD_PLUGIN_DEBUG=1 is the single end-to-end plugin debug switch;
  // the narrower flags remain for targeted tracing.
  static const bool enabled =
      std::getenv("FUTUREBOARD_PLUGIN_VIEW_DEBUG") != nullptr ||
      std::getenv("FUTUREBOARD_VST3_EDITOR_DEBUG") != nullptr ||
      std::getenv("FUTUREBOARD_PLUGIN_DEBUG") != nullptr;
  return enabled;
}

void daux_log_window_message(const char* tag, HWND hwnd, UINT msg) {
  if (!daux_plugin_view_message_debug()) return;
  const char* name = nullptr;
  switch (msg) {
    case WM_CREATE: name = "WM_CREATE"; break;
    case WM_SHOWWINDOW: name = "WM_SHOWWINDOW"; break;
    case WM_SIZE: name = "WM_SIZE"; break;
    case WM_CLOSE: name = "WM_CLOSE"; break;
    case WM_DESTROY: name = "WM_DESTROY"; break;
    case WM_DPICHANGED: name = "WM_DPICHANGED"; break;
    case WM_PAINT: name = "WM_PAINT"; break;
    case WM_ERASEBKGND: name = "WM_ERASEBKGND"; break;
    case WM_TIMER: name = "WM_TIMER"; break;
    default: break;
  }
  if (name) {
    std::fprintf(stderr, "[%s] %s hwnd=0x%p tid=%lu\n",
                 tag, name, static_cast<void*>(hwnd), GetCurrentThreadId());
  }
}

void daux_log_hwnd_state(const char* label, HWND top, HWND content) {
  const auto log_one = [label](const char* name, HWND hwnd) {
    if (!hwnd) {
      std::fprintf(stderr, "[plugin-view] hwnd_state label=%s %s=null\n", label, name);
      return;
    }
    RECT client{};
    RECT screen{};
    GetClientRect(hwnd, &client);
    GetWindowRect(hwnd, &screen);
    const LONG_PTR style = GetWindowLongPtrW(hwnd, GWL_STYLE);
    const LONG_PTR ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
    std::fprintf(
        stderr,
        "[plugin-view] hwnd_state label=%s %s=0x%p parent=0x%p style=0x%Ix ex_style=0x%Ix "
        "client=(%ld,%ld,%ld,%ld) screen=(%ld,%ld,%ld,%ld) visible=%d iconic=%d\n",
        label,
        name,
        static_cast<void*>(hwnd),
        static_cast<void*>(GetParent(hwnd)),
        static_cast<std::uintptr_t>(style),
        static_cast<std::uintptr_t>(ex_style),
        client.left,
        client.top,
        client.right,
        client.bottom,
        screen.left,
        screen.top,
        screen.right,
        screen.bottom,
        IsWindowVisible(hwnd) ? 1 : 0,
        IsIconic(hwnd) ? 1 : 0);
  };
  std::fprintf(stderr,
               "[plugin-view] hwnd_hierarchy label=%s top_hwnd=0x%p content_hwnd=0x%p "
               "content_hwnd_ne_top=%s content_parent=0x%p\n",
               label,
               static_cast<void*>(top),
               static_cast<void*>(content),
               (top && content && top != content) ? "true" : "false",
               static_cast<void*>(content ? GetParent(content) : nullptr));
  log_one("top", top);
  log_one("content", content);
}

// One-shot host wake timers installed at attach time (see embed_editor). Only
// these IDs may be killed in the wndprocs below: plugins commonly subclass the
// attach HWND and drive their repaint/meter/modal logic from their own
// WM_TIMER ticks — killing arbitrary timers freezes such editors after the
// first frame.
constexpr UINT_PTR kDauxWakeTimerTop = 0xDA01;
constexpr UINT_PTR kDauxWakeTimerContent = 0xDA02;

LRESULT CALLBACK daux_editor_content_wnd_proc(HWND hwnd, UINT msg, WPARAM wparam, LPARAM lparam) {
  daux_log_window_message("plugin-content-hwnd", hwnd, msg);
  switch (msg) {
    case WM_TIMER:
      if (wparam == kDauxWakeTimerTop || wparam == kDauxWakeTimerContent) {
        KillTimer(hwnd, wparam);
        return 0;
      }
      break; // plugin-installed timer — let DefWindowProc / subclass chain run
    case WM_ERASEBKGND:
      return 1; // do not repaint over GPU/WebView/OpenGL plug-in output
    case WM_PAINT: {
      PAINTSTRUCT ps{};
      BeginPaint(hwnd, &ps);
      EndPaint(hwnd, &ps);
      return 0;
    }
    case WM_MOUSEACTIVATE:
      // Plugin content clicks must activate without being eaten — the wrapper
      // (cross-process shell) handles the titlebar only.
      if (daux_plugin_view_message_debug()) {
        std::fprintf(stderr,
                     "[PluginEditorInput] mouse_activate result=MA_ACTIVATE hwnd=0x%p\n",
                     static_cast<void*>(hwnd));
      }
      return MA_ACTIVATE;
    case WM_LBUTTONDOWN: {
      // Generic rule: clicking plugin content focuses the plugin child under
      // the point so keyboard input follows the mouse (no vendor logic).
      const POINT pt{static_cast<short>(LOWORD(lparam)), static_cast<short>(HIWORD(lparam))};
      HWND target = ChildWindowFromPointEx(
          hwnd, pt, CWP_SKIPINVISIBLE | CWP_SKIPDISABLED | CWP_SKIPTRANSPARENT);
      if (!target) target = hwnd;
      const HWND focus_before = GetFocus();
      SetFocus(target);
      if (daux_plugin_view_message_debug()) {
        std::fprintf(stderr,
                     "[PluginEditorInput] hit_test area=content point=(%ld,%ld) "
                     "focus_before=0x%p focus_after=0x%p\n",
                     pt.x, pt.y,
                     static_cast<void*>(focus_before),
                     static_cast<void*>(GetFocus()));
      }
      break; // fall through to DefWindowProc so the click still routes
    }
    default:
      break;
  }
  return DefWindowProcW(hwnd, msg, wparam, lparam);
}

void register_editor_window_classes() {
  static std::once_flag once;
  std::call_once(once, [] {
    WNDCLASSEXW wc{};
    wc.cbSize = sizeof(WNDCLASSEXW);
    wc.lpfnWndProc = DefWindowProcW;
    wc.hInstance = GetModuleHandleW(nullptr);
    wc.hCursor = LoadCursorW(nullptr, MAKEINTRESOURCEW(32512));
    // Use black brush — prevents the white flash that occurs between the child
    // HWND being created and the IPlugView rendering its first frame.
    wc.hbrBackground = reinterpret_cast<HBRUSH>(GetStockObject(BLACK_BRUSH));
    wc.lpszClassName = kDauxEditorChildClass;
    RegisterClassExW(&wc);

    WNDCLASSEXW content{};
    content.cbSize = sizeof(WNDCLASSEXW);
    content.lpfnWndProc = daux_editor_content_wnd_proc;
    content.hInstance = GetModuleHandleW(nullptr);
    content.hCursor = LoadCursorW(nullptr, MAKEINTRESOURCEW(32512));
    content.hbrBackground = nullptr;
    content.lpszClassName = kDauxEditorContentClass;
    RegisterClassExW(&content);
  });
}
#endif

bool looks_like_zero_class_id(const std::string& value) {
  if (value.empty()) return true;
  for (char c : value) {
    if (c != '0' && c != '-' && c != '{' && c != '}') return false;
  }
  return true;
}

VST3::Optional<VST3::UID> first_audio_module_uid(const VST3::Hosting::PluginFactory& factory) {
  for (const auto& info : factory.classInfos()) {
    if (info.category() != kVst3AudioModuleClass) continue;
    return VST3::Optional<VST3::UID>(info.ID());
  }
  return {};
}

void log_factory_classes(const VST3::Hosting::PluginFactory& factory) {
  int index = 0;
  for (const auto& info : factory.classInfos()) {
    std::fprintf(stderr,
                 "[DAUx VST3] factory class[%d] name='%s' category='%s' uid=%s\n",
                 index++,
                 info.name().c_str(),
                 info.category().c_str(),
                 info.ID().toString().c_str());
  }
}

} // namespace

// ── Parameter helper types (stack/member allocated — zero heap use) ──────────

/// One pending parameter change (id + normalized value 0..1).
struct PendingParam {
  Steinberg::Vst::ParamID    id{0};
  Steinberg::Vst::ParamValue value{0.0};
};

/// Minimal IParamValueQueue: single value at sample-offset 0.
/// No heap allocation — lives inside SimpleParamChanges::queues[].
struct SimpleParamValueQueue final : Steinberg::Vst::IParamValueQueue {
  Steinberg::Vst::ParamID    param_id{0};
  Steinberg::Vst::ParamValue param_value{0.0};

  Steinberg::tresult PLUGIN_API queryInterface(
      const Steinberg::TUID iid, void** obj) override {
    if (std::memcmp(iid, Steinberg::Vst::IParamValueQueue::iid,
                    sizeof(Steinberg::TUID)) == 0) {
      *obj = this;
      return Steinberg::kResultOk;
    }
    *obj = nullptr;
    return Steinberg::kNoInterface;
  }
  Steinberg::uint32 PLUGIN_API addRef()  override { return 1; }
  Steinberg::uint32 PLUGIN_API release() override { return 1; }

  Steinberg::Vst::ParamID PLUGIN_API getParameterId() override { return param_id; }
  Steinberg::int32        PLUGIN_API getPointCount()  override { return 1; }

  Steinberg::tresult PLUGIN_API getPoint(
      Steinberg::int32 index,
      Steinberg::int32& sample_offset,
      Steinberg::Vst::ParamValue& value) override {
    if (index != 0) return Steinberg::kResultFalse;
    sample_offset = 0;
    value = param_value;
    return Steinberg::kResultOk;
  }
  Steinberg::tresult PLUGIN_API addPoint(
      Steinberg::int32,
      Steinberg::Vst::ParamValue v,
      Steinberg::int32& idx) override {
    param_value = v;
    idx = 0;
    return Steinberg::kResultOk;
  }
};

/// Minimal IParameterChanges: fixed capacity, no dynamic allocation.
/// Reused each process call — reset() clears the count without freeing memory.
struct SimpleParamChanges final : Steinberg::Vst::IParameterChanges {
  static constexpr int kMaxQueues = 64;

  std::array<SimpleParamValueQueue, kMaxQueues> queues{};
  int count{0};

  Steinberg::tresult PLUGIN_API queryInterface(
      const Steinberg::TUID iid, void** obj) override {
    if (std::memcmp(iid, Steinberg::Vst::IParameterChanges::iid,
                    sizeof(Steinberg::TUID)) == 0) {
      *obj = this;
      return Steinberg::kResultOk;
    }
    *obj = nullptr;
    return Steinberg::kNoInterface;
  }
  Steinberg::uint32 PLUGIN_API addRef()  override { return 1; }
  Steinberg::uint32 PLUGIN_API release() override { return 1; }

  Steinberg::int32 PLUGIN_API getParameterCount() override { return count; }

  Steinberg::Vst::IParamValueQueue* PLUGIN_API getParameterData(
      Steinberg::int32 index) override {
    if (index < 0 || index >= count) return nullptr;
    return &queues[index];
  }

  Steinberg::Vst::IParamValueQueue* PLUGIN_API addParameterData(
      const Steinberg::Vst::ParamID& id,
      Steinberg::int32& index) override {
    if (count >= kMaxQueues) return nullptr;
    index = count;
    queues[count].param_id    = id;
    queues[count].param_value = 0.0;
    return &queues[count++];
  }

  void reset() { count = 0; }
};

/// Minimal IEventList: fixed capacity, no dynamic allocation.
struct SimpleEventList final : Steinberg::Vst::IEventList {
  static constexpr int kMaxEvents = 256;

  std::array<Steinberg::Vst::Event, kMaxEvents> events{};
  int count{0};

  Steinberg::tresult PLUGIN_API queryInterface(
      const Steinberg::TUID iid, void** obj) override {
    if (std::memcmp(iid, Steinberg::Vst::IEventList::iid,
                    sizeof(Steinberg::TUID)) == 0) {
      *obj = this;
      return Steinberg::kResultOk;
    }
    *obj = nullptr;
    return Steinberg::kNoInterface;
  }
  Steinberg::uint32 PLUGIN_API addRef()  override { return 1; }
  Steinberg::uint32 PLUGIN_API release() override { return 1; }

  Steinberg::int32 PLUGIN_API getEventCount() override { return count; }

  Steinberg::tresult PLUGIN_API getEvent(
      Steinberg::int32 index, Steinberg::Vst::Event& e) override {
    if (index < 0 || index >= count) return Steinberg::kResultFalse;
    e = events[index];
    return Steinberg::kResultOk;
  }

  Steinberg::tresult PLUGIN_API addEvent(Steinberg::Vst::Event& e) override {
    if (count >= kMaxEvents) return Steinberg::kResultFalse;
    events[count++] = e;
    return Steinberg::kResultOk;
  }

  void reset() { count = 0; }

  bool push_note_on(Steinberg::int32 sample_offset, Steinberg::int16 channel,
                    Steinberg::int16 pitch, float velocity) {
    if (count >= kMaxEvents) return false;
    auto& e = events[count++];
    e = {};
    e.busIndex = 0;
    e.sampleOffset = sample_offset;
    e.type = Steinberg::Vst::Event::kNoteOnEvent;
    e.noteOn.channel = channel;
    e.noteOn.pitch = pitch;
    e.noteOn.velocity = velocity;
    e.noteOn.tuning = 0.f;
    e.noteOn.length = 0;
    e.noteOn.noteId = -1;
    return true;
  }

  bool push_note_off(Steinberg::int32 sample_offset, Steinberg::int16 channel,
                     Steinberg::int16 pitch, float velocity) {
    if (count >= kMaxEvents) return false;
    auto& e = events[count++];
    e = {};
    e.busIndex = 0;
    e.sampleOffset = sample_offset;
    e.type = Steinberg::Vst::Event::kNoteOffEvent;
    e.noteOff.channel = channel;
    e.noteOff.pitch = pitch;
    e.noteOff.velocity = velocity;
    e.noteOff.tuning = 0.f;
    e.noteOff.noteId = -1;
    return true;
  }

  void sort_by_sample_offset() {
    if (count <= 1) return;
    std::sort(events.begin(), events.begin() + count,
              [](const Steinberg::Vst::Event& a, const Steinberg::Vst::Event& b) {
                return a.sampleOffset < b.sampleOffset;
              });
  }
};

namespace {

bool daux_vst3_midi_debug() {
  static const bool enabled =
      std::getenv("FUTUREBOARD_FORENSIC_TRACE") != nullptr ||
      std::getenv("FUTUREBOARD_VST3_MIDI_DEBUG") != nullptr;
  return enabled;
}

}  // namespace

// Forward declaration so ComponentHandlerImpl can hold a back-pointer.
struct SphereDauxVst3Processor;

#if defined(_WIN32)
// IPlugFrame implementation — see definition below. Forward-declared so the
// processor can hold a raw pointer to its editor frame.
class PluginEditorFrame;
#endif

/// IComponentHandler that captures performEdit() callbacks from the plugin GUI
/// and enqueues them for delivery to IAudioProcessor on the next process call.
struct ComponentHandlerImpl final : Steinberg::Vst::IComponentHandler {
  SphereDauxVst3Processor* owner{nullptr};

  Steinberg::tresult PLUGIN_API queryInterface(
      const Steinberg::TUID iid, void** obj) override {
    if (std::memcmp(iid, Steinberg::Vst::IComponentHandler::iid,
                    sizeof(Steinberg::TUID)) == 0) {
      *obj = this;
      return Steinberg::kResultOk;
    }
    *obj = nullptr;
    return Steinberg::kNoInterface;
  }
  Steinberg::uint32 PLUGIN_API addRef()  override { return 1; }
  Steinberg::uint32 PLUGIN_API release() override { return 1; }

  Steinberg::tresult PLUGIN_API beginEdit(Steinberg::Vst::ParamID) override {
    return Steinberg::kResultOk;
  }
  Steinberg::tresult PLUGIN_API endEdit(Steinberg::Vst::ParamID) override {
    return Steinberg::kResultOk;
  }
  Steinberg::tresult PLUGIN_API restartComponent(Steinberg::int32) override {
    return Steinberg::kResultOk;
  }

  // Defined below, after SphereDauxVst3Processor is complete.
  Steinberg::tresult PLUGIN_API performEdit(
      Steinberg::Vst::ParamID id,
      Steinberg::Vst::ParamValue value) override;
};

// ── Platform editor function forward declarations ─────────────────────────────
// Implementations live in editor_mac.mm (macOS) and editor_linux.cpp (Linux).

#if defined(__APPLE__)
unsigned long long open_editor_mac(SphereDauxVst3Processor*, const char*, const char*, int, int);
void  close_editor_mac(SphereDauxVst3Processor*);
int   focus_editor_mac(SphereDauxVst3Processor*);
void  shutdown_editor_mac(SphereDauxVst3Processor*);
#elif defined(__linux__)
unsigned long long open_editor_linux(SphereDauxVst3Processor*, const char*, const char*, int, int);
void  close_editor_linux(SphereDauxVst3Processor*);
int   focus_editor_linux(SphereDauxVst3Processor*);
void  shutdown_editor_linux(SphereDauxVst3Processor*);
#endif

// ── Main processor struct ─────────────────────────────────────────────────────

struct SphereDauxVst3Processor {
  static constexpr int kMaxPending = 64;

  VST3::Hosting::Module::Ptr                        module;
  Steinberg::Vst::HostApplication                   host_context;
  Steinberg::IPtr<Steinberg::Vst::IComponent>       component;
  Steinberg::IPtr<Steinberg::Vst::IAudioProcessor>  processor;
  Steinberg::IPtr<Steinberg::Vst::IEditController>  controller;
  /// MIDI controller → parameter mapping (queried from `controller`). Null when
  /// the plugin exposes no IMidiMapping; CC events are then ignored.
  Steinberg::IPtr<Steinberg::Vst::IMidiMapping>     midi_mapping;
  Steinberg::IPtr<Steinberg::Vst::IConnectionPoint> component_connection;
  Steinberg::IPtr<Steinberg::Vst::IConnectionPoint> controller_connection;
  bool controller_is_component{false};

  // Stereo single-sample I/O buffers
  Steinberg::Vst::SpeakerArrangement input_arrangement  = Steinberg::Vst::SpeakerArr::kStereo;
  Steinberg::Vst::SpeakerArrangement output_arrangement = Steinberg::Vst::SpeakerArr::kStereo;
  float  input_l{0.f}, input_r{0.f};
  float  output_l{0.f}, output_r{0.f};
  float* input_channels[2]  = {&input_l,  &input_r};
  float* output_channels[2] = {&output_l, &output_r};
  Steinberg::Vst::AudioBusBuffers input_bus{};
  Steinberg::Vst::AudioBusBuffers output_bus{};
  Steinberg::Vst::ProcessContext  process_context{};
  Steinberg::Vst::ProcessData     process_data{};
  bool processing{false};

  // Diagnostics
  unsigned long long process_count{0};
  double last_input_peak{0.0};
  double last_output_peak{0.0};
  double last_difference_peak{0.0};
  bool   first_process_done{false};

  // Thread-safe parameter change queue (no dynamic allocation)
  std::array<PendingParam, kMaxPending> pending_buf{};
  int                                   pending_count{0};
  std::mutex                            pending_mutex;  // protects pending_buf/count

  SimpleParamChanges   param_changes_obj;  // reused per process call
  SimpleEventList      input_events_obj;   // reused per process call
  int                  event_input_bus_count{0};
  ComponentHandlerImpl component_handler;  // installed on IEditController
  /// Owned copy of the loaded module path (survives after create() returns).
  std::string plugin_path;

#if defined(_WIN32)
  Steinberg::IPtr<Steinberg::IPlugView> editor_view;
  // IPlugFrame handed to the view via setFrame() before attached(). Owned by
  // this processor; created on attach, destroyed on detach. Required for
  // WebView/CEF-backed editors (UAD Native) to bootstrap and to honour
  // plug-in-driven resizeView() requests.
  PluginEditorFrame* editor_frame{nullptr};
  HWND editor_hwnd{nullptr};
  HWND editor_attach_hwnd{nullptr};
  HWND editor_fallback_label_hwnd{nullptr};
  HWND editor_fallback_reload_hwnd{nullptr};
  HWND editor_fallback_generic_hwnd{nullptr};
  HWND editor_fallback_close_hwnd{nullptr};
  unsigned long long editor_handle{0};
  std::string editor_window_id;
  std::string editor_title;
  int editor_requested_width{0};
  int editor_requested_height{0};
  bool editor_attached{false};
  // ── GPUI-embedded editor state ───────────────────────────────────────────
  // When the editor is hosted inside a GPUI PluginView window (rather than the
  // daux-owned top-level shell above), `editor_parent_hwnd` is the GPUI window,
  // `editor_embed_top_hwnd` is the native editor shell, and
  // `editor_attach_hwnd` is the dedicated child content HWND passed to
  // IPlugView::attached(). The IPlugView is created from THIS processor's
  // existing `controller` — never a new component/controller.
  HWND editor_parent_hwnd{nullptr};
  HWND editor_embed_top_hwnd{nullptr};
  int  embed_host_kind{1};        // 0 = WS_CHILD, 1 = owned tool window, 2 = detached top-level
  bool embed_mode{false};
  bool embed_geometry_valid{false};
  RECT embed_last_applied{};      // last applied window rect (screen for tool)
  int  embed_host_x{0}, embed_host_y{0}, embed_host_w{0}, embed_host_h{0};
  int  embed_content_w{0}, embed_content_h{0};
  bool embed_resize_in_progress{false};
  // IPlugView::canResize, cached per created view (keyed on the view pointer
  // so view re-creation re-queries automatically). Drives the generic resize
  // contract: fixed-size views keep their getSize; resizable views go through
  // checkSizeConstraint.
  bool editor_resizable{false};
  const void* editor_resizable_view{nullptr};
  // Main-owned shell (bridge): resizeView updates these; Rust polls and resizes
  // the NativeEditorShell outer window — never SetWindowPos on editor_parent_hwnd.
  std::atomic<bool> pending_main_shell_resize{false};
  int pending_main_shell_w{0};
  int pending_main_shell_h{0};
  std::string embed_instance_label;
  // Detached mode (kind==2): set by the detached window's WM_CLOSE so the Rust
  // shell can tear the editor down. Consumed (and reset) via the take accessor.
  std::atomic<bool> embed_user_closed{false};
  // Bundled browser/WebView runtime — active only while an editor is open.
  // One DLL-directory cookie per native runtime dir we added to the search path.
  std::vector<DLL_DIRECTORY_COOKIE> plugin_browser_dll_cookies;
  HMODULE plugin_browser_loader = nullptr;  // optional verify-load (WebView2 only)
  int plugin_browser_runtime_kind = 0;      // DauxEditorRuntimeKind
  // Guards window proc access; set to false before destroy so pending messages
  // received after GWLP_USERDATA is zeroed still find a valid flag.
  std::atomic<bool> processor_valid{true};
#elif defined(__APPLE__) || defined(__linux__)
  // Platform editor state (macOS / Linux).
  // ObjC and GTK4 types are hidden behind void* to keep this C++ TU clean.
  // editor_mac.mm / editor_linux.cpp access these exclusively via the
  // sphere_daux_editor_bridge.h C API.
  Steinberg::IPtr<Steinberg::IPlugView> editor_view;
  void* editor_native_window{nullptr};    // NSWindow* (mac) / GtkWidget* (linux)
  void* editor_native_embed{nullptr};     // NSView* (mac only — the IPlugView parent)
  void* editor_native_delegate{nullptr};  // DauxEditorWindowDelegate* (mac only)
  unsigned long long editor_handle{0};
  std::string editor_window_id;
  std::string editor_title;
  int editor_requested_width{0};
  int editor_requested_height{0};
  bool editor_attached{false};
  std::atomic<bool> processor_valid{true};
#endif

  // ── Setup / shutdown ───────────────────────────────────────────────────────

  bool setup(double sample_rate) {
    const double sr = sample_rate > 0.0 ? sample_rate : 44100.0;

    // Wire up I/O buffer descriptors
    input_bus.numChannels       = 2;
    input_bus.channelBuffers32  = input_channels;
    output_bus.numChannels      = 2;
    output_bus.channelBuffers32 = output_channels;

    // ProcessData is reused every call — initialise once here
    process_data.processMode        = Steinberg::Vst::kRealtime;
    process_data.symbolicSampleSize = Steinberg::Vst::kSample32;
    process_data.numSamples         = 1;
    process_data.numInputs          = 1;
    process_data.numOutputs         = 1;
    process_data.inputs             = &input_bus;
    process_data.outputs            = &output_bus;
    process_data.inputParameterChanges  = nullptr;
    process_data.outputParameterChanges = nullptr;

    process_context.sampleRate         = sr;
    process_context.tempo              = 120.0;
    process_context.timeSigNumerator   = 4;
    process_context.timeSigDenominator = 4;
    process_context.state =
        Steinberg::Vst::ProcessContext::kTempoValid   |
        Steinberg::Vst::ProcessContext::kTimeSigValid |
        Steinberg::Vst::ProcessContext::kPlaying;
    process_data.processContext = &process_context;

    const auto input_bus_count =
        component->getBusCount(Steinberg::Vst::kAudio, Steinberg::Vst::kInput);
    const auto output_bus_count =
        component->getBusCount(Steinberg::Vst::kAudio, Steinberg::Vst::kOutput);
    std::fprintf(stderr, "[SphereVST3] busCount input=%d output=%d\n",
                 (int)input_bus_count, (int)output_bus_count);

    event_input_bus_count =
        component->getBusCount(Steinberg::Vst::kEvent, Steinberg::Vst::kInput);
    std::fprintf(stderr, "[SphereVST3] eventInputBusCount=%d\n", event_input_bus_count);
    if (event_input_bus_count > 0) {
      const auto ev_res = component->activateBus(
          Steinberg::Vst::kEvent, Steinberg::Vst::kInput, 0, true);
      if (ev_res != Steinberg::kResultOk) {
        std::fprintf(stderr,
                     "[SphereVST3] activate event input bus FAILED (result=%d)\n",
                     (int)ev_res);
      }
    }

    // Set stereo bus arrangements before bus activation. Some VST3 processors
    // reject processing if the arrangement is changed after activation.
    const auto arrangement_res =
        processor->setBusArrangements(&input_arrangement, 1, &output_arrangement, 1);
    if (arrangement_res != Steinberg::kResultOk) {
      std::ostringstream err;
      err << g_last_error
          << "; setBusArrangements returned " << (int)arrangement_res
          << " for requested stereo in/out; continuing with plugin default arrangement";
      set_last_error(err.str());
      std::fprintf(stderr, "[SphereVST3] setBusArrangements result=%d failed\n",
                   (int)arrangement_res);
    } else {
      std::fprintf(stderr, "[SphereVST3] setBusArrangements result=%d ok stereo in/out\n",
                   (int)arrangement_res);
    }

    // Activate stereo buses
    const auto in_res  = component->activateBus(
        Steinberg::Vst::kAudio, Steinberg::Vst::kInput,  0, true);
    const auto out_res = component->activateBus(
        Steinberg::Vst::kAudio, Steinberg::Vst::kOutput, 0, true);
    if (in_res  != Steinberg::kResultOk)
      std::fprintf(stderr, "[DAUx VST3] activate input bus FAILED (result=%d)\n",  (int)in_res);
    if (out_res != Steinberg::kResultOk)
      std::fprintf(stderr, "[DAUx VST3] activate output bus FAILED (result=%d)\n", (int)out_res);

    std::fprintf(stderr, "[SphereVST3] activateBus inputResult=%d outputResult=%d stereo in/out\n",
                 (int)in_res, (int)out_res);

    // setupProcessing
    Steinberg::Vst::ProcessSetup ps{};
    ps.processMode        = Steinberg::Vst::kRealtime;
    ps.symbolicSampleSize = Steinberg::Vst::kSample32;
    ps.maxSamplesPerBlock = 8192;
    ps.sampleRate         = sr;
    const auto setup_res = processor->setupProcessing(ps);
    if (setup_res != Steinberg::kResultOk) {
      std::ostringstream err;
      err << g_last_error << "; setupProcessing returned " << (int)setup_res
          << " sr=" << sr << " maxBlock=8192 realtime sample32";
      set_last_error(err.str());
      std::fprintf(stderr, "[DAUx VST3] setupProcessing FAILED (result=%d)\n", (int)setup_res);
      return false;
    }
    std::fprintf(stderr, "[SphereVST3] setupProcessing sr=%.0f block=8192 result=%d ok realtime sample32\n",
                 sr, (int)setup_res);

    // setActive(true) — accept kResultOk and kNotImplemented (some plugins
    // simply don't need explicit activation; treating not-implemented as fatal
    // would block legitimate plugins).
    const auto active_res = component->setActive(true);
    if (active_res != Steinberg::kResultOk && active_res != Steinberg::kNotImplemented) {
      std::ostringstream err;
      err << g_last_error << "; setActive(true) returned " << (int)active_res;
      set_last_error(err.str());
      std::fprintf(stderr, "[DAUx VST3] setActive(true) FAILED (result=%d)\n", (int)active_res);
      return false;
    }
    std::fprintf(stderr, "[SphereVST3] setActive result=%d ok\n", (int)active_res);

    // setProcessing(true) — accept kResultOk and kNotImplemented.
    // Per VST3 spec, setProcessing is an optional notification; plugins like
    // iZotope Ozone return kNotImplemented (0x80004001) and that's legitimate.
    const auto proc_res = processor->setProcessing(true);
    if (proc_res != Steinberg::kResultOk && proc_res != Steinberg::kNotImplemented) {
      std::ostringstream err;
      err << g_last_error << "; setProcessing(true) returned " << (int)proc_res;
      set_last_error(err.str());
      std::fprintf(stderr, "[DAUx VST3] setProcessing(true) FAILED (result=%d)\n", (int)proc_res);
      return false;
    }
    processing = true;
    std::fprintf(stderr, "[SphereVST3] setProcessing result=%d ok (notImplemented=%d)\n",
                 (int)proc_res, proc_res == Steinberg::kNotImplemented ? 1 : 0);

    // Register IComponentHandler so plugin GUI edits are captured
    if (controller) {
      component_handler.owner = this;
      const auto ch_res = controller->setComponentHandler(&component_handler);
      if (ch_res == Steinberg::kResultOk)
        std::fprintf(stderr, "[DAUx VST3] IComponentHandler registered\n");
      else
        std::fprintf(stderr,
                     "[DAUx VST3] setComponentHandler not accepted (result=%d) — "
                     "GUI edits may not reach processor\n", (int)ch_res);

      // MIDI controller → parameter mapping (optional). Used to route CC /
      // pitch-bend / aftertouch input to parameter changes during process().
      midi_mapping = Steinberg::FUnknownPtr<Steinberg::Vst::IMidiMapping>(controller);
      std::fprintf(stderr, "[DAUx VST3] IMidiMapping %s\n",
                   midi_mapping ? "available" : "not exposed");
    }

    return true;
  }

  void shutdown() {
#if defined(_WIN32)
    close_editor_window();
#elif defined(__APPLE__)
    shutdown_editor_mac(this);
#elif defined(__linux__)
    shutdown_editor_linux(this);
#endif
    if (processor && processing) processor->setProcessing(false);
    processing = false;
    if (component_connection && controller_connection) {
      component_connection->disconnect(controller_connection);
      controller_connection->disconnect(component_connection);
    }
    component_connection = nullptr;
    controller_connection = nullptr;
    if (controller && !controller_is_component) {
      if (auto pb = Steinberg::FUnknownPtr<Steinberg::IPluginBase>(controller))
        pb->terminate();
    }
    if (component) {
      component->setActive(false);
      if (auto pb = Steinberg::FUnknownPtr<Steinberg::IPluginBase>(component))
        pb->terminate();
    }
  }

  // ── Thread-safe parameter enqueue (called from audio thread OR GUI thread) ──

  /// Drain pending parameters, route MIDI CC to parameter changes, and fill
  /// inputEvents with note on/off for this block.
  void prepare_process_io(const SphereDauxVst3MidiEvent* midi_events,
                          int midi_event_count) {
    // Parameter changes come from two sources this block: GUI/host edits queued
    // in `pending_buf`, and CC events mapped via IMidiMapping below.
    param_changes_obj.reset();
    {
      std::lock_guard<std::mutex> lock(pending_mutex);
      for (int i = 0; i < pending_count; ++i) {
        Steinberg::int32 idx = 0;
        auto* q = param_changes_obj.addParameterData(pending_buf[i].id, idx);
        if (q) {
          Steinberg::int32 dummy = 0;
          q->addPoint(0, pending_buf[i].value, dummy);
        }
      }
      pending_count = 0;
    }

    input_events_obj.reset();
    int cc_mapped = 0;
    if (midi_events && midi_event_count > 0) {
      const int n = std::min(midi_event_count, SimpleEventList::kMaxEvents);
      for (int i = 0; i < n; ++i) {
        const auto& m = midi_events[i];
        const auto ch = static_cast<Steinberg::int16>(m.channel & 0x0F);
        const auto offset = static_cast<Steinberg::int32>(m.sample_offset);
        if (m.kind == 2) {
          // ControlChange → parameter change via IMidiMapping. `pitch` carries
          // the VST3 controller number (not masked to 7 bits: 128/129 are
          // aftertouch / pitch bend). The block-level value wins (our
          // SimpleParamValueQueue holds a single point).
          if (!midi_mapping) {
            continue;
          }
          const auto ctrl = static_cast<Steinberg::Vst::CtrlNumber>(m.pitch);
          Steinberg::Vst::ParamID pid = 0;
          if (midi_mapping->getMidiControllerAssignment(0, ch, ctrl, pid) ==
              Steinberg::kResultOk) {
            Steinberg::int32 idx = 0;
            auto* q = param_changes_obj.addParameterData(pid, idx);
            if (q) {
              Steinberg::int32 dummy = 0;
              q->addPoint(offset,
                          static_cast<Steinberg::Vst::ParamValue>(m.velocity),
                          dummy);
              ++cc_mapped;
            }
          }
          continue;
        }
        if (event_input_bus_count <= 0) {
          continue;
        }
        const auto pitch = static_cast<Steinberg::int16>(m.pitch & 0x7F);
        if (m.kind == 1) {
          input_events_obj.push_note_on(offset, ch, pitch, m.velocity);
        } else {
          input_events_obj.push_note_off(offset, ch, pitch, m.velocity);
        }
      }
      input_events_obj.sort_by_sample_offset();

      if (daux_vst3_midi_debug()) {
        std::fprintf(stderr,
                     "[vst3-midi] input_events=%d event_bus=%d active=%s\n",
                     input_events_obj.count,
                     event_input_bus_count,
                     event_input_bus_count > 0 ? "true" : "false");
        for (int i = 0; i < input_events_obj.count; ++i) {
          const auto& e = input_events_obj.events[i];
          if (e.type == Steinberg::Vst::Event::kNoteOnEvent) {
            std::fprintf(stderr,
                         "[vst3-midi] add note_on pitch=%d velocity=%.2f sampleOffset=%d\n",
                         (int)e.noteOn.pitch,
                         e.noteOn.velocity,
                         (int)e.sampleOffset);
          } else if (e.type == Steinberg::Vst::Event::kNoteOffEvent) {
            std::fprintf(stderr,
                         "[vst3-midi] add note_off pitch=%d sampleOffset=%d\n",
                         (int)e.noteOff.pitch,
                         (int)e.sampleOffset);
          }
        }
      }
    }

    process_data.inputParameterChanges =
        (param_changes_obj.count > 0) ? &param_changes_obj : nullptr;
    process_data.inputEvents =
        (input_events_obj.count > 0) ? &input_events_obj : nullptr;
  }

  /// Add or update a parameter change in the pending queue.
  /// Deduplicates by paramId — later value wins within one block.
  void enqueue_param(Steinberg::Vst::ParamID id, Steinberg::Vst::ParamValue value) {
    std::lock_guard<std::mutex> lock(pending_mutex);
    for (int i = 0; i < pending_count; ++i) {
      if (pending_buf[i].id == id) {
        pending_buf[i].value = value;
        return;
      }
    }
    if (pending_count < kMaxPending)
      pending_buf[pending_count++] = {id, value};
  }

#ifdef _WIN32
  void close_editor_window();
  void close_editor_view_only(const char* reason);
  // Detach the embedded IPlugView and destroy the host window, keeping the
  // component/controller (and thus the realtime processor) alive.
  void close_embed_editor(const char* reason);
#endif
};

#if defined(_WIN32)
bool daux_vst3_editor_debug() {
  static const bool enabled = std::getenv("FUTUREBOARD_VST3_EDITOR_DEBUG") != nullptr;
  return enabled;
}

inline bool daux_view_rect_equal(const Steinberg::ViewRect& a, const Steinberg::ViewRect& b) {
  return a.left == b.left && a.top == b.top && a.right == b.right && a.bottom == b.bottom;
}

inline int daux_view_rect_width(const Steinberg::ViewRect& r) {
  return static_cast<int>(r.right - r.left);
}

inline int daux_view_rect_height(const Steinberg::ViewRect& r) {
  return static_cast<int>(r.bottom - r.top);
}

inline Steinberg::ViewRect daux_local_view_rect(int width, int height) {
  return Steinberg::ViewRect{
      0,
      0,
      static_cast<Steinberg::int32>(width),
      static_cast<Steinberg::int32>(height),
  };
}

bool daux_embed_apply_content_size(SphereDauxVst3Processor* p,
                                   int content_w,
                                   int content_h,
                                   const char* reason);

bool daux_adjust_window_rect_for_dpi(HWND hwnd, RECT* rect, DWORD style, DWORD ex_style) {
  if (!hwnd || !rect) return false;
  UINT dpi = GetDpiForWindow(hwnd);
  if (dpi == 0) dpi = 96;
  return AdjustWindowRectExForDpi(rect, style, FALSE, ex_style, dpi) != 0;
}

UINT daux_hwnd_dpi(HWND hwnd) {
  if (!hwnd || !IsWindow(hwnd)) return 96;
  const UINT dpi = GetDpiForWindow(hwnd);
  return dpi > 0 ? dpi : 96;
}

void daux_log_editor_dpi(HWND ref_hwnd, const char* label) {
  const UINT dpi = daux_hwnd_dpi(ref_hwnd);
  const double scale = static_cast<double>(dpi) / 96.0;
  std::fprintf(stderr, "[PluginEditor] %s dpi=%u\n", label ? label : "dpi", dpi);
  std::fprintf(stderr, "[PluginEditor] %s scale=%.3f\n", label ? label : "scale", scale);
}

void daux_ensure_thread_dpi_awareness() {
  static std::once_flag once;
  std::call_once(once, [] {
    using SetThreadDpiAwarenessContextFn = DPI_AWARENESS_CONTEXT(WINAPI*)(DPI_AWARENESS_CONTEXT);
    using GetThreadDpiAwarenessContextFn = DPI_AWARENESS_CONTEXT(WINAPI*)();
    HMODULE user32 = GetModuleHandleW(L"user32.dll");
    auto* set_ctx = user32
                        ? reinterpret_cast<SetThreadDpiAwarenessContextFn>(
                              GetProcAddress(user32, "SetThreadDpiAwarenessContext"))
                        : nullptr;
    auto* get_ctx = user32
                        ? reinterpret_cast<GetThreadDpiAwarenessContextFn>(
                              GetProcAddress(user32, "GetThreadDpiAwarenessContext"))
                        : nullptr;
    if (set_ctx) {
      set_ctx(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    } else {
      SetProcessDPIAware();
    }
    if (get_ctx) {
      std::fprintf(stderr,
                   "[PluginEditor] dpi_awareness_context=0x%p tid=%lu\n",
                   static_cast<void*>(get_ctx()),
                   GetCurrentThreadId());
    } else {
      std::fprintf(stderr,
                   "[PluginEditor] dpi_awareness_context=legacy tid=%lu\n",
                   GetCurrentThreadId());
    }
  });
}

bool daux_verify_child_client_rect(HWND child,
                                   int expected_w,
                                   int expected_h,
                                   const char* phase) {
  if (!child || !IsWindow(child)) {
    std::fprintf(stderr,
                 "[PluginEditor] ERROR %s child_hwnd invalid hwnd=0x%p\n",
                 phase ? phase : "verify",
                 static_cast<void*>(child));
    return false;
  }
  RECT cr{};
  GetClientRect(child, &cr);
  const int cw = static_cast<int>(cr.right - cr.left);
  const int ch = static_cast<int>(cr.bottom - cr.top);
  if (cw <= 0 || ch <= 0 || cw != expected_w || ch != expected_h) {
    std::fprintf(stderr,
                 "[PluginEditor] ERROR %s child client=%dx%d expected=%dx%d\n",
                 phase ? phase : "verify",
                 cw,
                 ch,
                 expected_w,
                 expected_h);
    return false;
  }
  std::fprintf(stderr,
               "[PluginEditor] %s final child client=%dx%d\n",
               phase ? phase : "verify",
               cw,
               ch);
  return true;
}

bool daux_resize_child_client(HWND child, int content_w, int content_h) {
  if (!child || !IsWindow(child) || content_w <= 0 || content_h <= 0) return false;
  SetWindowPos(child,
               nullptr,
               0,
               0,
               content_w,
               content_h,
               SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW);
  return daux_verify_child_client_rect(child, content_w, content_h, "resize_child");
}

// IPlugFrame for VST3 editor hosting. Mirrors the SDK editorhost sample:
// plugView->setFrame(this) BEFORE plugView->attached(...). WebView2/CEF editors
// (UAD Native et al.) require a valid frame to bootstrap and call resizeView().
class PluginEditorFrame final : public Steinberg::IPlugFrame {
 public:
  explicit PluginEditorFrame(SphereDauxVst3Processor* owner) : owner_(owner) {}

  Steinberg::tresult PLUGIN_API resizeView(Steinberg::IPlugView* view,
                                           Steinberg::ViewRect* newSize) override {
    const bool debug = daux_vst3_editor_debug();
    if (debug) {
      std::fprintf(stderr, "[vst3-editor] resizeView called view=0x%p\n", static_cast<void*>(view));
    }
    if (newSize == nullptr || view == nullptr || !owner_) {
      if (debug) std::fprintf(stderr, "[vst3-editor] resizeView rejected (invalid args)\n");
      return Steinberg::kInvalidArgument;
    }
    if (view != owner_->editor_view.get()) {
      if (debug) std::fprintf(stderr, "[vst3-editor] resizeView rejected (view mismatch)\n");
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
    if (daux_view_rect_equal(current, *newSize)) {
      const int w = daux_view_rect_width(*newSize);
      const int h = daux_view_rect_height(*newSize);
      const bool applied = (w > 0 && h > 0)
                               ? daux_embed_apply_content_size(owner_, w, h, "resizeView.no-op")
                               : false;
      if (debug) {
        std::fprintf(stderr,
                     "[vst3-frame] resizeView requested=(%d,%d,%d,%d) content=%dx%d\n",
                     newSize->left,
                     newSize->top,
                     newSize->right,
                     newSize->bottom,
                     w,
                     h);
        std::fprintf(stderr,
                     "[vst3-frame] resizeView applied=%dx%d changed=%d\n",
                     w,
                     h,
                     applied ? 1 : 0);
        std::fprintf(stderr, "[vst3-editor] resizeView accepted (no-op)\n");
      }
      return Steinberg::kResultTrue;
    }

    const int w = daux_view_rect_width(*newSize);
    const int h = daux_view_rect_height(*newSize);
    if (debug) {
      std::fprintf(stderr,
                   "[vst3-frame] resizeView requested=(%d,%d,%d,%d) content=%dx%d\n",
                   newSize->left,
                   newSize->top,
                   newSize->right,
                   newSize->bottom,
                   w,
                   h);
    }

    resize_recursion_guard_ = true;
    bool applied = false;
    if (w > 0 && h > 0) {
      applied = daux_embed_apply_content_size(owner_, w, h, "resizeView");
    }
    resize_recursion_guard_ = false;

    Steinberg::ViewRect after{};
    if (view->getSize(&after) != Steinberg::kResultTrue) {
      if (debug) std::fprintf(stderr, "[vst3-editor] resizeView rejected (getSize after resize failed)\n");
      return Steinberg::kInternalError;
    }
    if (!daux_view_rect_equal(after, *newSize)) {
      auto local = daux_local_view_rect(w, h);
      const auto on_size_res = view->onSize(&local);
      if (debug) {
        std::fprintf(stderr,
                     "[vst3-editor] onSize result=0x%x rect=(%d,%d,%d,%d)\n",
                     static_cast<unsigned>(on_size_res),
                     local.left,
                     local.top,
                     local.right,
                     local.bottom);
      }
    }
    if (debug) {
      std::fprintf(stderr,
                   "[vst3-frame] resizeView applied=%dx%d changed=%d\n",
                   w,
                   h,
                   applied ? 1 : 0);
      std::fprintf(stderr,
                   "[vst3-editor] resizeView accepted\n");
    }
    return Steinberg::kResultOk;
  }

  Steinberg::tresult PLUGIN_API queryInterface(const Steinberg::TUID iid, void** obj) override {
    if (Steinberg::FUnknownPrivate::iidEqual(iid, Steinberg::IPlugFrame::iid) ||
        Steinberg::FUnknownPrivate::iidEqual(iid, Steinberg::FUnknown::iid)) {
      *obj = static_cast<Steinberg::IPlugFrame*>(this);
      addRef();
      return Steinberg::kResultTrue;
    }
    *obj = nullptr;
    return Steinberg::kNoInterface;
  }
  // Lifetime owned by the processor — a plug-in release() must not destroy us.
  Steinberg::uint32 PLUGIN_API addRef() override { return 1000; }
  Steinberg::uint32 PLUGIN_API release() override { return 1000; }

 private:
  SphereDauxVst3Processor* owner_;
  bool resize_recursion_guard_{false};
};

// Create (if needed) and install the IPlugFrame on the view before attached().
void daux_editor_install_frame(SphereDauxVst3Processor* processor) {
  if (!processor || !processor->editor_view) return;
  if (!processor->editor_frame) {
    processor->editor_frame = new PluginEditorFrame(processor);
  }
  if (daux_vst3_editor_debug()) {
    std::fprintf(stderr,
                 "[vst3-editor] setFrame called view=0x%p frame=0x%p\n",
                 static_cast<void*>(processor->editor_view.get()),
                 static_cast<void*>(processor->editor_frame));
  }
  const auto res = processor->editor_view->setFrame(processor->editor_frame);
  std::fprintf(stderr, "[vst3-editor] setFrame result=0x%x\n", static_cast<unsigned>(res));
}

void daux_editor_clear_frame(SphereDauxVst3Processor* processor) {
  if (!processor) return;
  if (processor->editor_view) {
    if (daux_vst3_editor_debug()) {
      std::fprintf(stderr, "[vst3-editor] setFrame null view=0x%p\n",
                   static_cast<void*>(processor->editor_view.get()));
    }
    processor->editor_view->setFrame(nullptr);
  }
  delete processor->editor_frame;
  processor->editor_frame = nullptr;
}
#endif  // _WIN32

// ── Bridge implementations for platform editor TUs ────────────────────────────
// These give editor_mac.mm and editor_linux.cpp access to the TU-private
// globals (g_last_error, g_next_editor_handle) and the IPlugView members
// of SphereDauxVst3Processor without exposing the full struct definition.

extern "C" void sphere_daux_editor_set_error(const char* msg) {
  set_last_error(msg ? msg : "");
}

extern "C" unsigned long long sphere_daux_editor_next_handle(void) {
  return g_next_editor_handle.fetch_add(1);
}

#if defined(__APPLE__) || defined(__linux__)

extern "C" int sphere_daux_editor_create_view(
    SphereDauxVst3Processor* proc,
    const char*              platform_type,
    int*                     out_width,
    int*                     out_height) {
  if (!proc || !proc->controller) return 0;
  proc->editor_view = Steinberg::IPtr<Steinberg::IPlugView>::adopt(
      proc->controller->createView(Steinberg::Vst::ViewType::kEditor));
  if (!proc->editor_view) {
    std::fprintf(stderr,
                 "[SphereVST3] createView FAILED for platform='%s'\n",
                 platform_type);
    return 0;
  }
  if (proc->editor_view->isPlatformTypeSupported(platform_type) !=
      Steinberg::kResultTrue) {
    std::fprintf(stderr,
                 "[SphereVST3] platform type '%s' not supported by plugin\n",
                 platform_type);
    proc->editor_view = nullptr;
    return 0;
  }
  Steinberg::ViewRect rect{};
  if (proc->editor_view->getSize(&rect) == Steinberg::kResultTrue) {
    const int w = rect.right - rect.left;
    const int h = rect.bottom - rect.top;
    if (w > 0 && out_width)  *out_width  = w;
    if (h > 0 && out_height) *out_height = h;
  }
  return 1;
}

extern "C" int sphere_daux_editor_attach_view(
    SphereDauxVst3Processor* proc,
    void*                    native_handle,
    const char*              platform_type) {
  if (!proc || !proc->editor_view) return 0;
  const auto res = proc->editor_view->attached(native_handle, platform_type);
  std::fprintf(stderr,
               "[SphereVST3] IPlugView::attached('%s') result=%d\n",
               platform_type, (int)res);
  if (res != Steinberg::kResultTrue && res != Steinberg::kResultOk) {
    proc->editor_view    = nullptr;
    proc->editor_attached = false;
    return 0;
  }
  proc->editor_attached = true;
  return 1;
}

extern "C" void sphere_daux_editor_notify_resize(
    SphereDauxVst3Processor* proc, int width, int height) {
  if (!proc || !proc->editor_view) return;
  Steinberg::ViewRect rect{0, 0,
      static_cast<Steinberg::int32>(width),
      static_cast<Steinberg::int32>(height)};
  proc->editor_view->onSize(&rect);
}

extern "C" void sphere_daux_editor_detach_view(SphereDauxVst3Processor* proc) {
  if (!proc) return;
  if (proc->editor_view && proc->editor_attached) {
    const auto res = proc->editor_view->removed();
    std::fprintf(stderr,
                 "[SphereVST3] IPlugView::removed() result=%d handle=%llu\n",
                 (int)res, proc->editor_handle);
  }
  proc->editor_view     = nullptr;
  proc->editor_attached = false;
}

extern "C" void sphere_daux_editor_store_native(
    SphereDauxVst3Processor* proc,
    void*                    native_window,
    void*                    native_embed,
    void*                    native_delegate,
    unsigned long long       handle,
    const char*              window_id,
    const char*              title,
    int                      requested_width,
    int                      requested_height) {
  if (!proc) return;
  proc->editor_native_window    = native_window;
  proc->editor_native_embed     = native_embed;
  proc->editor_native_delegate  = native_delegate;
  proc->editor_handle           = handle;
  proc->editor_window_id        = window_id ? window_id : "";
  proc->editor_title            = title     ? title     : "";
  proc->editor_requested_width  = requested_width;
  proc->editor_requested_height = requested_height;
}

extern "C" void sphere_daux_editor_clear_native(SphereDauxVst3Processor* proc) {
  if (!proc) return;
  proc->editor_native_window    = nullptr;
  proc->editor_native_embed     = nullptr;
  proc->editor_native_delegate  = nullptr;
  proc->editor_handle           = 0;
  proc->editor_window_id.clear();
  proc->editor_title.clear();
  proc->editor_requested_width  = 0;
  proc->editor_requested_height = 0;
}

extern "C" void* sphere_daux_editor_get_native_window(SphereDauxVst3Processor* p)
  { return p ? p->editor_native_window   : nullptr; }
extern "C" void* sphere_daux_editor_get_native_embed(SphereDauxVst3Processor* p)
  { return p ? p->editor_native_embed    : nullptr; }
extern "C" void* sphere_daux_editor_get_native_delegate(SphereDauxVst3Processor* p)
  { return p ? p->editor_native_delegate : nullptr; }
extern "C" unsigned long long sphere_daux_editor_get_handle(SphereDauxVst3Processor* p)
  { return p ? p->editor_handle : 0; }
extern "C" const char* sphere_daux_editor_get_window_id(SphereDauxVst3Processor* p)
  { return p ? p->editor_window_id.c_str() : ""; }
extern "C" const char* sphere_daux_editor_get_title(SphereDauxVst3Processor* p)
  { return p ? p->editor_title.c_str() : ""; }
extern "C" int sphere_daux_editor_get_requested_width(SphereDauxVst3Processor* p)
  { return p ? p->editor_requested_width  : 0; }
extern "C" int sphere_daux_editor_get_requested_height(SphereDauxVst3Processor* p)
  { return p ? p->editor_requested_height : 0; }

#endif // __APPLE__ || __linux__

// ── ComponentHandlerImpl::performEdit (needs full SphereDauxVst3Processor) ───

Steinberg::tresult PLUGIN_API ComponentHandlerImpl::performEdit(
    Steinberg::Vst::ParamID id,
    Steinberg::Vst::ParamValue value) {
  if (owner) owner->enqueue_param(id, value);
  static std::atomic<int> logged{0};
  const int n = logged.fetch_add(1);
  if (n < 16 || n % 50 == 0) {
    std::fprintf(stderr,
                 "[SphereVST3] editor param -> processor param=%u value=%.6f count=%d\n",
                 static_cast<unsigned int>(id),
                 static_cast<double>(value),
                 n + 1);
  }
  return Steinberg::kResultOk;
}

#ifdef _WIN32
void detach_editor_view(SphereDauxVst3Processor* processor) {
  if (!processor) return;
  if (processor->editor_view && processor->editor_attached) {
    // Mirror editorhost teardown: clear frame, then removed().
    if (daux_vst3_editor_debug()) {
      std::fprintf(stderr, "[vst3-editor] setFrame null view=0x%p\n",
                   static_cast<void*>(processor->editor_view.get()));
    }
    processor->editor_view->setFrame(nullptr);
    const auto removed_res = processor->editor_view->removed();
    std::fprintf(stderr,
                 "[SphereVST3] IPlugView::removed() result=0x%x handle=%llu\n",
                 static_cast<unsigned>(removed_res),
                 processor->editor_handle);
    if (daux_vst3_editor_debug()) {
      std::fprintf(stderr, "[vst3-editor] removed result=0x%x\n",
                   static_cast<unsigned>(removed_res));
    }
  }
  delete processor->editor_frame;
  processor->editor_frame = nullptr;
  processor->editor_view = nullptr;
  processor->editor_attached = false;
}

void destroy_fallback_controls(SphereDauxVst3Processor* processor) {
  if (!processor) return;
  HWND controls[] = {
      processor->editor_fallback_label_hwnd,
      processor->editor_fallback_reload_hwnd,
      processor->editor_fallback_generic_hwnd,
      processor->editor_fallback_close_hwnd,
  };
  for (HWND control : controls) {
    if (control && IsWindow(control)) DestroyWindow(control);
  }
  processor->editor_fallback_label_hwnd = nullptr;
  processor->editor_fallback_reload_hwnd = nullptr;
  processor->editor_fallback_generic_hwnd = nullptr;
  processor->editor_fallback_close_hwnd = nullptr;
}

// ── Generic VST3 editor resize contract (no vendor/plugin logic) ─────────────

// Resize-path log throttle: at most 4 [PluginEditorResize] bursts per second so
// interactive drags never flood stderr.
bool daux_resize_log_allow() {
  static std::atomic<unsigned long long> window_start{0};
  static std::atomic<unsigned> count{0};
  const unsigned long long now = GetTickCount64();
  const unsigned long long start = window_start.load(std::memory_order_relaxed);
  if (now - start >= 1000) {
    window_start.store(now, std::memory_order_relaxed);
    count.store(1, std::memory_order_relaxed);
    return true;
  }
  return count.fetch_add(1, std::memory_order_relaxed) < 4;
}

// IPlugView::canResize, queried once per created view and cached (spec item 1).
// kResultTrue means the editor supports host-driven resizing; everything else
// is treated as fixed-size.
bool daux_editor_view_resizable(SphereDauxVst3Processor* p) {
  if (!p || !p->editor_view) return false;
  if (p->editor_resizable_view != p->editor_view.get()) {
    const auto res = p->editor_view->canResize();
    p->editor_resizable = (res == Steinberg::kResultTrue);
    p->editor_resizable_view = p->editor_view.get();
    std::fprintf(stderr,
                 "[PluginEditorResize] canResize result=0x%x resizable=%d\n",
                 static_cast<unsigned>(res),
                 p->editor_resizable ? 1 : 0);
  }
  return p->editor_resizable;
}

// Host/user-driven resize contract (editorhost `constrainSize`, spec item 2):
// fixed-size views snap to their current `getSize`; resizable views go through
// `IPlugView::checkSizeConstraint` and use the plugin-adjusted rect. `w`/`h`
// are PLUGIN CONTENT dimensions (never include titlebar/non-client frame).
// Returns true when the constraint changed the requested size.
bool daux_constrain_content_size(SphereDauxVst3Processor* p, int* w, int* h) {
  if (!p || !p->editor_view || !w || !h || *w <= 0 || *h <= 0) return false;
  const int want_w = *w;
  const int want_h = *h;
  if (!daux_editor_view_resizable(p)) {
    Steinberg::ViewRect cur{};
    const auto gs = p->editor_view->getSize(&cur);
    if (gs == Steinberg::kResultTrue || gs == Steinberg::kResultOk) {
      const int cw = daux_view_rect_width(cur);
      const int ch = daux_view_rect_height(cur);
      if (cw > 0 && ch > 0) {
        *w = cw;
        *h = ch;
      }
    }
  } else {
    Steinberg::ViewRect want{0, 0, want_w, want_h};
    const auto res = p->editor_view->checkSizeConstraint(&want);
    if (res == Steinberg::kResultTrue || res == Steinberg::kResultOk) {
      const int cw = daux_view_rect_width(want);
      const int ch = daux_view_rect_height(want);
      if (cw > 0 && ch > 0) {
        *w = cw;
        *h = ch;
      }
    }
    if (daux_resize_log_allow()) {
      std::fprintf(stderr,
                   "[PluginEditorResize] checkSizeConstraint result=0x%x\n",
                   static_cast<unsigned>(res));
    }
  }
  const bool changed = (*w != want_w || *h != want_h);
  if (daux_resize_log_allow()) {
    std::fprintf(stderr,
                 "[PluginEditorResize] desired_plugin=%dx%d constrained_plugin=%dx%d resizable=%d\n",
                 want_w,
                 want_h,
                 *w,
                 *h,
                 p->editor_resizable ? 1 : 0);
  }
  return changed;
}

void resize_editor_view(SphereDauxVst3Processor* processor) {
  if (!processor || !processor->editor_view || !processor->editor_attach_hwnd) return;
  RECT rc{};
  GetClientRect(processor->editor_attach_hwnd, &rc);
  const int content_w = static_cast<int>(rc.right - rc.left);
  const int content_h = static_cast<int>(rc.bottom - rc.top);
  if (content_w <= 0 || content_h <= 0) {
    std::fprintf(stderr,
                 "[PluginEditor] ERROR onSize skipped zero client=%dx%d handle=%llu\n",
                 content_w,
                 content_h,
                 processor->editor_handle);
    return;
  }
  auto local = daux_local_view_rect(content_w, content_h);
  auto resize_res = processor->editor_view->onSize(&local);
  if (!daux_verify_child_client_rect(
          processor->editor_attach_hwnd, content_w, content_h, "onSize")) {
    daux_resize_child_client(processor->editor_attach_hwnd, content_w, content_h);
    resize_res = processor->editor_view->onSize(&local);
    std::fprintf(stderr,
                 "[PluginEditor] onSize retry result=0x%x rect=%d,%d,%d,%d\n",
                 static_cast<unsigned>(resize_res),
                 local.left,
                 local.top,
                 local.right,
                 local.bottom);
  }
  // Repaint the freshly sized child — never leave stale pixels at the edges
  // (spec item 3). FALSE: no background erase, the plugin owns its pixels.
  InvalidateRect(processor->editor_attach_hwnd, nullptr, FALSE);
  static std::atomic<unsigned int> resize_log_count{0};
  const unsigned int count = resize_log_count.fetch_add(1);
  if (count < 12 || count % 50 == 0) {
    std::fprintf(stderr,
                 "[SphereVST3] IPlugView::onSize() result=%d handle=%llu rect=%d,%d,%d,%d count=%u\n",
                 (int)resize_res,
                 processor->editor_handle,
                 (int)local.left,
                 (int)local.top,
                 (int)local.right,
                 (int)local.bottom,
                 count + 1);
  }
  if (daux_resize_log_allow()) {
    RECT after{};
    GetClientRect(processor->editor_attach_hwnd, &after);
    std::fprintf(stderr,
                 "[PluginEditorResize] child_rect=(0,0,%ld,%ld) onSize result=0x%x\n",
                 after.right - after.left,
                 after.bottom - after.top,
                 static_cast<unsigned>(resize_res));
  }
}

void layout_attach_or_fallback(SphereDauxVst3Processor* processor, HWND hwnd) {
  if (!processor || !hwnd) return;
  RECT rc{};
  GetClientRect(hwnd, &rc);
  const int w = rc.right - rc.left;
  const int h = rc.bottom - rc.top;
  if (processor->editor_attach_hwnd && IsWindow(processor->editor_attach_hwnd)) {
    MoveWindow(processor->editor_attach_hwnd, 0, 0, w, h, TRUE);
    resize_editor_view(processor);
  }
  if (processor->editor_fallback_label_hwnd && IsWindow(processor->editor_fallback_label_hwnd)) {
    const int panel_w = 360;
    const int x = std::max(16, (w - panel_w) / 2);
    const int y = std::max(18, (h - 96) / 2);
    MoveWindow(processor->editor_fallback_label_hwnd, x, y, panel_w, 24, TRUE);
    MoveWindow(processor->editor_fallback_reload_hwnd, x, y + 42, 104, 28, TRUE);
    MoveWindow(processor->editor_fallback_generic_hwnd, x + 116, y + 42, 124, 28, TRUE);
    MoveWindow(processor->editor_fallback_close_hwnd, x + 252, y + 42, 82, 28, TRUE);
  }
}

void show_attach_failed_state(SphereDauxVst3Processor* processor, const char* reason) {
  if (!processor || !processor->editor_hwnd) return;
  if (processor->editor_attach_hwnd && IsWindow(processor->editor_attach_hwnd)) {
    paint_dark_child(processor->editor_attach_hwnd);
    DestroyWindow(processor->editor_attach_hwnd);
  }
  processor->editor_attach_hwnd = nullptr;
  processor->editor_view = nullptr;
  processor->editor_attached = false;
  destroy_fallback_controls(processor);

  processor->editor_fallback_label_hwnd = CreateWindowExW(
      0, L"STATIC", L"Editor failed to attach",
      WS_CHILD | WS_VISIBLE | SS_CENTER,
      0, 0, 1, 1, processor->editor_hwnd, nullptr, GetModuleHandleW(nullptr), nullptr);
  processor->editor_fallback_reload_hwnd = CreateWindowExW(
      0, L"BUTTON", L"Reload Editor",
      WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON,
      0, 0, 1, 1, processor->editor_hwnd,
      reinterpret_cast<HMENU>(static_cast<INT_PTR>(kDauxFallbackReloadId)),
      GetModuleHandleW(nullptr), nullptr);
  processor->editor_fallback_generic_hwnd = CreateWindowExW(
      0, L"BUTTON", L"Generic Params",
      WS_CHILD | WS_VISIBLE | WS_DISABLED | BS_PUSHBUTTON,
      0, 0, 1, 1, processor->editor_hwnd,
      reinterpret_cast<HMENU>(static_cast<INT_PTR>(kDauxFallbackGenericId)),
      GetModuleHandleW(nullptr), nullptr);
  processor->editor_fallback_close_hwnd = CreateWindowExW(
      0, L"BUTTON", L"Close",
      WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON,
      0, 0, 1, 1, processor->editor_hwnd,
      reinterpret_cast<HMENU>(static_cast<INT_PTR>(kDauxFallbackCloseId)),
      GetModuleHandleW(nullptr), nullptr);
  layout_attach_or_fallback(processor, processor->editor_hwnd);
  std::fprintf(stderr,
               "[SphereVST3] editor attach failed state shown handle=%llu reason=%s\n",
               processor->editor_handle,
               reason ? reason : "unknown");
}

extern "C" unsigned long long sphere_daux_vst3_open_editor(
    SphereDauxVst3Processor* processor,
    const char*              window_id,
    const char*              title,
    int                      width,
    int                      height);

LRESULT CALLBACK daux_editor_window_proc(HWND hwnd, UINT msg, WPARAM wparam, LPARAM lparam) {
  auto* processor =
      reinterpret_cast<SphereDauxVst3Processor*>(GetWindowLongPtrW(hwnd, GWLP_USERDATA));
  switch (msg) {
    case WM_NCCREATE: {
      auto* create = reinterpret_cast<CREATESTRUCTW*>(lparam);
      processor = reinterpret_cast<SphereDauxVst3Processor*>(create->lpCreateParams);
      SetWindowLongPtrW(hwnd, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(processor));
      return TRUE;
    }
    case WM_SIZE:
      if (processor) layout_attach_or_fallback(processor, hwnd);
      return 0;
    case WM_SETFOCUS:
      if (processor && processor->editor_attach_hwnd) SetFocus(processor->editor_attach_hwnd);
      return 0;
    case WM_CTLCOLORSTATIC: {
      SetTextColor(reinterpret_cast<HDC>(wparam), RGB(231, 237, 245));
      SetBkColor(reinterpret_cast<HDC>(wparam), RGB(11, 15, 20));
      static HBRUSH dark_brush = CreateSolidBrush(RGB(11, 15, 20));
      return reinterpret_cast<LRESULT>(dark_brush);
    }
    case WM_ERASEBKGND: {
      RECT rc{};
      GetClientRect(hwnd, &rc);
      HBRUSH brush = CreateSolidBrush(RGB(11, 15, 20));
      FillRect(reinterpret_cast<HDC>(wparam), &rc, brush);
      DeleteObject(brush);
      return 1;
    }
    case WM_COMMAND:
      if (processor) {
        const int command_id = LOWORD(wparam);
        if (command_id == kDauxFallbackCloseId || command_id == kDauxFallbackGenericId) {
          processor->close_editor_window();
          return 0;
        }
        if (command_id == kDauxFallbackReloadId) {
          const std::string window_id = processor->editor_window_id;
          const std::string title = processor->editor_title;
          const int requested_width = processor->editor_requested_width;
          const int requested_height = processor->editor_requested_height;
          processor->close_editor_window();
          sphere_daux_vst3_open_editor(
              processor,
              window_id.c_str(),
              title.empty() ? "Plugin Editor" : title.c_str(),
              requested_width,
              requested_height);
          return 0;
        }
      }
      break;
    case WM_CLOSE:
      // User pressed the window's X button: destroy the editor shell/view only.
      // Processor and controller stay alive; only insert removal may destroy them.
      if (processor) {
        processor->close_editor_window();
        return 0;
      }
      break;
    case WM_DAUX_DESTROY:
      // Posted by close_editor_window() when called cross-thread; execute the
      // DestroyWindow on this HWND's own thread.
      DestroyWindow(hwnd);
      return 0;
    case WM_DESTROY:
      // GWLP_USERDATA was already zeroed by close_editor_window() before this
      // message was dispatched, so processor may be null here.  Do cleanup only
      // if the pointer is still set (shouldn't normally happen but guard anyway).
      if (processor) {
        detach_editor_view(processor);
        if (processor->editor_attach_hwnd && IsWindow(processor->editor_attach_hwnd)) {
          DestroyWindow(processor->editor_attach_hwnd);
        }
        destroy_fallback_controls(processor);
        processor->editor_attach_hwnd = nullptr;
        processor->editor_hwnd = nullptr;
        processor->editor_handle = 0;
        processor->editor_window_id.clear();
        processor->editor_title.clear();
      }
      return 0;
    default:
      break;
  }
  return DefWindowProcW(hwnd, msg, wparam, lparam);
}

void register_editor_parent_class() {
  static std::once_flag once;
  std::call_once(once, [] {
    WNDCLASSEXW wc{};
    wc.cbSize = sizeof(WNDCLASSEXW);
    wc.lpfnWndProc = daux_editor_window_proc;
    wc.hInstance = GetModuleHandleW(nullptr);
    wc.hCursor = LoadCursorW(nullptr, MAKEINTRESOURCEW(32512));
    wc.hbrBackground = reinterpret_cast<HBRUSH>(GetStockObject(BLACK_BRUSH));
    wc.lpszClassName = kDauxEditorWindowClass;
    RegisterClassExW(&wc);
  });
}

void daux_plugin_browser_runtime_release(SphereDauxVst3Processor* processor);

void SphereDauxVst3Processor::close_editor_window() {
  HWND hwnd = editor_hwnd;
  HWND embed_top = editor_embed_top_hwnd;
  HWND child = editor_attach_hwnd;
  // Zero back-pointer FIRST so any pending messages dispatched after this
  // cannot dereference the (potentially freed) SphereDauxVst3Processor.
  if (hwnd && IsWindow(hwnd)) {
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
  }
  if (embed_top && IsWindow(embed_top)) {
    SetWindowLongPtrW(embed_top, GWLP_USERDATA, 0);
  }
  detach_editor_view(this);
  if (child && IsWindow(child)) {
    DestroyWindow(child);
  }
  if (embed_top && IsWindow(embed_top)) {
    DestroyWindow(embed_top);
  }
  destroy_fallback_controls(this);
  editor_attach_hwnd = nullptr;
  editor_embed_top_hwnd = nullptr;
  editor_parent_hwnd = nullptr;
  embed_mode = false;
  embed_geometry_valid = false;
  embed_content_w = 0;
  embed_content_h = 0;
  editor_hwnd = nullptr;
  editor_handle = 0;
  editor_window_id.clear();
  editor_title.clear();
  editor_requested_width = 0;
  editor_requested_height = 0;
  daux_plugin_browser_runtime_release(this);
  if (hwnd && IsWindow(hwnd)) {
    // Use PostMessage so the destroy is executed on the HWND's owning thread
    // (Electron main thread) rather than potentially a foreign thread.
    // WM_DAUX_DESTROY handler calls DestroyWindow directly.
    const DWORD hwnd_tid = GetWindowThreadProcessId(hwnd, nullptr);
    if (hwnd_tid == GetCurrentThreadId()) {
      DestroyWindow(hwnd);
    } else {
      PostMessageW(hwnd, WM_DAUX_DESTROY, 0, 0);
    }
  }
}

void SphereDauxVst3Processor::close_editor_view_only(const char* reason) {
  // Properly detach the IPlugView (calls removed()) and destroy ONLY the child
  // attach HWND.  The parent shell HWND remains alive so the same editor_handle
  // identity is preserved for the next reopen call.
  //
  // This is the correct hide/close path:
  //   detach view → destroy child HWND → hide parent HWND
  //
  // On next open() we always create a fresh child HWND.  Reusing the stale child
  // HWND after removed() was the root cause of the white-window regression.
  detach_editor_view(this);
  if (editor_attach_hwnd && IsWindow(editor_attach_hwnd)) {
    DestroyWindow(editor_attach_hwnd);
  }
  destroy_fallback_controls(this);
  editor_attach_hwnd = nullptr;
  std::fprintf(stderr,
               "[SphereVST3] close_editor_view_only handle=%llu reason=%s "
               "childHwndDestroyed=1\n",
               editor_handle, reason ? reason : "unknown");
}

// ── GPUI-embedded editor (reuses this processor's controller) ────────────────

bool daux_embed_debug() {
  static const bool enabled = std::getenv("FUTUREBOARD_PLUGIN_VIEW_DEBUG") != nullptr ||
                              std::getenv("FUTUREBOARD_PLUGIN_DEBUG") != nullptr;
  return enabled;
}

// 0 = WS_CHILD embed, 1 = owned tool window, 2 = detached top-level window.
// GPUI's D3D swap chain paints over WS_CHILD hosts, so an owned
// WS_POPUP|WS_EX_TOOLWINDOW overlay is the default. `detached` opts into a
// standalone OS window modeled on the VST3 SDK editorhost sample — a generic
// escape hatch for editors that won't render under the GPUI-composited shell.
// Selected per-run via FUTUREBOARD_PLUGIN_EDITOR_MODE; never plugin-hardcoded.
int daux_embed_resolve_host_kind() {
  const char* mode = std::getenv("FUTUREBOARD_PLUGIN_EDITOR_MODE");
  if (mode && *mode) {
    if (_stricmp(mode, "child") == 0 || _stricmp(mode, "ws_child") == 0) return 0;
    if (_stricmp(mode, "tool") == 0 || _stricmp(mode, "owned") == 0 ||
        _stricmp(mode, "popup") == 0 || _stricmp(mode, "default") == 0 ||
        _stricmp(mode, "embedded") == 0) return 1;
    if (_stricmp(mode, "detached") == 0 || _stricmp(mode, "external") == 0 ||
        _stricmp(mode, "window") == 0) return 2;
  }
  return 1;
}

const char* daux_embed_host_kind_name(int kind) {
  if (kind == 2) return "DetachedNativeWindow";
  return kind == 1 ? "EmbeddedOwnedToolWindow" : "ChildHwndEmbed";
}

const char* daux_embed_selected_mode_label(int kind) {
  const char* mode = std::getenv("FUTUREBOARD_PLUGIN_EDITOR_MODE");
  if (mode && *mode) {
    if (kind == 2) return "detached";
    if (_stricmp(mode, "embedded") == 0) return "embedded";
    if (_stricmp(mode, "child") == 0 || _stricmp(mode, "ws_child") == 0) return "child";
  }
  return kind == 2 ? "detached" : "default";
}

// Initialize COM as STA on the editor (GPUI UI) thread before any IPlugView::
// attached call. UAD Native, Slate, and other CEF/WebView-backed VST3s rely on
// this — without an STA on the thread that owns their parent HWND, the
// embedded Chromium host never finishes initializing and the editor stays
// blank. Idempotent and safe to re-enter: if the thread is already initialized
// to a different apartment we log the HRESULT and keep going (the host will
// still typically attach, just without our hint).
//
// Deliberately NOT paired with `CoUninitialize` — the editor thread keeps STA
// for the editor lifetime. Tearing down COM mid-editor will crash WebView
// hosts.
void daux_embed_ensure_com_initialized() {
  static thread_local HRESULT s_last_hr = static_cast<HRESULT>(0x7FFFFFFF);
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

// ── Generic browser/WebView runtime compatibility layer ──────────────────────
// Many modern VST3 plug-ins render their editor with an embedded browser engine
// (WebView2, CEF/Chromium, JUCE WebBrowserComponent, vendor browser runtimes)
// and ship the runtime DLLs/resources *inside* the .vst3 bundle. The loader DLLs
// resolve dependents from their own directory, so before createView/attached we
// detect the bundled runtime and add ONLY its native dir(s) to the DLL search
// path for the lifetime of the editor — never globally, never permanently.
//
// Detection is keyed off marker files, not vendor names, so this is not UAD-only
// and never touches plug-ins that ship no browser runtime (e.g. FabFilter).

// Mirror of the Rust `PluginEditorRuntimeKind`.
enum class DauxEditorRuntimeKind {
  Native = 0,
  WebView2 = 1,
  Cef = 2,
  Chromium = 3,
  BrowserUnknown = 4,
};

const char* daux_editor_runtime_kind_name(DauxEditorRuntimeKind kind) {
  switch (kind) {
    case DauxEditorRuntimeKind::WebView2: return "WebView2";
    case DauxEditorRuntimeKind::Cef: return "Cef";
    case DauxEditorRuntimeKind::Chromium: return "Chromium";
    case DauxEditorRuntimeKind::BrowserUnknown: return "BrowserUnknown";
    case DauxEditorRuntimeKind::Native:
    default: return "Native";
  }
}

bool daux_plugin_webview_based_debug() {
  static const bool enabled =
      std::getenv("FUTUREBOARD_PLUGIN_WEBVIEW_DEBUG") != nullptr ||
      std::getenv("FUTUREBOARD_UAD_DEBUG") != nullptr;
  return enabled;
}

std::wstring daux_webview_runtime_arch_subdir() {
#if defined(_M_ARM64)
  return L"win-arm64";
#else
  return L"win-x64";
#endif
}

bool daux_path_exists_w(const std::wstring& path) {
  if (path.empty()) return false;
  const DWORD attrs = GetFileAttributesW(path.c_str());
  return attrs != INVALID_FILE_ATTRIBUTES && (attrs & FILE_ATTRIBUTE_DIRECTORY) == 0;
}

bool daux_dir_exists_w(const std::wstring& path) {
  if (path.empty()) return false;
  const DWORD attrs = GetFileAttributesW(path.c_str());
  return attrs != INVALID_FILE_ATTRIBUTES && (attrs & FILE_ATTRIBUTE_DIRECTORY) != 0;
}

std::wstring daux_join_path_w(std::wstring base, const wchar_t* suffix) {
  if (base.empty()) return suffix ? suffix : L"";
  while (!base.empty() && (base.back() == L'\\' || base.back() == L'/')) {
    base.pop_back();
  }
  if (!suffix || !*suffix) return base;
  std::wstring out = std::move(base);
  out.push_back(L'\\');
  out += suffix;
  return out;
}

bool daux_file_in_dir(const std::wstring& dir, const wchar_t* file) {
  return daux_path_exists_w(daux_join_path_w(dir, file));
}

std::string daux_wide_to_utf8(const std::wstring& value) {
  if (value.empty()) return {};
  const int len =
      WideCharToMultiByte(CP_UTF8, 0, value.c_str(), -1, nullptr, 0, nullptr, nullptr);
  if (len <= 1) return {};
  std::string out(static_cast<std::size_t>(len - 1), '\0');
  WideCharToMultiByte(CP_UTF8, 0, value.c_str(), -1, out.data(), len, nullptr, nullptr);
  return out;
}

void daux_push_dir_unique(std::vector<std::wstring>& dirs, const std::wstring& dir) {
  if (dir.empty()) return;
  for (const auto& existing : dirs) {
    if (_wcsicmp(existing.c_str(), dir.c_str()) == 0) return;
  }
  dirs.push_back(dir);
}

// Result of scanning a .vst3 bundle for a bundled browser/WebView runtime.
struct DauxEditorRuntimeDetection {
  DauxEditorRuntimeKind kind = DauxEditorRuntimeKind::Native;
  std::vector<std::wstring> dll_dirs;  // native dirs to add to the DLL search path
  std::wstring webview2_loader;        // WebViewLoader.dll to verify-load (safe)
  std::wstring marker;                 // diagnostic: first marker file found
};

// Scan the bundle directory for known browser/WebView runtime marker files.
// Bounded: probes a fixed list of candidate sub-directories, no recursion.
DauxEditorRuntimeDetection daux_detect_editor_runtime(const std::string& plugin_path) {
  DauxEditorRuntimeDetection out;
  if (plugin_path.empty()) return out;
  const std::wstring root = widen_utf8(plugin_path.c_str());
  const std::wstring arch = daux_webview_runtime_arch_subdir();

  static const wchar_t* kBaseRel[] = {
      L"",  // bundle root
      L"Contents\\Resources",
      L"Contents\\x86_64-win",
      L"Contents\\Resources\\WebView2",
      L"Contents\\Resources\\CEF",
      L"Contents\\Resources\\Chromium",
      L"Contents\\Resources\\Browser",
      L"Contents\\Resources\\runtimes",
      L"Contents\\Resources\\bin",
  };

  bool found_webview2 = false;
  bool found_cef = false;
  bool found_chromium = false;
  bool found_browser = false;

  for (const wchar_t* rel : kBaseRel) {
    const std::wstring base = (*rel) ? daux_join_path_w(root, rel) : root;
    if (!daux_dir_exists_w(base)) continue;

    // WebView2 fixed-version runtime: WebViewLoader.dll may sit directly in the
    // base dir or under runtimes\win-{arch}\native (and the bare win-{arch}\native).
    const std::wstring runtimes_native = daux_join_path_w(
        daux_join_path_w(daux_join_path_w(base, L"runtimes"), arch.c_str()), L"native");
    const std::wstring arch_native =
        daux_join_path_w(daux_join_path_w(base, arch.c_str()), L"native");
    const std::wstring wv2_candidates[] = {base, runtimes_native, arch_native};
    for (const std::wstring& nd : wv2_candidates) {
      if (nd.empty() || !daux_dir_exists_w(nd)) continue;
      const std::wstring loader = daux_join_path_w(nd, L"WebViewLoader.dll");
      if (daux_path_exists_w(loader)) {
        found_webview2 = true;
        daux_push_dir_unique(out.dll_dirs, nd);
        if (out.webview2_loader.empty()) out.webview2_loader = loader;
        if (out.marker.empty()) out.marker = loader;
      }
      if (daux_file_in_dir(nd, L"Microsoft.Web.WebView2.Core.dll")) {
        found_webview2 = true;
        daux_push_dir_unique(out.dll_dirs, nd);
        if (out.marker.empty()) out.marker = daux_join_path_w(nd, L"Microsoft.Web.WebView2.Core.dll");
      }
    }

    // CEF / Chromium markers in the base dir.
    const bool has_libcef = daux_file_in_dir(base, L"libcef.dll");
    const bool has_chrome_elf = daux_file_in_dir(base, L"chrome_elf.dll");
    const bool has_cef_pak = daux_file_in_dir(base, L"cef.pak") ||
                             daux_file_in_dir(base, L"cef_100_percent.pak") ||
                             daux_file_in_dir(base, L"cef_200_percent.pak");
    const bool has_icu = daux_file_in_dir(base, L"icudtl.dat");
    const bool has_v8 = daux_file_in_dir(base, L"snapshot_blob.bin") ||
                        daux_file_in_dir(base, L"v8_context_snapshot.bin");
    const bool has_respak = daux_file_in_dir(base, L"resources.pak");

    if (has_libcef) {
      found_cef = true;
      daux_push_dir_unique(out.dll_dirs, base);
      if (out.marker.empty()) out.marker = daux_join_path_w(base, L"libcef.dll");
    }
    if (has_cef_pak) found_cef = true;
    if (has_chrome_elf) {
      daux_push_dir_unique(out.dll_dirs, base);
      if (!has_libcef && !has_cef_pak) found_chromium = true;
      if (out.marker.empty()) out.marker = daux_join_path_w(base, L"chrome_elf.dll");
    }
    if (has_icu || has_v8 || has_respak) found_browser = true;
  }

  if (found_webview2) out.kind = DauxEditorRuntimeKind::WebView2;
  else if (found_cef) out.kind = DauxEditorRuntimeKind::Cef;
  else if (found_chromium) out.kind = DauxEditorRuntimeKind::Chromium;
  else if (found_browser) out.kind = DauxEditorRuntimeKind::BrowserUnknown;
  else out.kind = DauxEditorRuntimeKind::Native;
  return out;
}

bool daux_plugin_is_browser_based(const std::string& plugin_path) {
  return daux_detect_editor_runtime(plugin_path).kind != DauxEditorRuntimeKind::Native;
}

void daux_webview2_ensure_dll_search_policy() {
  static std::once_flag once;
  std::call_once(once, [] {
    if (!SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_DEFAULT_DIRS |
                                  LOAD_LIBRARY_SEARCH_USER_DIRS)) {
      std::fprintf(
          stderr,
          "[plugin-webview-based] SetDefaultDllDirectories failed err=%lu\n",
          GetLastError());
    } else if (daux_plugin_webview_based_debug()) {
      std::fprintf(stderr, "[plugin-webview-based] SetDefaultDllDirectories ok\n");
    }
  });
}

// Add every detected native runtime dir to the per-process DLL search path and
// (for WebView2 only) verify-load the thin WebViewLoader.dll. CEF/Chromium
// loaders are NOT force-loaded — that would spin up Chromium on the wrong thread;
// the plug-in loads its own once the directory is discoverable.
bool daux_plugin_browser_runtime_prepare(SphereDauxVst3Processor* processor) {
  if (!processor) return false;
  if (!processor->plugin_browser_dll_cookies.empty() || processor->plugin_browser_loader) {
    return true;  // already prepared
  }

  const DauxEditorRuntimeDetection det = daux_detect_editor_runtime(processor->plugin_path);
  processor->plugin_browser_runtime_kind = static_cast<int>(det.kind);
  const bool debug = daux_plugin_webview_based_debug() || daux_embed_debug();

  if (det.kind == DauxEditorRuntimeKind::Native) {
    if (debug) {
      std::fprintf(stderr,
                   "[plugin-webview-based] runtime=Native (no bundled browser runtime) path=%s\n",
                   processor->plugin_path.c_str());
    }
    return true;  // normal native UI plug-in — nothing to do
  }

  if (debug) {
    std::fprintf(stderr,
                 "[plugin-webview-based] runtime=%s marker=%s dll_dirs=%zu path=%s\n",
                 daux_editor_runtime_kind_name(det.kind),
                 daux_wide_to_utf8(det.marker).c_str(),
                 det.dll_dirs.size(),
                 processor->plugin_path.c_str());
  }

  if (det.dll_dirs.empty()) {
    // Browser engine detected by resource files (e.g. *.pak) but no native dir
    // to add — nothing actionable, but not a failure. Editor open continues.
    if (debug) {
      std::fprintf(stderr,
                   "[plugin-webview-based] runtime=%s detected via resources only; no DLL dir to add\n",
                   daux_editor_runtime_kind_name(det.kind));
    }
    return true;
  }

  daux_webview2_ensure_dll_search_policy();

  std::vector<DLL_DIRECTORY_COOKIE> cookies;
  for (const std::wstring& dir : det.dll_dirs) {
    DLL_DIRECTORY_COOKIE cookie = AddDllDirectory(dir.c_str());
    if (!cookie) {
      const DWORD err = GetLastError();
      std::fprintf(stderr, "[plugin-webview-based] AddDllDirectory failed err=%lu\n", err);
      for (DLL_DIRECTORY_COOKIE c : cookies) RemoveDllDirectory(c);
      set_last_error("Failed to configure plugin browser runtime search path (AddDllDirectory err=" +
                     std::to_string(err) + ")");
      return false;
    }
    cookies.push_back(cookie);
    if (debug) {
      std::fprintf(stderr, "[plugin-webview-based] AddDllDirectory ok dir=%s\n",
                   daux_wide_to_utf8(dir).c_str());
    }
  }

  HMODULE loader = nullptr;
  if (!det.webview2_loader.empty()) {
    loader = LoadLibraryW(det.webview2_loader.c_str());
    if (!loader) {
      const DWORD err = GetLastError();
      std::fprintf(stderr, "[plugin-webview-based] LoadLibrary WebViewLoader.dll failed err=%lu\n", err);
      for (DLL_DIRECTORY_COOKIE c : cookies) RemoveDllDirectory(c);
      set_last_error(std::string("Failed to load plugin WebView2 runtime from ") +
                     daux_wide_to_utf8(det.webview2_loader) +
                     " (GetLastError=" + std::to_string(err) + ")");
      return false;
    }
    if (debug) {
      std::fprintf(stderr, "[plugin-webview-based] LoadLibrary WebViewLoader.dll ok\n");
    }
  }

  processor->plugin_browser_dll_cookies = std::move(cookies);
  processor->plugin_browser_loader = loader;
  return true;
}

void daux_plugin_browser_runtime_release(SphereDauxVst3Processor* processor) {
  if (!processor) return;
  if (processor->plugin_browser_loader) {
    FreeLibrary(processor->plugin_browser_loader);
    processor->plugin_browser_loader = nullptr;
  }
  for (DLL_DIRECTORY_COOKIE cookie : processor->plugin_browser_dll_cookies) {
    if (cookie) RemoveDllDirectory(cookie);
  }
  processor->plugin_browser_dll_cookies.clear();
}

bool daux_embed_content_screen_rect(HWND parent, int x, int y, int w, int h, RECT* out) {
  if (!parent || !IsWindow(parent) || !out || w <= 0 || h <= 0) return false;
  POINT tl{x, y};
  POINT br{x + w, y + h};
  if (!ClientToScreen(parent, &tl) || !ClientToScreen(parent, &br)) return false;
  out->left = tl.x; out->top = tl.y; out->right = br.x; out->bottom = br.y;
  return true;
}

void daux_embed_apply_tool_styles(HWND overlay, HWND owner) {
  if (!overlay || !IsWindow(overlay)) return;
  LONG_PTR ex = GetWindowLongPtr(overlay, GWL_EXSTYLE);
  ex &= ~WS_EX_APPWINDOW;
  ex |= WS_EX_TOOLWINDOW;
  SetWindowLongPtr(overlay, GWL_EXSTYLE, ex);
  if (owner && IsWindow(owner)) {
    SetWindowLongPtrW(overlay, GWLP_HWNDPARENT, reinterpret_cast<LONG_PTR>(owner));
  }
}

void daux_embed_raise(HWND host) {
  if (!host || !IsWindow(host)) return;
  EnumChildWindows(
      host,
      [](HWND hwnd, LPARAM) -> BOOL {
        ShowWindow(hwnd, SW_SHOW);
        SetWindowPos(hwnd, HWND_TOP, 0, 0, 0, 0,
                     SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
        return TRUE;
      },
      0);
}

// Native editor top window proc. VST3 editor hosting follows
// public.sdk/samples/vst-hosting/editorhost lifecycle: the host does not paint
// over the editor, resize is forwarded to IPlugView::onSize, and close detaches
// the view. Futureboard adds one dedicated child content HWND; attached() is
// called with that child, never with this top HWND or a GPUI shell HWND.
LRESULT CALLBACK daux_detached_wnd_proc(HWND hwnd, UINT msg, WPARAM wparam, LPARAM lparam) {
  auto* processor = reinterpret_cast<SphereDauxVst3Processor*>(
      static_cast<LONG_PTR>(GetWindowLongPtrW(hwnd, GWLP_USERDATA)));
  const bool live = processor && processor->processor_valid.load(std::memory_order_acquire);
  daux_log_window_message("plugin-top-hwnd", hwnd, msg);
  switch (msg) {
    case WM_ERASEBKGND:
      return 1; // never fill — the plug-in paints the whole client area
    case WM_TIMER:
      if (wparam == kDauxWakeTimerTop || wparam == kDauxWakeTimerContent) {
        KillTimer(hwnd, wparam);
        return 0;
      }
      break; // plugin-installed timer — keep it alive (see content proc note)
    case WM_PAINT: {
      PAINTSTRUCT ps{};
      BeginPaint(hwnd, &ps);
      EndPaint(hwnd, &ps);
      return 0;
    }
    case WM_SIZE:
      if (wparam == SIZE_MINIMIZED) return 0;
      if (live && processor->editor_attach_hwnd && IsWindow(processor->editor_attach_hwnd) &&
          !processor->embed_resize_in_progress) {
        RECT rc{};
        GetClientRect(hwnd, &rc);
        const int content_w = std::max<LONG>(0, rc.right - rc.left);
        const int content_h = std::max<LONG>(0, rc.bottom - rc.top);
        if (content_w <= 0 || content_h <= 0) return 0;
        processor->embed_resize_in_progress = true;
        SetWindowPos(processor->editor_attach_hwnd, nullptr, 0, 0, content_w, content_h,
                     SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW);
        processor->embed_resize_in_progress = false;
        std::fprintf(stderr,
                     "[plugin-view] resize top=(%d,%d) content=(%d,%d)\n",
                     content_w, content_h, content_w, content_h);
        if (processor->editor_attached) resize_editor_view(processor);
      }
      return 0;
    case WM_GETMINMAXINFO:
      // Fixed-size contract for the user-resizable detached window (spec
      // items 1/8): lock min = max = current outer size so dragging edges
      // can never open blank/garbage area. Programmatic resizes (plugin
      // resizeView) run under embed_resize_in_progress and stay exempt.
      if (live && processor->embed_host_kind == 2 && processor->editor_attached &&
          processor->editor_view && !processor->embed_resize_in_progress &&
          !daux_editor_view_resizable(processor) && lparam) {
        RECT wr{};
        if (GetWindowRect(hwnd, &wr)) {
          auto* mmi = reinterpret_cast<MINMAXINFO*>(lparam);
          const POINT size{wr.right - wr.left, wr.bottom - wr.top};
          mmi->ptMinTrackSize = size;
          mmi->ptMaxTrackSize = size;
          mmi->ptMaxSize = size;
          return 0;
        }
      }
      break;
    case WM_SIZING:
      // Resizable contract for the detached window (spec item 2): constrain
      // the in-drag rect through checkSizeConstraint so the user can only
      // reach sizes the plugin accepts.
      if (live && processor->embed_host_kind == 2 && processor->editor_attached &&
          processor->editor_view && lparam && daux_editor_view_resizable(processor)) {
        RECT* drag = reinterpret_cast<RECT*>(lparam);
        RECT frame{0, 0, 0, 0};
        const DWORD style = static_cast<DWORD>(GetWindowLongPtrW(hwnd, GWL_STYLE));
        const DWORD ex_style = static_cast<DWORD>(GetWindowLongPtrW(hwnd, GWL_EXSTYLE));
        if (!daux_adjust_window_rect_for_dpi(hwnd, &frame, style, ex_style)) {
          AdjustWindowRectEx(&frame, style, FALSE, ex_style);
        }
        const int nc_w = static_cast<int>((frame.right - frame.left));
        const int nc_h = static_cast<int>((frame.bottom - frame.top));
        int content_w = static_cast<int>(drag->right - drag->left) - nc_w;
        int content_h = static_cast<int>(drag->bottom - drag->top) - nc_h;
        if (content_w > 0 && content_h > 0 &&
            daux_constrain_content_size(processor, &content_w, &content_h)) {
          const int outer_w = content_w + nc_w;
          const int outer_h = content_h + nc_h;
          // Keep the anchored edges: move only the side(s) being dragged.
          switch (wparam) {
            case WMSZ_LEFT:
            case WMSZ_TOPLEFT:
            case WMSZ_BOTTOMLEFT:
              drag->left = drag->right - outer_w;
              break;
            default:
              drag->right = drag->left + outer_w;
              break;
          }
          switch (wparam) {
            case WMSZ_TOP:
            case WMSZ_TOPLEFT:
            case WMSZ_TOPRIGHT:
              drag->top = drag->bottom - outer_h;
              break;
            default:
              drag->bottom = drag->top + outer_h;
              break;
          }
          return TRUE;
        }
      }
      break;
    case WM_DPICHANGED: {
      if (!live || !lparam) break;
      const RECT* const suggested = reinterpret_cast<RECT*>(lparam);
      SetWindowPos(hwnd,
                   nullptr,
                   suggested->left,
                   suggested->top,
                   suggested->right - suggested->left,
                   suggested->bottom - suggested->top,
                   SWP_NOZORDER | SWP_NOACTIVATE);
      daux_log_editor_dpi(hwnd, "WM_DPICHANGED");
      if (processor->editor_attach_hwnd && IsWindow(processor->editor_attach_hwnd) &&
          processor->editor_attached) {
        RECT rc{};
        GetClientRect(processor->editor_attach_hwnd, &rc);
        const int content_w = static_cast<int>(rc.right - rc.left);
        const int content_h = static_cast<int>(rc.bottom - rc.top);
        if (content_w > 0 && content_h > 0) {
          daux_resize_child_client(processor->editor_attach_hwnd, content_w, content_h);
          auto local = daux_local_view_rect(content_w, content_h);
          const auto on_size_res = processor->editor_view->onSize(&local);
          std::fprintf(stderr,
                       "[PluginEditor] WM_DPICHANGED onSize result=0x%x client=%dx%d\n",
                       static_cast<unsigned>(on_size_res),
                       content_w,
                       content_h);
        }
      }
      return 0;
    }
    case WM_CLOSE:
      if (live) {
        processor->embed_user_closed.store(true, std::memory_order_release);
      }
      ShowWindow(hwnd, SW_HIDE); // Rust drains the flag and removes the shell
      return 0;
    case WM_MOUSEACTIVATE:
      // Never refuse activation for plugin content — clicks must reach the
      // plugin child / dialog tree, not bounce to the wrapper or DAW surfaces.
      return MA_ACTIVATE;
    default:
      break;
  }
  return DefWindowProcW(hwnd, msg, wparam, lparam);
}

void register_detached_editor_class() {
  static std::once_flag once;
  std::call_once(once, [] {
    WNDCLASSEXW wc{};
    wc.cbSize = sizeof(WNDCLASSEXW);
    wc.lpfnWndProc = daux_detached_wnd_proc;
    wc.hInstance = GetModuleHandleW(nullptr);
    wc.hCursor = LoadCursorW(nullptr, MAKEINTRESOURCEW(32512));
    wc.hbrBackground = nullptr; // editorhost: no background — plug-in owns paint
    wc.lpszClassName = kDauxEditorDetachedClass;
    RegisterClassExW(&wc);
  });
}

HWND daux_embed_create_content_child(HWND top, int w, int h) {
  if (!top || !IsWindow(top)) return nullptr;
  // WS_EX_NOPARENTNOTIFY: never send synchronous WM_PARENTNOTIFY up a
  // cross-process parent chain (main-app shell) on child create/destroy/click.
  return CreateWindowExW(
      WS_EX_NOPARENTNOTIFY,
      kDauxEditorContentClass,
      L"",
      WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
      0, 0, w > 0 ? w : 640, h > 0 ? h : 480,
      top, nullptr, GetModuleHandleW(nullptr), nullptr);
}

// Create the native top editor HWND. The dedicated child content HWND is created
// separately and is the only HWND passed to IPlugView::attached().
HWND daux_embed_create_top(HWND parent, int kind, int x, int y, int w, int h) {
  register_detached_editor_class();
  DWORD style = WS_CLIPCHILDREN | WS_CLIPSIBLINGS;
  // WS_EX_NOPARENTNOTIFY: in embedded (WS_CHILD) mode the parent is the
  // main-app content HWND in another process; WM_PARENTNOTIFY would be a
  // synchronous cross-process send. Harmless for owned/top-level kinds.
  DWORD ex_style = WS_EX_NOPARENTNOTIFY;
  HWND hwnd_parent = nullptr;
  if (kind == 2) {
    style |= WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX |
             WS_THICKFRAME | WS_MAXIMIZEBOX;
    ex_style |= WS_EX_APPWINDOW;
  } else if (kind == 1) {
    style |= WS_POPUP;
    ex_style |= WS_EX_TOOLWINDOW;
    hwnd_parent = parent; // owner
  } else {
    style |= WS_CHILD | WS_VISIBLE;
    hwnd_parent = parent;
  }
  RECT rect{0, 0, w > 0 ? w : 640, h > 0 ? h : 480};
  HWND dpi_ref = nullptr;
  if (parent && IsWindow(parent)) {
    dpi_ref = parent;
  }
  if (dpi_ref) {
    const UINT dpi = daux_hwnd_dpi(dpi_ref);
    if (!AdjustWindowRectExForDpi(&rect, style, FALSE, ex_style, dpi)) {
      AdjustWindowRectEx(&rect, style, FALSE, ex_style);
    }
  } else {
    AdjustWindowRectEx(&rect, style, FALSE, ex_style);
  }
  int px = x, py = y;
  if (kind == 2) {
    px = CW_USEDEFAULT;
    py = CW_USEDEFAULT;
  }
  if (kind == 1 && parent && IsWindow(parent)) {
    RECT screen{};
    if (daux_embed_content_screen_rect(parent, x, y, w, h, &screen)) {
      px = screen.left;
      py = screen.top;
    }
  } else if (kind == 2 && parent && IsWindow(parent)) {
    RECT pr{};
    if (GetWindowRect(parent, &pr)) {
      px = pr.left + 48;
      py = pr.top + 48;
    }
  }
  HWND top = CreateWindowExW(
      ex_style, kDauxEditorDetachedClass, L"Plugin Editor", style,
      px, py, rect.right - rect.left, rect.bottom - rect.top,
      hwnd_parent, nullptr, GetModuleHandleW(nullptr), nullptr);
  if (top) {
    set_daux_dark_titlebar(top);
    if (kind == 1) daux_embed_apply_tool_styles(top, parent);
  }
  return top;
}

// Reposition/resize the host window to the requested region. Returns true only
// when the applied rect actually changed, so idle frames do no SetWindowPos /
// onSize / raise work (mirrors the SpherePluginHost anti-flicker fix).
bool daux_embed_sync_geometry(SphereDauxVst3Processor* p, int x, int y, int w, int h,
                              bool log_reposition) {
  HWND top = p ? p->editor_embed_top_hwnd : nullptr;
  if (!p || !top || !IsWindow(top)) return false;
  // Detached: a standalone OS window owns its own position/size (user-movable,
  // resizeView-driven). Never snap it to the GPUI parent region.
  if (p->embed_host_kind == 2) return false;
  p->embed_host_x = x; p->embed_host_y = y; p->embed_host_w = w; p->embed_host_h = h;
  if (p->embed_host_kind == 1 && p->editor_parent_hwnd) {
    if (!IsWindow(p->editor_parent_hwnd)) return false;
    const bool parent_visible =
        IsWindowVisible(p->editor_parent_hwnd) && !IsIconic(p->editor_parent_hwnd);
    ShowWindow(top, parent_visible ? SW_SHOWNA : SW_HIDE);
    RECT screen{};
    if (!daux_embed_content_screen_rect(p->editor_parent_hwnd, x, y, w, h, &screen)) return false;
    if (p->embed_geometry_valid && EqualRect(&screen, &p->embed_last_applied)) return false;
    p->embed_last_applied = screen;
    p->embed_geometry_valid = true;
    SetWindowPos(top, p->editor_parent_hwnd,
                 screen.left, screen.top,
                 screen.right - screen.left, screen.bottom - screen.top,
                 SWP_NOACTIVATE | SWP_SHOWWINDOW);
    daux_embed_apply_tool_styles(top, p->editor_parent_hwnd);
  } else {
    RECT want{x, y, x + w, y + h};
    if (p->embed_geometry_valid && EqualRect(&want, &p->embed_last_applied)) return false;
    p->embed_last_applied = want;
    p->embed_geometry_valid = true;
    SetWindowPos(top, HWND_TOP, x, y, w, h, SWP_SHOWWINDOW | SWP_NOACTIVATE);
  }
  EnableWindow(top, TRUE);
  if (p->editor_attach_hwnd && IsWindow(p->editor_attach_hwnd)) {
    RECT rc{};
    GetClientRect(top, &rc);
    SetWindowPos(p->editor_attach_hwnd, nullptr, 0, 0, rc.right - rc.left, rc.bottom - rc.top,
                 SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW);
  }
  daux_embed_raise(top);
  if (log_reposition && daux_embed_debug()) {
    std::fprintf(stderr,
                 "[plugin-view] daux reposition top=0x%p content=0x%p mode=%s\n",
                 static_cast<void*>(top),
                 static_cast<void*>(p->editor_attach_hwnd),
                 daux_embed_host_kind_name(p->embed_host_kind));
  }
  return true;
}

bool daux_embed_apply_content_size(SphereDauxVst3Processor* p,
                                   int content_w,
                                   int content_h,
                                   const char* reason) {
  if (!p || content_w <= 0 || content_h <= 0) return false;
  if (p->embed_resize_in_progress) return false;

  const bool debug = daux_embed_debug() || daux_vst3_editor_debug();

  // Detached: size the standalone top-level window so its CLIENT area equals the
  // plug-in's preferred size (editorhost pattern). Position is left to the user.
  if (p->embed_host_kind == 2) {
    bool changed = false;
    HWND top = p->editor_embed_top_hwnd;
    if (top && IsWindow(top)) {
      RECT wr{0, 0, content_w, content_h};
      const DWORD style = static_cast<DWORD>(GetWindowLongPtrW(top, GWL_STYLE));
      const DWORD ex_style = static_cast<DWORD>(GetWindowLongPtrW(top, GWL_EXSTYLE));
      if (!daux_adjust_window_rect_for_dpi(top, &wr, style, ex_style)) {
        AdjustWindowRectEx(&wr, style, FALSE, ex_style);
      }
      const int win_w = wr.right - wr.left;
      const int win_h = wr.bottom - wr.top;
      RECT cur{};
      GetWindowRect(top, &cur);
      if ((cur.right - cur.left) != win_w || (cur.bottom - cur.top) != win_h) {
        p->embed_resize_in_progress = true;
        SetWindowPos(top, nullptr, 0, 0, win_w, win_h,
                     SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
        if (p->editor_attach_hwnd && IsWindow(p->editor_attach_hwnd)) {
          SetWindowPos(p->editor_attach_hwnd, nullptr, 0, 0, content_w, content_h,
                       SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
        p->embed_resize_in_progress = false;
        changed = true;
      }
    }
    p->embed_content_w = content_w;
    p->embed_content_h = content_h;
    if (debug) {
      std::fprintf(stderr,
                   "[plugin-view] auto_size mode=detached plugin=%dx%d reason=%s changed=%d\n",
                   content_w, content_h, reason ? reason : "unknown", changed ? 1 : 0);
    }
    return changed;
  }

  const int header_h = std::max(0, p->embed_host_y);
  const int shell_client_w = std::max(content_w, p->embed_host_x + content_w);
  const int shell_client_h = header_h + content_h;
  bool changed = false;

  p->embed_resize_in_progress = true;

  // Main-owned bridge shell: parent is the shell content HWND — resize via IPC.
  if (p->embed_mode && p->editor_parent_hwnd && IsWindow(p->editor_parent_hwnd) &&
      !p->editor_hwnd) {
    p->pending_main_shell_w = content_w;
    p->pending_main_shell_h = content_h;
    p->pending_main_shell_resize.store(true, std::memory_order_release);
  } else if (p->embed_mode && p->editor_parent_hwnd && IsWindow(p->editor_parent_hwnd)) {
    RECT client{};
    GetClientRect(p->editor_parent_hwnd, &client);
    const int current_w = client.right - client.left;
    const int current_h = client.bottom - client.top;
    if (current_w != shell_client_w || current_h != shell_client_h) {
      SetWindowPos(p->editor_parent_hwnd,
                   nullptr,
                   0,
                   0,
                   shell_client_w,
                   shell_client_h,
                   SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
      changed = true;
    }
  } else if (p->editor_hwnd && IsWindow(p->editor_hwnd)) {
    RECT wr{0, 0, content_w, content_h};
    const DWORD style = static_cast<DWORD>(GetWindowLongPtrW(p->editor_hwnd, GWL_STYLE));
    const DWORD ex_style = static_cast<DWORD>(GetWindowLongPtrW(p->editor_hwnd, GWL_EXSTYLE));
    if (!daux_adjust_window_rect_for_dpi(p->editor_hwnd, &wr, style, ex_style)) {
      AdjustWindowRectEx(&wr, style, FALSE, ex_style);
    }
    const int shell_w = wr.right - wr.left;
    const int shell_h = wr.bottom - wr.top;
    RECT win{};
    GetWindowRect(p->editor_hwnd, &win);
    if ((win.right - win.left) != shell_w || (win.bottom - win.top) != shell_h) {
      SetWindowPos(p->editor_hwnd,
                   nullptr,
                   0,
                   0,
                   shell_w,
                   shell_h,
                   SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
      changed = true;
    }
  }

  if (p->editor_embed_top_hwnd && IsWindow(p->editor_embed_top_hwnd)) {
    const bool host_changed =
        daux_embed_sync_geometry(p, 0, header_h, content_w, content_h, false);
    changed = changed || host_changed;
  }

  p->embed_content_w = content_w;
  p->embed_content_h = content_h;
  p->embed_resize_in_progress = false;

  if (p->pending_main_shell_resize.load(std::memory_order_acquire)) {
    const UINT dpi =
        (p->editor_parent_hwnd && IsWindow(p->editor_parent_hwnd))
            ? GetDpiForWindow(p->editor_parent_hwnd)
            : 96;
    std::fprintf(
        stderr,
        "[PluginEditor] resizeView requested instance=%s size=%dx%d dpi=%u reason=%s\n",
        p->embed_instance_label.empty() ? "<unknown>" : p->embed_instance_label.c_str(),
        content_w,
        content_h,
        dpi,
        reason ? reason : "unknown");
    std::fprintf(stderr,
                 "[PluginEditor] container resized client=%dx%d\n",
                 content_w,
                 content_h);
  }

  if (debug) {
    std::fprintf(stderr,
                 "[plugin-view] auto_size plugin=%dx%d shell=%dx%d content=(0,%d,%d,%d) reason=%s changed=%d\n",
                 content_w,
                 content_h,
                 shell_client_w,
                 shell_client_h,
                 header_h,
                 content_w,
                 content_h,
                 reason ? reason : "unknown",
                 changed ? 1 : 0);
  }
  return changed;
}

bool daux_embed_has_visible_ui(SphereDauxVst3Processor* p) {
  if (!p || !p->editor_attach_hwnd || !IsWindow(p->editor_attach_hwnd)) return false;
  if (!IsWindowVisible(p->editor_attach_hwnd)) return false;
  RECT cr{};
  GetClientRect(p->editor_attach_hwnd, &cr);
  if (cr.right - cr.left < 4 || cr.bottom - cr.top < 4) return false;
  struct Ctx { int visible = 0; } ctx{};
  EnumChildWindows(
      p->editor_attach_hwnd,
      [](HWND hwnd, LPARAM lp) -> BOOL {
        if (!IsWindowVisible(hwnd)) return TRUE;
        RECT r{};
        GetWindowRect(hwnd, &r);
        if (r.right > r.left && r.bottom > r.top) reinterpret_cast<Ctx*>(lp)->visible++;
        return TRUE;
      },
      reinterpret_cast<LPARAM>(&ctx));
  if (ctx.visible > 0) return true;
  if (p->editor_view) {
    Steinberg::ViewRect sz{};
    const auto gs = p->editor_view->getSize(&sz);
    if (gs == Steinberg::kResultTrue || gs == Steinberg::kResultOk) {
      if (sz.right - sz.left > 16 && sz.bottom - sz.top > 16) return true;
    }
  }
  return false;
}

void SphereDauxVst3Processor::close_embed_editor(const char* reason) {
  detach_editor_view(this); // IPlugView::removed(); keeps controller/processor
  if (editor_attach_hwnd && IsWindow(editor_attach_hwnd)) {
    DestroyWindow(editor_attach_hwnd);
  }
  if (editor_embed_top_hwnd && IsWindow(editor_embed_top_hwnd)) {
    // Stop the top WndProc from touching this processor during teardown.
    SetWindowLongPtrW(editor_embed_top_hwnd, GWLP_USERDATA, 0);
    DestroyWindow(editor_embed_top_hwnd);
  }
  embed_user_closed.store(false, std::memory_order_release);
  editor_attach_hwnd = nullptr;
  editor_embed_top_hwnd = nullptr;
  editor_parent_hwnd = nullptr;
  embed_mode = false;
  embed_geometry_valid = false;
  embed_content_w = 0;
  embed_content_h = 0;
  embed_resize_in_progress = false;
  editor_handle = 0;
  daux_plugin_browser_runtime_release(this);
  std::fprintf(stderr,
               "[SphereVST3] close_embed_editor reason=%s (processor kept alive)\n",
               reason ? reason : "unknown");
}
#endif

// ── C API ─────────────────────────────────────────────────────────────────────

extern "C" int sphere_daux_vst3_bridge_probe(void) {
  std::fprintf(stderr, "[DAUx VST3] bridge probe ok\n");
  std::fflush(stderr);
  return 0xDA03;
}

extern "C" const char* sphere_daux_vst3_last_error(void) {
  return g_last_error.c_str();
}

extern "C" SphereDauxVst3Processor* sphere_daux_vst3_create(
    const char* plugin_path,
    const char* class_id,
    double      sample_rate) {
  set_last_error("");
  if (!plugin_path || !*plugin_path) {
    set_last_error("empty plugin_path");
    return nullptr;
  }

  std::fprintf(stderr, "[DAUx VST3] create entered: path=%s classId=%s\n",
               plugin_path, class_id ? class_id : "");
#if defined(_WIN32)
  std::fprintf(stderr,
               "[process] role=plugin_host mode=in_process pid=%lu tid=%lu\n",
               GetCurrentProcessId(),
               GetCurrentThreadId());
#endif
  std::fflush(stderr);

  auto instance = std::make_unique<SphereDauxVst3Processor>();
  std::fprintf(stderr, "[DAUx VST3] instance created: path=%s classId=%s\n",
               plugin_path, class_id ? class_id : "");
  std::string error;
  instance->module = VST3::Hosting::Module::create(plugin_path, error);
  if (!instance->module) {
    set_last_error("module load failed: " + error);
    std::fprintf(stderr, "[DAUx VST3] module load FAILED: %s\n", error.c_str());
    return nullptr;
  }
  std::fprintf(stderr, "[DAUx VST3] plugin loaded: %s\n", plugin_path);

  const auto factory = instance->module->getFactory();
  factory.setHostContext(&instance->host_context);
  log_factory_classes(factory);
  {
    std::ostringstream classes;
    int index = 0;
    for (const auto& info : factory.classInfos()) {
      if (index > 0) classes << " | ";
      classes << "[" << index << "] name='" << info.name() << "' category='"
              << info.category() << "' uid=" << info.ID().toString();
      ++index;
    }
    if (index == 0) set_last_error("factory has no classes");
    else set_last_error("factory classes: " + classes.str());
  }

  const std::string requested = class_id ? class_id : "";
  VST3::Optional<VST3::UID> uid;
  if (!looks_like_zero_class_id(requested))
    uid = VST3::UID::fromString(requested);
  if (!uid) {
    std::fprintf(stderr,
                 "[DAUx VST3] classId missing/zero/invalid; trying first Audio Module Class fallback\n");
    uid = first_audio_module_uid(factory);
  }
  if (!uid) {
    set_last_error(g_last_error + "; no Audio Module Class found");
    std::fprintf(stderr, "[DAUx VST3] no Audio Module Class found in factory\n");
    return nullptr;
  }

  instance->component = factory.createInstance<Steinberg::Vst::IComponent>(*uid);
  if (!instance->component) {
    std::fprintf(stderr,
                 "[DAUx VST3] create IComponent FAILED for classId=%s; trying first Audio Module Class fallback\n",
                 requested.c_str());
    uid = first_audio_module_uid(factory);
    if (uid) instance->component = factory.createInstance<Steinberg::Vst::IComponent>(*uid);
    if (!instance->component) {
      set_last_error(g_last_error + "; create IComponent failed for requested classId='" + requested + "' and fallback");
      std::fprintf(stderr, "[DAUx VST3] create IComponent FAILED after fallback\n");
      return nullptr;
    }
  }
  std::fprintf(stderr, "[DAUx VST3] component created classId=%s\n", uid->toString().c_str());
  if (auto pb = Steinberg::FUnknownPtr<Steinberg::IPluginBase>(instance->component)) {
    if (pb->initialize(&instance->host_context) != Steinberg::kResultOk) {
      set_last_error(g_last_error + "; component initialize failed");
      std::fprintf(stderr, "[DAUx VST3] component initialize FAILED\n");
      return nullptr;
    }
    std::fprintf(stderr, "[SphereVST3] component initialized result=0 ok\n");
  } else {
    set_last_error(g_last_error + "; component does not implement IPluginBase");
    std::fprintf(stderr, "[DAUx VST3] component does not implement IPluginBase\n");
    return nullptr;
  }

  if (instance->component->queryInterface(
          Steinberg::Vst::IAudioProcessor::iid,
          reinterpret_cast<void**>(&instance->processor)) != Steinberg::kResultTrue ||
      !instance->processor) {
    set_last_error(g_last_error + "; component does not implement IAudioProcessor");
    std::fprintf(stderr, "[DAUx VST3] component does not implement IAudioProcessor\n");
    return nullptr;
  }
  std::fprintf(stderr, "[SphereVST3] processor found\n");

  // Obtain IEditController (either from the component itself or a separate class)
  Steinberg::Vst::IEditController* raw_ctrl = nullptr;
  if (instance->component->queryInterface(
          Steinberg::Vst::IEditController::iid,
          reinterpret_cast<void**>(&raw_ctrl)) == Steinberg::kResultTrue) {
    instance->controller = Steinberg::IPtr<Steinberg::Vst::IEditController>::adopt(raw_ctrl);
    instance->controller_is_component = true;
    std::fprintf(stderr, "[SphereVST3] controller initialized result=0 ok component-owned\n");
  } else {
    Steinberg::TUID ctrl_cid{};
    if (instance->component->getControllerClassId(ctrl_cid) == Steinberg::kResultTrue) {
      instance->controller =
          factory.createInstance<Steinberg::Vst::IEditController>(VST3::UID(ctrl_cid));
      if (instance->controller) {
        if (auto pb = Steinberg::FUnknownPtr<Steinberg::IPluginBase>(instance->controller)) {
          if (pb->initialize(&instance->host_context) != Steinberg::kResultOk) {
            std::fprintf(stderr, "[DAUx VST3] controller initialize FAILED\n");
            instance->controller = nullptr;
          } else {
            std::fprintf(stderr, "[SphereVST3] controller initialized result=0 ok\n");
          }
        }
      }
    }
  }

  // Connect component ↔ controller
  if (instance->controller) {
    instance->component_connection =
        Steinberg::FUnknownPtr<Steinberg::Vst::IConnectionPoint>(instance->component);
    instance->controller_connection =
        Steinberg::FUnknownPtr<Steinberg::Vst::IConnectionPoint>(instance->controller);
    if (instance->component_connection && instance->controller_connection) {
      instance->component_connection->connect(instance->controller_connection);
      instance->controller_connection->connect(instance->component_connection);
      std::fprintf(stderr, "[DAUx VST3] component/controller connected\n");
    }
  }

  if (!instance->setup(sample_rate)) {
    set_last_error(g_last_error + "; setup failed");
    instance->shutdown();
    return nullptr;
  }

  instance->plugin_path = plugin_path ? plugin_path : "";
  set_last_error("");
  std::fprintf(stderr, "[DAUx VST3] processor ready: %s handle=0x%p\n",
               plugin_path, static_cast<void*>(instance.get()));
  return instance.release();
}

extern "C" void sphere_daux_vst3_destroy(SphereDauxVst3Processor* processor) {
  if (!processor) return;
  std::fprintf(stderr,
               "[SphereVST3] destroying processor handle=0x%p\n",
               static_cast<void*>(processor));
#if defined(_WIN32)
  // Mark invalid BEFORE zeroing GWLP_USERDATA so the window proc can check
  // this flag even if it races between the zero and a pending message.
  processor->processor_valid.store(false, std::memory_order_seq_cst);
  // Zero the back-pointer so any still-pending WM_TIMER/WM_PAINT cannot
  // dereference the struct after it is freed.
  if (processor->editor_hwnd && IsWindow(processor->editor_hwnd)) {
    SetWindowLongPtrW(processor->editor_hwnd, GWLP_USERDATA, 0);
  }
#elif defined(__APPLE__) || defined(__linux__)
  processor->processor_valid.store(false, std::memory_order_seq_cst);
#endif
  processor->shutdown();
  delete processor;
}

extern "C" int sphere_daux_vst3_process_stereo_sample(
    SphereDauxVst3Processor* processor,
    float in_l, float in_r,
    float* out_l, float* out_r) {
  if (!processor || !processor->processor || !out_l || !out_r) return 0;

  processor->prepare_process_io(nullptr, 0);

  // Fill input, clear output
  processor->input_l  = in_l;
  processor->input_r  = in_r;
  processor->output_l = 0.f;
  processor->output_r = 0.f;

  const auto result = processor->processor->process(processor->process_data);

  processor->last_input_peak = std::max(
      std::abs(static_cast<double>(in_l)),
      std::abs(static_cast<double>(in_r)));
  processor->last_output_peak = std::max(
      std::abs(static_cast<double>(processor->output_l)),
      std::abs(static_cast<double>(processor->output_r)));
  processor->last_difference_peak = std::max(
      std::abs(static_cast<double>(processor->output_l - in_l)),
      std::abs(static_cast<double>(processor->output_r - in_r)));

  // First-process debug log (fires once, outside the hot path thereafter)
  if (!processor->first_process_done) {
    processor->first_process_done = true;
    std::fprintf(stderr,
                 "[SphereVST3] first process %s inputPeakL=%.6f outputPeakL=%.6f diffPeak=%.6f\n",
                 result == Steinberg::kResultOk ? "ok" : "failed",
                 processor->last_input_peak,
                 processor->last_output_peak,
                 processor->last_difference_peak);
  }

  if (result != Steinberg::kResultOk) return 0;

  processor->process_count += 1;

  *out_l = processor->output_l;
  *out_r = processor->output_r;
  return 1;
}

extern "C" int sphere_daux_vst3_process_stereo_block(
    SphereDauxVst3Processor* processor,
    const float* in_l,
    const float* in_r,
    float* out_l,
    float* out_r,
    int frames) {
  return sphere_daux_vst3_process_stereo_block_with_midi(
      processor, in_l, in_r, out_l, out_r, frames, nullptr, 0);
}

extern "C" int sphere_daux_vst3_process_stereo_block_with_midi(
    SphereDauxVst3Processor* processor,
    const float* in_l,
    const float* in_r,
    float* out_l,
    float* out_r,
    int frames,
    const SphereDauxVst3MidiEvent* events,
    int event_count) {
  if (!processor || !processor->processor || !in_l || !in_r || !out_l || !out_r || frames <= 0) {
    return 0;
  }

  processor->prepare_process_io(events, event_count);

  float* input_channels[2] = {
      const_cast<float*>(in_l),
      const_cast<float*>(in_r),
  };
  float* output_channels[2] = {out_l, out_r};
  processor->input_bus.numChannels = 2;
  processor->input_bus.channelBuffers32 = input_channels;
  processor->output_bus.numChannels = 2;
  processor->output_bus.channelBuffers32 = output_channels;
  processor->process_data.numSamples = frames;
  processor->process_data.inputs = &processor->input_bus;
  processor->process_data.outputs = &processor->output_bus;

  const auto result = processor->processor->process(processor->process_data);

  if (daux_vst3_midi_debug() && event_count > 0) {
    std::fprintf(stderr,
                 "[vst3-process] frames=%d midi_events=%d result=%d\n",
                 frames,
                 event_count,
                 (int)result);
  }

  double input_peak_l = 0.0;
  double input_peak_r = 0.0;
  double output_peak_l = 0.0;
  double output_peak_r = 0.0;
  double diff_peak = 0.0;
  for (int i = 0; i < frames; ++i) {
    input_peak_l = std::max(input_peak_l, std::abs(static_cast<double>(in_l[i])));
    input_peak_r = std::max(input_peak_r, std::abs(static_cast<double>(in_r[i])));
    output_peak_l = std::max(output_peak_l, std::abs(static_cast<double>(out_l[i])));
    output_peak_r = std::max(output_peak_r, std::abs(static_cast<double>(out_r[i])));
    diff_peak = std::max(diff_peak, std::abs(static_cast<double>(out_l[i] - in_l[i])));
    diff_peak = std::max(diff_peak, std::abs(static_cast<double>(out_r[i] - in_r[i])));
  }
  processor->last_input_peak = std::max(input_peak_l, input_peak_r);
  processor->last_output_peak = std::max(output_peak_l, output_peak_r);
  processor->last_difference_peak = diff_peak;

  if (!processor->first_process_done) {
    processor->first_process_done = true;
    std::fprintf(stderr,
                 "[SphereVST3] first process %s frames=%d inputPeakL=%.6f outputPeakL=%.6f diffPeak=%.6f\n",
                 result == Steinberg::kResultOk ? "ok" : "failed",
                 frames,
                 input_peak_l,
                 output_peak_l,
                 processor->last_difference_peak);
  }

  processor->process_data.numSamples = 1;
  processor->process_data.inputEvents = nullptr;
  processor->input_bus.channelBuffers32 = processor->input_channels;
  processor->output_bus.channelBuffers32 = processor->output_channels;

  if (result != Steinberg::kResultOk) return 0;
  processor->process_count += 1;
  return 1;
}

extern "C" int sphere_daux_vst3_event_input_bus_count(
    SphereDauxVst3Processor* processor) {
  if (!processor) return 0;
  return processor->event_input_bus_count;
}

/// Enqueue a normalized parameter change for delivery on the next process call.
/// Safe to call from any thread (audio thread or UI thread).
extern "C" void sphere_daux_vst3_set_param(
    SphereDauxVst3Processor* processor,
    unsigned int             param_id,
    double                   value) {
  if (!processor) return;
  processor->enqueue_param(
      static_cast<Steinberg::Vst::ParamID>(param_id),
      static_cast<Steinberg::Vst::ParamValue>(value));
}

extern "C" unsigned long long sphere_daux_vst3_open_editor(
    SphereDauxVst3Processor* processor,
    const char*              window_id,
    const char*              title,
    int                      width,
    int                      height) {
  if (!processor || !processor->controller) {
    std::fprintf(stderr,
                 "[SphereVST3] editor open failed processor=%p controller=%p exists=%d\n",
                 static_cast<void*>(processor),
                 processor ? static_cast<void*>(processor->controller.get()) : nullptr,
                 processor && processor->controller ? 1 : 0);
    return 0;
  }
#if defined(__APPLE__)
  return open_editor_mac(processor, window_id, title, width, height);
#elif defined(__linux__)
  return open_editor_linux(processor, window_id, title, width, height);
#elif !defined(_WIN32)
  (void)window_id;
  (void)title;
  (void)width;
  (void)height;
  set_last_error("DAUx VST3 editor: unsupported platform");
  return 0;
#else
  if (processor->editor_hwnd && IsWindow(processor->editor_hwnd)) {
    ShowWindow(processor->editor_hwnd, SW_SHOWNORMAL);
    UpdateWindow(processor->editor_hwnd);
    SetForegroundWindow(processor->editor_hwnd);
    if (processor->editor_attach_hwnd) SetFocus(processor->editor_attach_hwnd);
    std::fprintf(stderr,
                 "[SphereVST3] editor already open; focused existing shell handle=%llu windowId=%s\n",
                 processor->editor_handle,
                 processor->editor_window_id.c_str());
    return processor->editor_handle;

    // Parent shell still alive (was hidden after user-close or programmatic-close).
    // Always create a FRESH child HWND + fresh IPlugView — never reuse the stale
    // child HWND.  After IPlugView::removed() the child HWND has no vendor content;
    // re-attaching to it yields a white/blank window (the root cause).
    std::fprintf(stderr,
                 "[SphereVST3] editor reopen: creating fresh child HWND + IPlugView "
                 "handle=%llu windowId=%s\n",
                 processor->editor_handle,
                 processor->editor_window_id.c_str());

    // Measure the current client area of the parent shell to match size.
    RECT rc{};
    GetClientRect(processor->editor_hwnd, &rc);
    const int w = (rc.right - rc.left > 0) ? (rc.right - rc.left) : (width > 0 ? width : 820);
    const int h = (rc.bottom - rc.top > 0) ? (rc.bottom - rc.top) : (height > 0 ? height : 560);

    // Create fresh child attach HWND.
    HWND child = CreateWindowExW(
        0,
        kDauxEditorContentClass,
        L"",
        WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
        0, 0, w, h,
        processor->editor_hwnd,
        nullptr,
        GetModuleHandleW(nullptr),
        nullptr);
    if (!child) {
      std::fprintf(stderr,
                   "[SphereVST3] editor reopen: CreateWindowExW child FAILED handle=%llu\n",
                   processor->editor_handle);
      return 0;
    }
    std::fprintf(stderr,
                 "[SphereVST3] editor reopen: fresh child HWND=0x%p handle=%llu\n",
                 static_cast<void*>(child), processor->editor_handle);
    processor->editor_attach_hwnd = child;

    // Create fresh IPlugView.
    processor->editor_view = Steinberg::IPtr<Steinberg::IPlugView>::adopt(
        processor->controller->createView(Steinberg::Vst::ViewType::kEditor));
    std::fprintf(stderr,
                 "[SphereVST3] editor reopen: createView %s handle=%llu\n",
                 processor->editor_view ? "ok" : "FAILED",
                 processor->editor_handle);
    if (!processor->editor_view) {
      DestroyWindow(child);
      processor->editor_attach_hwnd = nullptr;
      return 0;
    }
    if (processor->editor_view->isPlatformTypeSupported(Steinberg::kPlatformTypeHWND) !=
        Steinberg::kResultTrue) {
      processor->editor_view = nullptr;
      DestroyWindow(child);
      processor->editor_attach_hwnd = nullptr;
      std::fprintf(stderr,
                   "[SphereVST3] editor reopen: HWND not supported handle=%llu\n",
                   processor->editor_handle);
      return 0;
    }

    // Set the frame BEFORE attached() — required by WebView/CEF editors.
    daux_editor_install_frame(processor);

    // Attach to fresh child HWND.
    const auto attach_res = processor->editor_view->attached(
        reinterpret_cast<void*>(child), Steinberg::kPlatformTypeHWND);
    std::fprintf(stderr,
                 "[SphereVST3] editor reopen: IPlugView::attached() result=%d handle=%llu\n",
                 (int)attach_res, processor->editor_handle);
    if (attach_res != Steinberg::kResultTrue && attach_res != Steinberg::kResultOk) {
      daux_editor_clear_frame(processor);
      processor->editor_view = nullptr;
      DestroyWindow(child);
      processor->editor_attach_hwnd = nullptr;
      return 0;
    }
    processor->editor_attached = true;

    // Update window title (latency/CPU may change between opens).
    if (title && *title) {
      SetWindowTextW(processor->editor_hwnd, widen_utf8(title).c_str());
      std::fprintf(stderr,
                   "[SphereVST3] editor reopen: title='%s' handle=%llu\n",
                   title, processor->editor_handle);
    }

    resize_editor_view(processor);
    ShowWindow(processor->editor_hwnd, SW_SHOWNORMAL);
    UpdateWindow(processor->editor_hwnd);
    SetForegroundWindow(processor->editor_hwnd);
    SetWindowPos(processor->editor_hwnd, HWND_TOPMOST, 0, 0, 0, 0,
                 SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    std::fprintf(stderr,
                 "[SphereVST3] editor reopen: complete handle=%llu "
                 "mainHWND=0x%p attachHWND=0x%p\n",
                 processor->editor_handle,
                 static_cast<void*>(processor->editor_hwnd),
                 static_cast<void*>(child));
    return processor->editor_handle;
  }

  daux_ensure_thread_dpi_awareness();
  register_editor_window_classes();
  register_editor_parent_class();

  processor->editor_window_id = window_id ? window_id : "";
  processor->editor_title = title && *title ? title : "Plugin Editor";
  processor->editor_requested_width = width;
  processor->editor_requested_height = height;
  const std::string plugin_instance_id = processor->editor_window_id;
  const auto identity_colon = plugin_instance_id.find_last_of(':');
  const char* identity =
      identity_colon == std::string::npos ? plugin_instance_id.c_str()
                                          : plugin_instance_id.c_str() + identity_colon + 1;
  std::fprintf(stderr,
               "[SphereVST3] editor open request pluginInstanceId=%s windowId=%s controller=%p exists=%d\n",
               identity,
               processor->editor_window_id.c_str(),
               static_cast<void*>(processor->controller.get()),
               processor->controller ? 1 : 0);

  if (!daux_plugin_browser_runtime_prepare(processor)) {
    return 0;
  }

  processor->editor_view = Steinberg::IPtr<Steinberg::IPlugView>::adopt(
      processor->controller->createView(Steinberg::Vst::ViewType::kEditor));
  std::fprintf(stderr, "[PluginEditor] create view ok\n");
  std::fprintf(stderr,
               "[SphereVST3] IPlugView createView pluginInstanceId=%s ptr=%p exists=%d\n",
               identity,
               static_cast<void*>(processor->editor_view.get()),
               processor->editor_view ? 1 : 0);
  if (!processor->editor_view) {
    daux_plugin_browser_runtime_release(processor);
    if (daux_plugin_is_browser_based(processor->plugin_path)) {
      set_last_error("Browser/WebView-based plugin editor createView failed (controller returned null view)");
    } else {
      set_last_error("controller did not create editor view");
    }
    return 0;
  }
  if (processor->editor_view->isPlatformTypeSupported(Steinberg::kPlatformTypeHWND) !=
      Steinberg::kResultTrue) {
    processor->editor_view = nullptr;
    set_last_error("editor view does not support HWND");
    return 0;
  }

  Steinberg::ViewRect preferred{};
  int editor_width = width > 0 ? width : 820;
  int editor_height = height > 0 ? height : 560;
  const auto get_size_result = processor->editor_view->getSize(&preferred);
  std::fprintf(stderr,
               "[SphereVST3] IPlugView::getSize() result=%d rect=%d,%d,%d,%d pluginInstanceId=%s\n",
               (int)get_size_result,
               (int)preferred.left,
               (int)preferred.top,
               (int)preferred.right,
               (int)preferred.bottom,
               identity);
  if (get_size_result == Steinberg::kResultTrue) {
    const int preferred_width = preferred.right - preferred.left;
    const int preferred_height = preferred.bottom - preferred.top;
    if (preferred_width > 0) editor_width = preferred_width;
    if (preferred_height > 0) editor_height = preferred_height;
  }
  std::fprintf(stderr,
               "[PluginEditor] getSize rect=%d,%d,%d,%d\n",
               (int)preferred.left,
               (int)preferred.top,
               (int)preferred.right,
               (int)preferred.bottom);
  std::fprintf(stderr,
               "[PluginEditor] view_get_size=%dx%d client_size=%dx%d\n",
               editor_width,
               editor_height,
               editor_width,
               editor_height);

  RECT rect{0, 0, editor_width, editor_height};
  const DWORD outer_style = WS_OVERLAPPEDWINDOW;
  const DWORD outer_ex_style = WS_EX_TOPMOST;
  if (!AdjustWindowRectExForDpi(&rect, outer_style, FALSE, outer_ex_style, 96)) {
    AdjustWindowRectEx(&rect, outer_style, FALSE, outer_ex_style);
  }
  const auto wide_title = widen_utf8(title && *title ? title : "Plugin Editor");
  HWND hwnd = CreateWindowExW(
      WS_EX_TOPMOST,
      kDauxEditorWindowClass,
      wide_title.c_str(),
      WS_OVERLAPPEDWINDOW,
      CW_USEDEFAULT,
      CW_USEDEFAULT,
      rect.right - rect.left,
      rect.bottom - rect.top,
      nullptr,
      nullptr,
      GetModuleHandleW(nullptr),
      processor);
  if (!hwnd) {
    processor->editor_view = nullptr;
    set_last_error("CreateWindowExW failed for DAUx VST3 editor");
    return 0;
  }
  set_daux_dark_titlebar(hwnd);
  const UINT dpi = daux_hwnd_dpi(hwnd);
  daux_log_editor_dpi(hwnd, "created top");
  RECT outer_rect{0, 0, editor_width, editor_height};
  if (!AdjustWindowRectExForDpi(&outer_rect, outer_style, FALSE, outer_ex_style, dpi)) {
    AdjustWindowRectEx(&outer_rect, outer_style, FALSE, outer_ex_style);
  }
  const int outer_w = outer_rect.right - outer_rect.left;
  const int outer_h = outer_rect.bottom - outer_rect.top;
  std::fprintf(stderr,
               "[PluginEditor] created top hwnd=0x%p dpi=%u\n",
               static_cast<void*>(hwnd),
               dpi);
  std::fprintf(stderr,
               "[PluginEditor] outer_size=%dx%d\n",
               outer_w,
               outer_h);
  SetWindowPos(hwnd,
               HWND_TOPMOST,
               0,
               0,
               outer_w,
               outer_h,
               SWP_NOMOVE | SWP_NOACTIVATE);

  HWND child = CreateWindowExW(
      0,
      kDauxEditorContentClass,
      L"",
      WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
      0,
      0,
      editor_width,
      editor_height,
      hwnd,
      nullptr,
      GetModuleHandleW(nullptr),
      nullptr);
  if (!child) {
    DestroyWindow(hwnd);
    processor->editor_view = nullptr;
    set_last_error("CreateWindowExW failed for DAUx VST3 attach HWND");
    return 0;
  }

  processor->editor_hwnd = hwnd;
  processor->editor_attach_hwnd = child;
  processor->editor_handle = g_next_editor_handle.fetch_add(1);
  std::fprintf(stderr,
               "[PluginEditor] created child hwnd=0x%p client=%dx%d\n",
               static_cast<void*>(child),
               editor_width,
               editor_height);
  std::fprintf(stderr,
               "[SphereVST3] editor HWNDs pluginInstanceId=%s handle=%llu mainHWND=0x%p childHWND=0x%p\n",
               identity,
               processor->editor_handle,
               static_cast<void*>(hwnd),
               static_cast<void*>(child));

  // Set the frame BEFORE attached() — required by WebView/CEF editors.
  daux_editor_install_frame(processor);
  std::fprintf(stderr, "[PluginEditor] setFrame ok\n");

  const auto attach_result =
      processor->editor_view->attached(reinterpret_cast<void*>(child), Steinberg::kPlatformTypeHWND);
  std::fprintf(stderr,
               "[PluginEditor] attached result=0x%x\n",
               static_cast<unsigned>(attach_result));
  std::fprintf(stderr,
               "[SphereVST3] IPlugView::attached(child HWND) result=%d handle=%llu childHWND=0x%p pluginInstanceId=%s\n",
               (int)attach_result,
               processor->editor_handle,
               static_cast<void*>(child),
               identity);
  if (attach_result != Steinberg::kResultTrue && attach_result != Steinberg::kResultOk) {
    set_last_error("IPlugView::attached(HWND) failed for DAUx VST3 editor");
    show_attach_failed_state(processor, "attached-failed");
    ShowWindow(hwnd, SW_SHOWNORMAL);
    UpdateWindow(hwnd);
    SetForegroundWindow(hwnd);
    return processor->editor_handle;
  }

  processor->editor_attached = true;
  {
    auto local = daux_local_view_rect(editor_width, editor_height);
    const auto on_size_res = processor->editor_view->onSize(&local);
    std::fprintf(stderr,
                 "[PluginEditor] onSize result=0x%x\n",
                 static_cast<unsigned>(on_size_res));
    daux_verify_child_client_rect(child, editor_width, editor_height, "after attach");
  }
  ShowWindow(hwnd, SW_SHOWNORMAL);
  UpdateWindow(hwnd);
  SetForegroundWindow(hwnd);
  std::fprintf(stderr, "[PluginEditor] visible=true\n");
  std::fprintf(stderr,
               "[SphereVST3] editor opened same-instance handle=%llu windowId=%s mainHWND=0x%p attachHWND=0x%p\n",
               processor->editor_handle,
               processor->editor_window_id.c_str(),
               static_cast<void*>(hwnd),
               static_cast<void*>(child));
  return processor->editor_handle;
#endif
}

extern "C" void sphere_daux_vst3_close_editor(SphereDauxVst3Processor* processor) {
  if (!processor) return;
#if defined(_WIN32)
  // Detach IPlugView and destroy the native editor shell.
  // Processor and controller are kept alive — only insert removal may destroy them.
  if (processor->editor_hwnd && IsWindow(processor->editor_hwnd)) {
    const unsigned long long handle = processor->editor_handle;
    const std::string window_id = processor->editor_window_id;
    processor->close_editor_window();
    std::fprintf(stderr,
                 "[SphereVST3] editor closed (programmatic) handle=%llu windowId=%s\n",
                 handle,
                 window_id.c_str());
  }
#elif defined(__APPLE__)
  close_editor_mac(processor);
#elif defined(__linux__)
  close_editor_linux(processor);
#endif
}

extern "C" int sphere_daux_vst3_focus_editor(SphereDauxVst3Processor* processor) {
  if (!processor) return 0;
#if defined(_WIN32)
  if (processor->editor_hwnd && IsWindow(processor->editor_hwnd)) {
    ShowWindow(processor->editor_hwnd, SW_SHOWNORMAL);
    SetForegroundWindow(processor->editor_hwnd);
    if (processor->editor_attach_hwnd) SetFocus(processor->editor_attach_hwnd);
    return 1;
  }
#elif defined(__APPLE__)
  return focus_editor_mac(processor);
#elif defined(__linux__)
  return focus_editor_linux(processor);
#endif
  return 0;
}

// ── GPUI-embedded editor C ABI ───────────────────────────────────────────────
// These attach the EXISTING runtime instance's editor view (built from
// processor->controller) into a GPUI-provided parent HWND. They never create a
// new component/controller — GUI parameter edits flow through the same
// ComponentHandlerImpl that feeds the realtime processor.

extern "C" unsigned long long sphere_daux_vst3_embed_editor(
    SphereDauxVst3Processor* processor,
    unsigned long long       parent_hwnd,
    int x, int y, int width, int height) {
#ifdef _WIN32
  // Phase 4: ensure COM (STA) is live on the editor thread before any
  // IPlugView call. Some VST3 editors — notably UAD Native / Chromium-backed
  // hosts — never finish initializing their WebView without an STA on the
  // thread that owns the parent HWND. Idempotent / no-op when already
  // initialized.
  daux_embed_ensure_com_initialized();
  daux_ensure_thread_dpi_awareness();

  if (!processor || !processor->controller) {
    std::fprintf(stderr, "[vst3-editor] attach failed error=processor/controller missing\n");
    set_last_error("embed editor: processor/controller missing");
    return 0;
  }
  HWND parent = reinterpret_cast<HWND>(static_cast<std::uintptr_t>(parent_hwnd));
  if (!parent || !IsWindow(parent) || width <= 0 || height <= 0) {
    std::fprintf(
        stderr,
        "[vst3-editor] attach failed error=invalid parent HWND or region parent=0x%p w=%d h=%d\n",
        static_cast<void*>(parent), width, height);
    set_last_error("embed editor: invalid parent HWND or region");
    return 0;
  }
  std::fprintf(stderr, "[plugin-view] sdk_reference=editorhost\n");
  std::fprintf(stderr,
               "[process] role=editor_host mode=in_process pid=%lu tid=%lu\n",
               GetCurrentProcessId(),
               GetCurrentThreadId());
  std::fprintf(stderr, "[plugin-view] ui_thread_id=%lu\n", GetCurrentThreadId());
  std::fprintf(stderr, "[plugin-view] platform_type=HWND\n");
  std::fprintf(
      stderr,
      "[vst3-editor] attach begin parent=0x%p platform=HWND region=(%d,%d,%d,%d) tid=%lu\n",
      static_cast<void*>(parent), x, y, width, height, GetCurrentThreadId());

  // Reuse: if this instance already has an embedded editor attached, just
  // re-sync geometry and return the existing handle — never re-create.
  if (processor->embed_mode && processor->editor_attached &&
      processor->editor_embed_top_hwnd && IsWindow(processor->editor_embed_top_hwnd) &&
      processor->editor_attach_hwnd && IsWindow(processor->editor_attach_hwnd)) {
    processor->editor_parent_hwnd = parent;
    processor->embed_geometry_valid = false; // force re-apply against new parent
    daux_embed_sync_geometry(processor, x, y, width, height, daux_embed_debug());
    std::fprintf(stderr,
                 "[SphereVST3] embed editor reuse handle=%llu (existing instance/view)\n",
                 processor->editor_handle);
    return processor->editor_handle;
  }

  const int kind = daux_embed_resolve_host_kind();
  register_editor_window_classes();
  daux_log_editor_dpi(parent, "attach parent");

  std::fprintf(stderr, "[vst3-editor] create_tid=%lu\n", GetCurrentThreadId());
  if (!processor->editor_view) {
    if (!daux_plugin_browser_runtime_prepare(processor)) {
      return 0;
    }
    processor->editor_view = Steinberg::IPtr<Steinberg::IPlugView>::adopt(
        processor->controller->createView(Steinberg::Vst::ViewType::kEditor));
  } else {
    std::fprintf(stderr, "[vst3-editor] createView reuse prepared view ptr=0x%p\n",
                 static_cast<void*>(processor->editor_view.get()));
  }
  std::fprintf(stderr, "[PluginEditor] create view ok\n");
  std::fprintf(
      stderr,
      "[vst3-editor] createView result=%s ptr=0x%p\n",
      processor->editor_view ? "ok" : "null",
      static_cast<void*>(processor->editor_view.get()));
  if (!processor->editor_view) {
    daux_plugin_browser_runtime_release(processor);
    if (daux_plugin_is_browser_based(processor->plugin_path)) {
      set_last_error("Browser/WebView-based plugin editor createView failed (controller returned null view)");
    } else {
      set_last_error("embed editor: controller did not create view");
    }
    return 0;
  }
  if (processor->editor_view->isPlatformTypeSupported(Steinberg::kPlatformTypeHWND) !=
      Steinberg::kResultTrue) {
    processor->editor_view = nullptr;
    daux_plugin_browser_runtime_release(processor);
    std::fprintf(stderr, "[vst3-editor] attach failed error=view does not support HWND\n");
    set_last_error("embed editor: view does not support HWND");
    return 0;
  }

  Steinberg::ViewRect preferred{};
  const auto get_size_result = processor->editor_view->getSize(&preferred);
  int preferred_w = 0;
  int preferred_h = 0;
  if (get_size_result == Steinberg::kResultTrue || get_size_result == Steinberg::kResultOk) {
    preferred_w = daux_view_rect_width(preferred);
    preferred_h = daux_view_rect_height(preferred);
  }
  // IPlugView::getSize is the content/client size source of truth.
  int editor_w =
      (preferred_w > 0) ? preferred_w : (width > 0 ? width : 640);
  int editor_h =
      (preferred_h > 0) ? preferred_h : (height > 0 ? height : 480);
  const UINT dpi = daux_hwnd_dpi(parent);
  daux_log_editor_dpi(parent, "embed");
  std::fprintf(stderr,
               "[PluginEditor] getSize rect=%d,%d,%d,%d\n",
               preferred.left,
               preferred.top,
               preferred.right,
               preferred.bottom);
  std::fprintf(stderr,
               "[PluginEditor] view_get_size=%dx%d client_size=%dx%d\n",
               editor_w,
               editor_h,
               editor_w,
               editor_h);
  std::fprintf(stderr,
               "[PluginEditor] view_size=%dx%d client_rect=%dx%d dpi=%u host_region=%dx%d\n",
               preferred_w,
               preferred_h,
               editor_w,
               editor_h,
               dpi,
               width,
               height);
  std::fprintf(
      stderr,
      "[vst3-editor] getSize result=0x%x width=%d height=%d rect=(%d,%d,%d,%d)\n",
      static_cast<unsigned>(get_size_result),
      editor_w,
      editor_h,
      preferred.left,
      preferred.top,
      preferred.right,
      preferred.bottom);

  HWND top = daux_embed_create_top(parent, kind, x, y, editor_w, editor_h);
  if (!top) {
    set_last_error("embed editor: top window creation failed");
    return 0;
  }
  processor->embed_user_closed.store(false, std::memory_order_release);
  SetWindowLongPtrW(top, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(processor));
  HWND content = daux_embed_create_content_child(top, editor_w, editor_h);
  if (!content) {
    DestroyWindow(top);
    set_last_error("embed editor: content child HWND creation failed");
    return 0;
  }
  const HWND content_parent = GetParent(content);
  const bool content_is_child = content_parent == top;
  SetTimer(top, kDauxWakeTimerTop, 250, nullptr);
  SetTimer(content, kDauxWakeTimerContent, 250, nullptr);
  std::fprintf(stderr, "[plugin-view] selected_host_mode=%s\n", daux_embed_selected_mode_label(kind));
  std::fprintf(stderr, "[plugin-view] top_hwnd=0x%p\n", static_cast<void*>(top));
  std::fprintf(stderr, "[plugin-view] content_hwnd=0x%p\n", static_cast<void*>(content));
  std::fprintf(stderr, "[plugin-view] content_is_child=%s\n", content_is_child ? "true" : "false");
  std::fprintf(stderr, "[plugin-view] content_parent=0x%p\n", static_cast<void*>(content_parent));
  if (content == top || !content_is_child) {
    std::fprintf(stderr,
                 "[plugin-view] ERROR invalid HWND hierarchy top=0x%p content=0x%p parent=0x%p\n",
                 static_cast<void*>(top),
                 static_cast<void*>(content),
                 static_cast<void*>(content_parent));
    DestroyWindow(content);
    DestroyWindow(top);
    set_last_error("embed editor: content HWND must be a child and must differ from top HWND");
    return 0;
  }
  std::fprintf(stderr,
               "[PluginEditor] created top hwnd=0x%p dpi=%u\n",
               static_cast<void*>(top),
               daux_hwnd_dpi(top));
  std::fprintf(stderr,
               "[PluginEditor] created child hwnd=0x%p client=%dx%d\n",
               static_cast<void*>(content),
               editor_w,
               editor_h);
  daux_log_hwnd_state("created", top, content);

  // Publish the host HWND and set the frame BEFORE attached() so the editor's
  // synchronous resizeView() calls (common for WebView/CEF editors) land on a
  // valid window. Cleared on the failure path below.
  processor->editor_embed_top_hwnd = top;
  processor->editor_attach_hwnd = content;
  processor->editor_parent_hwnd = parent;
  processor->embed_host_kind = kind;
  processor->embed_mode = true;
  processor->embed_host_x = x;
  processor->embed_host_y = y;
  processor->embed_host_w = width;
  processor->embed_host_h = height;
  daux_embed_apply_content_size(processor, editor_w, editor_h, "createView.getSize");
  daux_resize_child_client(content, editor_w, editor_h);
  daux_editor_install_frame(processor);
  std::fprintf(stderr, "[PluginEditor] setFrame ok\n");
  daux_log_hwnd_state("sized_before_attach", top, content);

  const ULONGLONG attach_start_ms = GetTickCount64();
  auto attach_returned = std::make_shared<std::atomic<bool>>(false);
  auto attach_watchdog = attach_returned;
  std::thread([attach_watchdog, attach_start_ms, content] {
    std::this_thread::sleep_for(std::chrono::seconds(5));
    if (!attach_watchdog->load(std::memory_order_acquire)) {
      std::fprintf(stderr,
                   "[vst3-editor] attached still_blocked elapsed_ms=%llu parent=0x%p watchdog_tid=%lu\n",
                   static_cast<unsigned long long>(GetTickCount64() - attach_start_ms),
                   static_cast<void*>(content),
                   GetCurrentThreadId());
    }
  }).detach();
  std::fprintf(stderr,
               "[vst3-editor] attached begin parent=0x%p attach_tid=%lu start_ms=%llu\n",
               static_cast<void*>(content),
               GetCurrentThreadId(),
               static_cast<unsigned long long>(attach_start_ms));
  const auto attach_res =
      processor->editor_view->attached(reinterpret_cast<void*>(content), Steinberg::kPlatformTypeHWND);
  attach_returned->store(true, std::memory_order_release);
  const ULONGLONG attach_end_ms = GetTickCount64();
  std::fprintf(stderr,
               "[PluginEditor] attached result=0x%x\n",
               static_cast<unsigned>(attach_res));
  std::fprintf(
      stderr,
      "[vst3-editor] attached result=0x%x content=0x%p attach_tid=%lu end_ms=%llu elapsed_ms=%llu\n",
      static_cast<unsigned>(attach_res),
      static_cast<void*>(content),
      GetCurrentThreadId(),
      static_cast<unsigned long long>(attach_end_ms),
      static_cast<unsigned long long>(attach_end_ms - attach_start_ms));
  if (attach_res != Steinberg::kResultTrue && attach_res != Steinberg::kResultOk) {
    daux_editor_clear_frame(processor);
    processor->editor_view = nullptr;
    processor->editor_attach_hwnd = nullptr;
    processor->editor_embed_top_hwnd = nullptr;
    processor->editor_parent_hwnd = nullptr;
    processor->embed_mode = false;
    processor->embed_geometry_valid = false;
    processor->embed_content_w = 0;
    processor->embed_content_h = 0;
    daux_plugin_browser_runtime_release(processor);
    DestroyWindow(content);
    DestroyWindow(top);
    if (daux_plugin_is_browser_based(processor->plugin_path)) {
      char msg[160];
      std::snprintf(msg, sizeof(msg),
                    "Browser/WebView-based plugin editor createView failed (IPlugView::attached returned 0x%x)",
                    static_cast<unsigned>(attach_res));
      set_last_error(msg);
    } else {
      set_last_error("embed editor: IPlugView::attached(HWND) failed");
    }
    return 0;
  }

  processor->editor_attached = true;
  processor->editor_embed_top_hwnd = top;
  processor->editor_attach_hwnd = content;
  processor->editor_parent_hwnd = parent;
  processor->editor_hwnd = nullptr; // embed mode: no daux-owned shell
  processor->embed_host_kind = kind;
  processor->embed_mode = true;
  processor->embed_geometry_valid = false;
  processor->editor_handle = g_next_editor_handle.fetch_add(1);
  // Editor resize contract (spec item 1): query canResize once per view, after
  // attach (some editors only report it reliably once attached). The flag is
  // forwarded to the main app via EditorAttached so the wrapper can lock its
  // size for fixed-size editors.
  daux_editor_view_resizable(processor);

  daux_embed_apply_content_size(processor, editor_w, editor_h, "attached");
  {
    Steinberg::ViewRect after_attach_size{};
    const auto after_get_size_result = processor->editor_view->getSize(&after_attach_size);
    const int after_w = daux_view_rect_width(after_attach_size);
    const int after_h = daux_view_rect_height(after_attach_size);
    const int size_w = after_w > 0 ? after_w : editor_w;
    const int size_h = after_h > 0 ? after_h : editor_h;
    daux_resize_child_client(content, size_w, size_h);
    std::fprintf(
        stderr,
        "[vst3-editor] getSize after_attach result=0x%x width=%d height=%d rect=(%d,%d,%d,%d)\n",
        static_cast<unsigned>(after_get_size_result),
        size_w,
        size_h,
        after_attach_size.left,
        after_attach_size.top,
        after_attach_size.right,
        after_attach_size.bottom);
    editor_w = size_w;
    editor_h = size_h;
  }
  {
    auto local = daux_local_view_rect(editor_w, editor_h);
    const auto on_size_res = processor->editor_view->onSize(&local);
    std::fprintf(stderr,
                 "[PluginEditor] onSize result=0x%x\n",
                 static_cast<unsigned>(on_size_res));
    std::fprintf(
        stderr,
        "[vst3-editor] onSize result=0x%x rect=(%d,%d,%d,%d)\n",
        static_cast<unsigned>(on_size_res),
        local.left,
        local.top,
        local.right,
        local.bottom);
    if (!daux_verify_child_client_rect(content, editor_w, editor_h, "after attach")) {
      daux_resize_child_client(content, editor_w, editor_h);
      processor->editor_view->onSize(&local);
    }
  }
  daux_log_hwnd_state("after_attach_onSize", top, content);
  if (kind == 1) daux_embed_apply_tool_styles(top, parent);

  ShowWindow(top, kind == 2 ? SW_SHOWNORMAL : SW_SHOWNA);
  ShowWindow(content, SW_SHOW);
  UpdateWindow(top);
  UpdateWindow(content);
  std::fprintf(stderr, "[PluginEditor] visible=true\n");
  {
    RECT crc{};
    GetWindowRect(content, &crc);
    std::fprintf(stderr,
                 "[PluginEditorHWND] wrapper=0x%p child=0x%p child_enabled=%d "
                 "child_visible=%d child_style=0x%Ix child_ex_style=0x%Ix\n",
                 static_cast<void*>(top),
                 static_cast<void*>(content),
                 IsWindowEnabled(content) ? 1 : 0,
                 IsWindowVisible(content) ? 1 : 0,
                 static_cast<std::uintptr_t>(GetWindowLongPtrW(content, GWL_STYLE)),
                 static_cast<std::uintptr_t>(GetWindowLongPtrW(content, GWL_EXSTYLE)));
    std::fprintf(stderr,
                 "[PluginEditorHWND] child_rect=(%ld,%ld,%ld,%ld)\n",
                 crc.left, crc.top, crc.right, crc.bottom);
  }

  // Phase 2: enumerate plug-in-created child windows. For Chromium/CEF/WebView
  // editors (UAD Native, Slate, some iZotope) the host HWND will commonly have
  // ZERO children at this point because the WebView is still booting on an
  // internal helper thread. The delayed-ready poller in Rust re-checks at
  // 100/500/1000/3000/5000 ms.
  {
    int child_count = 0;
    EnumChildWindows(
        content,
        [](HWND hwnd, LPARAM lp) -> BOOL {
          char cls[64] = {0};
          GetClassNameA(hwnd, cls, sizeof(cls));
          RECT r{};
          GetWindowRect(hwnd, &r);
          DWORD tid = 0;
          GetWindowThreadProcessId(hwnd, &tid);
          const LONG_PTR style = GetWindowLongPtr(hwnd, GWL_STYLE);
          std::fprintf(
              stderr,
              "[vst3-editor]   child hwnd=0x%p class='%s' visible=%d rect=(%ld,%ld %ldx%ld) "
              "style=0x%08lx tid=%lu\n",
              static_cast<void*>(hwnd),
              cls,
              IsWindowVisible(hwnd) ? 1 : 0,
              r.left, r.top, r.right - r.left, r.bottom - r.top,
              static_cast<unsigned long>(style),
              tid);
          (*reinterpret_cast<int*>(lp))++;
          return TRUE;
        },
        reinterpret_cast<LPARAM>(&child_count));
    std::fprintf(
        stderr,
        "[vst3-editor] EnumChildWindows count=%d content=0x%p\n",
        child_count,
        static_cast<void*>(content));
  }

  // Phase 5: post-attach paint hygiene — repaint the host + any plug-in
  // children. WebView plug-ins sometimes need an explicit invalidate before
  // their first frame goes on screen.
  ShowWindow(top, kind == 2 ? SW_SHOWNORMAL : SW_SHOWNA);
  ShowWindow(content, SW_SHOW);
  InvalidateRect(content, nullptr, TRUE);
  UpdateWindow(content);
  EnumChildWindows(
      content,
      [](HWND hwnd, LPARAM) -> BOOL {
        if (!IsWindow(hwnd)) return TRUE;
        InvalidateRect(hwnd, nullptr, TRUE);
        UpdateWindow(hwnd);
        return TRUE;
      },
      0);

  if (!IsWindowVisible(content) || !daux_embed_has_visible_ui(processor)) {
    std::fprintf(stderr,
                 "[SphereVST3] embed editor attached but no visible UI yet handle=%llu mode=%s "
                 "(deferring to delayed-ready poller)\n",
                 processor->editor_handle, daux_embed_host_kind_name(kind));
    // Leave it attached — Rust will poll embed_has_visible_ui at 100/500/1000/
    // 3000/5000 ms (Phase 6) before declaring the editor blank.
  }

  std::fprintf(stderr,
               "[SphereVST3] embed editor ok handle=%llu mode=%s parent=0x%p content=0x%p "
               "region=(%d,%d,%d,%d) (reused runtime instance)\n",
               processor->editor_handle,
               daux_embed_host_kind_name(kind),
               static_cast<void*>(parent),
               static_cast<void*>(content),
               x, y, width, height);
  return processor->editor_handle;
#else
  (void)processor; (void)parent_hwnd; (void)x; (void)y; (void)width; (void)height;
  return 0;
#endif
}

extern "C" void sphere_daux_vst3_embed_set_bounds(
    SphereDauxVst3Processor* processor, int x, int y, int width, int height) {
#ifdef _WIN32
  if (!processor || !processor->embed_mode) return;
  if (width <= 0 || height <= 0) return;
  if (daux_resize_log_allow()) {
    std::fprintf(stderr,
                 "[PluginEditorResize] wrapper_client=%dx%d origin=(%d,%d)\n",
                 width, height, x, y);
  }
  // Generic VST3 resize contract (spec items 1/2): never hand the plugin a
  // size it did not agree to. Fixed-size views snap back to their getSize;
  // resizable views go through checkSizeConstraint. `width`/`height` arrive
  // as the wrapper's CONTENT client size (titlebar already excluded by the
  // main app), so this is plugin content size end to end.
  int content_w = width;
  int content_h = height;
  if (processor->editor_view && processor->editor_attached) {
    daux_constrain_content_size(processor, &content_w, &content_h);
  }
  const HWND focus_before = GetFocus();
  processor->embed_host_x = x;
  processor->embed_host_y = y;
  processor->embed_host_w = content_w;
  processor->embed_host_h = content_h;
  processor->embed_content_w = content_w;
  processor->embed_content_h = content_h;
  processor->embed_geometry_valid = false;
  if (daux_embed_sync_geometry(processor, x, y, content_w, content_h, daux_embed_debug())) {
    resize_editor_view(processor);
    std::fprintf(stderr,
                 "[plugin-host] host_hwnd resize %dx%d\n",
                 content_w, content_h);
    // Repaint everything the resize exposed — no stale edge pixels.
    if (processor->editor_embed_top_hwnd && IsWindow(processor->editor_embed_top_hwnd)) {
      InvalidateRect(processor->editor_embed_top_hwnd, nullptr, FALSE);
    }
    // Input contract after resize (spec item 10): a geometry change must not
    // steal focus from the plugin subtree.
    if (focus_before && IsWindow(focus_before) && processor->editor_attach_hwnd &&
        (focus_before == processor->editor_attach_hwnd ||
         IsChild(processor->editor_attach_hwnd, focus_before)) &&
        GetFocus() != focus_before) {
      SetFocus(focus_before);
    }
  }
  // The wrapper asked for a size the plugin rejected or adjusted: tell the
  // main app so the shell snaps to the constrained size instead of leaving
  // blank/garbage area around the plugin content. This converges in one round
  // trip — the next ResizeEditor arrives already constrained and is a no-op.
  if (content_w != width || content_h != height) {
    processor->pending_main_shell_w = content_w;
    processor->pending_main_shell_h = content_h;
    processor->pending_main_shell_resize.store(true, std::memory_order_release);
    std::fprintf(stderr,
                 "[PluginEditorResize] wrapper_snapback requested=%dx%d constrained=%dx%d\n",
                 width, height, content_w, content_h);
  }
#else
  (void)processor; (void)x; (void)y; (void)width; (void)height;
#endif
}

extern "C" void sphere_daux_vst3_embed_refresh(SphereDauxVst3Processor* processor) {
#ifdef _WIN32
  if (!processor || !processor->embed_mode || !processor->editor_attach_hwnd) return;
  if (!IsWindow(processor->editor_attach_hwnd)) return;
  // Detached: standalone window manages its own geometry — nothing to re-sync.
  if (processor->embed_host_kind == 2) return;
  // Idle frames: re-sync geometry only (tracks parent moves). onSize/pump only
  // run when the applied rect actually changed — no per-frame flicker/spam.
  if (daux_embed_sync_geometry(processor,
                               processor->embed_host_x, processor->embed_host_y,
                               processor->embed_host_w, processor->embed_host_h,
                               false)) {
    resize_editor_view(processor);
  }
#else
  (void)processor;
#endif
}

extern "C" void sphere_daux_vst3_embed_detach(SphereDauxVst3Processor* processor) {
#ifdef _WIN32
  if (!processor || !processor->embed_mode) return;
  processor->close_embed_editor("embed_detach");
#else
  (void)processor;
#endif
}

// Real Win32 HWND of the embed subtree root (the host child created under the
// main app's content HWND; falls back to the `IPlugView::attached` content
// child). `sphere_daux_vst3_embed_editor` returns an opaque monotonic handle,
// NOT an HWND — callers that need to pump/focus/audit the editor subtree must
// use this.
extern "C" unsigned long long sphere_daux_vst3_embed_attach_hwnd(
    SphereDauxVst3Processor* processor) {
#ifdef _WIN32
  if (!processor) return 0;
  if (processor->editor_embed_top_hwnd && IsWindow(processor->editor_embed_top_hwnd)) {
    return reinterpret_cast<unsigned long long>(processor->editor_embed_top_hwnd);
  }
  if (processor->editor_attach_hwnd && IsWindow(processor->editor_attach_hwnd)) {
    return reinterpret_cast<unsigned long long>(processor->editor_attach_hwnd);
  }
  return 0;
#else
  (void)processor;
  return 0;
#endif
}

extern "C" int sphere_daux_vst3_embed_is_valid(SphereDauxVst3Processor* processor) {
#ifdef _WIN32
  return (processor && processor->embed_mode && processor->editor_attach_hwnd &&
          IsWindow(processor->editor_attach_hwnd)) ? 1 : 0;
#else
  (void)processor;
  return 0;
#endif
}

extern "C" int sphere_daux_vst3_embed_has_visible_ui(SphereDauxVst3Processor* processor) {
#ifdef _WIN32
  return daux_embed_has_visible_ui(processor) ? 1 : 0;
#else
  (void)processor;
  return 0;
#endif
}

extern "C" int sphere_daux_vst3_embed_host_kind(SphereDauxVst3Processor* processor) {
#ifdef _WIN32
  if (!processor || !processor->embed_mode) return -1;
  return processor->embed_host_kind; // 0 child, 1 tool, 2 detached
#else
  (void)processor;
  return -1;
#endif
}

// Detached mode only: returns 1 (and resets) if the user closed the standalone
// editor window (WM_CLOSE). The Rust shell polls this to tear the editor down.
extern "C" int sphere_daux_vst3_embed_take_user_close(SphereDauxVst3Processor* processor) {
#ifdef _WIN32
  if (!processor) return 0;
  return processor->embed_user_closed.exchange(false, std::memory_order_acq_rel) ? 1 : 0;
#else
  (void)processor;
  return 0;
#endif
}

extern "C" void sphere_daux_vst3_embed_set_instance_label(
    SphereDauxVst3Processor* processor, const char* instance_id) {
#ifdef _WIN32
  if (!processor) return;
  processor->embed_instance_label = instance_id ? instance_id : "";
#else
  (void)processor;
  (void)instance_id;
#endif
}

extern "C" int sphere_daux_vst3_prepare_editor_view(
    SphereDauxVst3Processor* processor, int* out_width, int* out_height) {
#ifdef _WIN32
  daux_embed_ensure_com_initialized();
  daux_ensure_thread_dpi_awareness();
  if (!processor || !processor->controller) return 0;
  if (!processor->editor_view) {
    if (!daux_plugin_browser_runtime_prepare(processor)) return 0;
    processor->editor_view = Steinberg::IPtr<Steinberg::IPlugView>::adopt(
        processor->controller->createView(Steinberg::Vst::ViewType::kEditor));
    if (!processor->editor_view) return 0;
    if (processor->editor_view->isPlatformTypeSupported(Steinberg::kPlatformTypeHWND) !=
        Steinberg::kResultTrue) {
      processor->editor_view = nullptr;
      daux_plugin_browser_runtime_release(processor);
      return 0;
    }
    daux_editor_install_frame(processor);
  }
  Steinberg::ViewRect sz{};
  const auto gs = processor->editor_view->getSize(&sz);
  const int w = daux_view_rect_width(sz);
  const int h = daux_view_rect_height(sz);
  if (gs != Steinberg::kResultTrue && gs != Steinberg::kResultOk) return 0;
  if (w <= 0 || h <= 0) return 0;
  if (out_width) *out_width = w;
  if (out_height) *out_height = h;
  std::fprintf(stderr,
               "[PluginEditor] prepare getSize instance=%s view_size=%dx%d\n",
               processor->embed_instance_label.empty()
                   ? "<unknown>"
                   : processor->embed_instance_label.c_str(),
               w,
               h);
  return 1;
#else
  (void)processor;
  (void)out_width;
  (void)out_height;
  return 0;
#endif
}

extern "C" int sphere_daux_vst3_take_pending_shell_resize(
    SphereDauxVst3Processor* processor, int* out_width, int* out_height) {
#ifdef _WIN32
  if (!processor) return 0;
  if (!processor->pending_main_shell_resize.load(std::memory_order_acquire)) return 0;
  processor->pending_main_shell_resize.store(false, std::memory_order_release);
  if (out_width) *out_width = processor->pending_main_shell_w;
  if (out_height) *out_height = processor->pending_main_shell_h;
  return 1;
#else
  (void)processor;
  (void)out_width;
  (void)out_height;
  return 0;
#endif
}

// IPlugView::canResize for the current editor view: 1 resizable, 0 fixed-size,
// -1 unknown (no view). The main app uses this to lock the wrapper window for
// fixed-size editors (spec item 1/8).
extern "C" int sphere_daux_vst3_editor_resizable(SphereDauxVst3Processor* processor) {
#ifdef _WIN32
  if (!processor || !processor->editor_view) return -1;
  return daux_editor_view_resizable(processor) ? 1 : 0;
#else
  (void)processor;
  return -1;
#endif
}

extern "C" int sphere_daux_vst3_embed_content_size(
    SphereDauxVst3Processor* processor, int* out_width, int* out_height) {
#ifdef _WIN32
  if (!processor || !processor->embed_mode) return 0;
  if (processor->embed_content_w <= 0 || processor->embed_content_h <= 0) return 0;
  if (out_width) *out_width = processor->embed_content_w;
  if (out_height) *out_height = processor->embed_content_h;
  return 1;
#else
  (void)processor;
  (void)out_width;
  (void)out_height;
  return 0;
#endif
}

extern "C" unsigned long long sphere_daux_vst3_process_count(
    SphereDauxVst3Processor* processor) {
  return processor ? processor->process_count : 0;
}

extern "C" double sphere_daux_vst3_last_input_peak(SphereDauxVst3Processor* processor) {
  return processor ? processor->last_input_peak : 0.0;
}

extern "C" double sphere_daux_vst3_last_output_peak(SphereDauxVst3Processor* processor) {
  return processor ? processor->last_output_peak : 0.0;
}

extern "C" double sphere_daux_vst3_last_difference_peak(SphereDauxVst3Processor* processor) {
  return processor ? processor->last_difference_peak : 0.0;
}

extern "C" int sphere_daux_vst3_is_valid(SphereDauxVst3Processor* processor) {
#if defined(_WIN32) || defined(__APPLE__) || defined(__linux__)
  return (processor && processor->processor_valid.load(std::memory_order_acquire)) ? 1 : 0;
#else
  return processor ? 1 : 0;
#endif
}

extern "C" int sphere_daux_vst3_get_latency_samples(SphereDauxVst3Processor* processor) {
  if (!processor || !processor->processor) return 0;
  const auto latency = processor->processor->getLatencySamples();
  return static_cast<int>(latency);
}

// ── Plugin state persistence ─────────────────────────────────────────────────
//
// Raw VST3 state streams, exactly as the plugin writes them:
//   component  — IComponent::getState (the processor state; this is the blob
//                IEditController::setComponentState receives on restore)
//   controller — IEditController::getState (GUI-side state; only meaningful
//                for split component/controller plugins)
// Buffers returned by get_state are malloc-owned by the caller and must be
// released with sphere_daux_vst3_state_free.

namespace {

// Copy a captured MemoryStream into a malloc buffer. An empty stream is a
// valid (zero-length) state, not an error.
bool daux_copy_stream(const Steinberg::MemoryStream& stream,
                      unsigned char** out_data,
                      int* out_len) {
  const auto size = stream.getSize();
  if (size <= 0) {
    *out_data = nullptr;
    *out_len = 0;
    return true;
  }
  auto* buf = static_cast<unsigned char*>(std::malloc(static_cast<size_t>(size)));
  if (!buf) return false;
  std::memcpy(buf, stream.getData(), static_cast<size_t>(size));
  *out_data = buf;
  *out_len = static_cast<int>(size);
  return true;
}

}  // namespace

// Capture the plugin's current state. Returns 1 on success (zero-length blobs
// are valid — some plugins have no state), 0 on failure. A plugin returning
// kNotImplemented from getState is treated as "no state", not failure.
extern "C" int sphere_daux_vst3_get_state(
    SphereDauxVst3Processor* processor,
    unsigned char** out_component, int* out_component_len,
    unsigned char** out_controller, int* out_controller_len) {
  if (out_component) *out_component = nullptr;
  if (out_component_len) *out_component_len = 0;
  if (out_controller) *out_controller = nullptr;
  if (out_controller_len) *out_controller_len = 0;
  if (!processor || !processor->component || !out_component || !out_component_len ||
      !out_controller || !out_controller_len) {
    return 0;
  }

  Steinberg::MemoryStream component_stream;
  const auto comp_res = processor->component->getState(&component_stream);
  if (comp_res != Steinberg::kResultOk) {
    component_stream.setSize(0);
  }

  Steinberg::MemoryStream controller_stream;
  // For single-component plugins the controller IS the component — its state
  // already lives in the component stream; querying it again would duplicate.
  if (processor->controller && !processor->controller_is_component) {
    const auto ctrl_res = processor->controller->getState(&controller_stream);
    if (ctrl_res != Steinberg::kResultOk) {
      controller_stream.setSize(0);
    }
  }

  if (!daux_copy_stream(component_stream, out_component, out_component_len)) return 0;
  if (!daux_copy_stream(controller_stream, out_controller, out_controller_len)) {
    std::free(*out_component);
    *out_component = nullptr;
    *out_component_len = 0;
    return 0;
  }
  std::fprintf(stderr,
               "[SphereVST3] get_state ok component_bytes=%d controller_bytes=%d comp_result=0x%x\n",
               *out_component_len, *out_controller_len,
               static_cast<unsigned>(comp_res));
  return 1;
}

// Restore a previously captured state. Restore order follows the VST3
// workflow: IComponent::setState, then IEditController::setComponentState
// (same component blob, fresh cursor), then IEditController::setState with
// the controller blob. Returns 1 when the component state was applied.
extern "C" int sphere_daux_vst3_set_state(
    SphereDauxVst3Processor* processor,
    const unsigned char* component_data, int component_len,
    const unsigned char* controller_data, int controller_len) {
  if (!processor || !processor->component) return 0;

  int ok = 1;
  if (component_data && component_len > 0) {
    // Non-owning stream over the caller's buffer; cursor starts at 0.
    Steinberg::MemoryStream comp_stream(
        const_cast<unsigned char*>(component_data), component_len);
    const auto res = processor->component->setState(&comp_stream);
    if (res != Steinberg::kResultOk) {
      std::fprintf(stderr,
                   "[SphereVST3] set_state component setState result=0x%x bytes=%d\n",
                   static_cast<unsigned>(res), component_len);
      ok = (res == Steinberg::kNotImplemented) ? 1 : 0;
    }
    if (processor->controller && !processor->controller_is_component) {
      Steinberg::MemoryStream sync_stream(
          const_cast<unsigned char*>(component_data), component_len);
      const auto sync_res = processor->controller->setComponentState(&sync_stream);
      if (sync_res != Steinberg::kResultOk) {
        std::fprintf(stderr,
                     "[SphereVST3] set_state controller setComponentState result=0x%x\n",
                     static_cast<unsigned>(sync_res));
      }
    }
  }
  if (controller_data && controller_len > 0 && processor->controller &&
      !processor->controller_is_component) {
    Steinberg::MemoryStream ctrl_stream(
        const_cast<unsigned char*>(controller_data), controller_len);
    const auto res = processor->controller->setState(&ctrl_stream);
    if (res != Steinberg::kResultOk) {
      std::fprintf(stderr,
                   "[SphereVST3] set_state controller setState result=0x%x bytes=%d\n",
                   static_cast<unsigned>(res), controller_len);
    }
  }
  std::fprintf(stderr,
               "[SphereVST3] set_state applied component_bytes=%d controller_bytes=%d ok=%d\n",
               component_len, controller_len, ok);
  return ok;
}

extern "C" void sphere_daux_vst3_state_free(unsigned char* data) {
  std::free(data);
}
