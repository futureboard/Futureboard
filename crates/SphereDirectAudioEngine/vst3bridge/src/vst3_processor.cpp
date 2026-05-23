#include "sphere_daux_vst3_processor.h"

#include <algorithm>
#include <array>
#include <atomic>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <memory>
#include <mutex>
#include <sstream>
#include <string>

// IPlugView is needed on all platforms for the editor bridge functions.
#include "pluginterfaces/gui/iplugview.h"

#ifdef _WIN32
#  define WIN32_LEAN_AND_MEAN
#  define NOMINMAX
#  include <windows.h>
#  include <dwmapi.h>
#  pragma comment(lib, "dwmapi.lib")
#endif

#include "pluginterfaces/base/ipluginbase.h"
#include "pluginterfaces/vst/ivstaudioprocessor.h"
#include "pluginterfaces/vst/ivstcomponent.h"
#include "pluginterfaces/vst/ivsteditcontroller.h"
#include "pluginterfaces/vst/ivstparameterchanges.h"
#include "pluginterfaces/vst/ivstprocesscontext.h"
#include "public.sdk/source/vst/hosting/hostclasses.h"
#include "public.sdk/source/vst/hosting/module.h"
#include "public.sdk/source/vst/utility/uid.h"
#include "sphere_daux_editor_bridge.h"

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

// Forward declaration so ComponentHandlerImpl can hold a back-pointer.
struct SphereDauxVst3Processor;

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
  ComponentHandlerImpl component_handler;  // installed on IEditController

#if defined(_WIN32)
  Steinberg::IPtr<Steinberg::IPlugView> editor_view;
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
#endif
};

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
    const auto removed_res = processor->editor_view->removed();
    std::fprintf(stderr,
                 "[SphereVST3] IPlugView::removed() result=%d handle=%llu\n",
                 (int)removed_res, processor->editor_handle);
  }
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

void resize_editor_view(SphereDauxVst3Processor* processor) {
  if (!processor || !processor->editor_view || !processor->editor_attach_hwnd) return;
  RECT rc{};
  GetClientRect(processor->editor_attach_hwnd, &rc);
  Steinberg::ViewRect view_rect{
      static_cast<Steinberg::int32>(rc.left),
      static_cast<Steinberg::int32>(rc.top),
      static_cast<Steinberg::int32>(rc.right),
      static_cast<Steinberg::int32>(rc.bottom),
  };
  const auto resize_res = processor->editor_view->onSize(&view_rect);
  static std::atomic<unsigned int> resize_log_count{0};
  const unsigned int count = resize_log_count.fetch_add(1);
  if (count < 12 || count % 50 == 0) {
    std::fprintf(stderr,
                 "[SphereVST3] IPlugView::onSize() result=%d handle=%llu rect=%d,%d,%d,%d count=%u\n",
                 (int)resize_res,
                 processor->editor_handle,
                 (int)view_rect.left,
                 (int)view_rect.top,
                 (int)view_rect.right,
                 (int)view_rect.bottom,
                 count + 1);
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

void SphereDauxVst3Processor::close_editor_window() {
  HWND hwnd = editor_hwnd;
  HWND child = editor_attach_hwnd;
  // Zero back-pointer FIRST so any pending messages dispatched after this
  // cannot dereference the (potentially freed) SphereDauxVst3Processor.
  if (hwnd && IsWindow(hwnd)) {
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
  }
  detach_editor_view(this);
  if (child && IsWindow(child)) {
    DestroyWindow(child);
  }
  destroy_fallback_controls(this);
  editor_attach_hwnd = nullptr;
  editor_hwnd = nullptr;
  editor_handle = 0;
  editor_window_id.clear();
  editor_title.clear();
  editor_requested_width = 0;
  editor_requested_height = 0;
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

  // Drain pending parameter changes into inputParameterChanges.
  // Lock scope is minimal — no allocation occurs here.
  {
    std::lock_guard<std::mutex> lock(processor->pending_mutex);
    if (processor->pending_count > 0) {
      processor->param_changes_obj.reset();
      for (int i = 0; i < processor->pending_count; ++i) {
        Steinberg::int32 idx = 0;
        auto* q = processor->param_changes_obj.addParameterData(
            processor->pending_buf[i].id, idx);
        if (q) {
          Steinberg::int32 dummy = 0;
          q->addPoint(0, processor->pending_buf[i].value, dummy);
        }
      }
      processor->pending_count = 0;
      processor->process_data.inputParameterChanges = &processor->param_changes_obj;
    } else {
      processor->process_data.inputParameterChanges = nullptr;
    }
  }

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
  if (!processor || !processor->processor || !in_l || !in_r || !out_l || !out_r || frames <= 0) {
    return 0;
  }

  {
    std::lock_guard<std::mutex> lock(processor->pending_mutex);
    if (processor->pending_count > 0) {
      processor->param_changes_obj.reset();
      for (int i = 0; i < processor->pending_count; ++i) {
        Steinberg::int32 idx = 0;
        auto* q = processor->param_changes_obj.addParameterData(
            processor->pending_buf[i].id, idx);
        if (q) {
          Steinberg::int32 dummy = 0;
          q->addPoint(0, processor->pending_buf[i].value, dummy);
        }
      }
      processor->pending_count = 0;
      processor->process_data.inputParameterChanges = &processor->param_changes_obj;
    } else {
      processor->process_data.inputParameterChanges = nullptr;
    }
  }

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
  processor->input_bus.channelBuffers32 = processor->input_channels;
  processor->output_bus.channelBuffers32 = processor->output_channels;

  if (result != Steinberg::kResultOk) return 0;
  processor->process_count += 1;
  return 1;
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
        kDauxEditorChildClass,
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

    // Attach to fresh child HWND.
    const auto attach_res = processor->editor_view->attached(
        reinterpret_cast<void*>(child), Steinberg::kPlatformTypeHWND);
    std::fprintf(stderr,
                 "[SphereVST3] editor reopen: IPlugView::attached() result=%d handle=%llu\n",
                 (int)attach_res, processor->editor_handle);
    if (attach_res != Steinberg::kResultTrue && attach_res != Steinberg::kResultOk) {
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

  processor->editor_view = Steinberg::IPtr<Steinberg::IPlugView>::adopt(
      processor->controller->createView(Steinberg::Vst::ViewType::kEditor));
  std::fprintf(stderr,
               "[SphereVST3] IPlugView createView pluginInstanceId=%s ptr=%p exists=%d\n",
               identity,
               static_cast<void*>(processor->editor_view.get()),
               processor->editor_view ? 1 : 0);
  if (!processor->editor_view) {
    set_last_error("controller did not create editor view");
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

  RECT rect{0, 0, editor_width, editor_height};
  AdjustWindowRectEx(&rect, WS_OVERLAPPEDWINDOW, FALSE, WS_EX_TOPMOST);
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
  // Ensure always-on-top after creation (belt-and-suspenders alongside WS_EX_TOPMOST).
  SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);

  HWND child = CreateWindowExW(
      0,
      kDauxEditorChildClass,
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
               "[SphereVST3] editor HWNDs pluginInstanceId=%s handle=%llu mainHWND=0x%p childHWND=0x%p\n",
               identity,
               processor->editor_handle,
               static_cast<void*>(hwnd),
               static_cast<void*>(child));

  const auto attach_result =
      processor->editor_view->attached(reinterpret_cast<void*>(child), Steinberg::kPlatformTypeHWND);
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
  resize_editor_view(processor);
  ShowWindow(hwnd, SW_SHOWNORMAL);
  UpdateWindow(hwnd);
  SetForegroundWindow(hwnd);
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
