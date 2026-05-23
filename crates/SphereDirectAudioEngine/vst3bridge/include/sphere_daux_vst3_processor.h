#pragma once

#ifdef _WIN32
#  define SPHERE_DAUX_VST3_API __declspec(dllexport)
#else
#  define SPHERE_DAUX_VST3_API __attribute__((visibility("default")))
#endif

extern "C" {

struct SphereDauxVst3Processor;

SPHERE_DAUX_VST3_API int sphere_daux_vst3_bridge_probe(void);

SPHERE_DAUX_VST3_API const char* sphere_daux_vst3_last_error(void);

SPHERE_DAUX_VST3_API SphereDauxVst3Processor* sphere_daux_vst3_create(
    const char* plugin_path,
    const char* class_id,
    double sample_rate);

SPHERE_DAUX_VST3_API void sphere_daux_vst3_destroy(SphereDauxVst3Processor* processor);

SPHERE_DAUX_VST3_API int sphere_daux_vst3_process_stereo_sample(
    SphereDauxVst3Processor* processor,
    float in_l,
    float in_r,
    float* out_l,
    float* out_r);

SPHERE_DAUX_VST3_API int sphere_daux_vst3_process_stereo_block(
    SphereDauxVst3Processor* processor,
    const float* in_l,
    const float* in_r,
    float* out_l,
    float* out_r,
    int frames);

SPHERE_DAUX_VST3_API unsigned long long sphere_daux_vst3_process_count(
    SphereDauxVst3Processor* processor);

SPHERE_DAUX_VST3_API double sphere_daux_vst3_last_input_peak(
    SphereDauxVst3Processor* processor);

SPHERE_DAUX_VST3_API double sphere_daux_vst3_last_output_peak(
    SphereDauxVst3Processor* processor);

SPHERE_DAUX_VST3_API double sphere_daux_vst3_last_difference_peak(
    SphereDauxVst3Processor* processor);

/// Enqueue a normalized parameter change (0..1) for the given VST3 ParamID.
/// The change is delivered to IAudioProcessor via inputParameterChanges on the
/// next sphere_daux_vst3_process_stereo_sample call.
/// Thread-safe: may be called from the audio thread or the UI thread.
SPHERE_DAUX_VST3_API void sphere_daux_vst3_set_param(
    SphereDauxVst3Processor* processor,
    unsigned int param_id,
    double value);

/// Open/focus/close a native editor for the already-created processor instance.
/// UI thread only. The editor is bound to the same component/controller that
/// feeds the realtime processor, so GUI parameter edits enqueue into the same
/// parameter queue consumed by process().
SPHERE_DAUX_VST3_API unsigned long long sphere_daux_vst3_open_editor(
    SphereDauxVst3Processor* processor,
    const char* window_id,
    const char* title,
    int width,
    int height);

SPHERE_DAUX_VST3_API void sphere_daux_vst3_close_editor(
    SphereDauxVst3Processor* processor);

SPHERE_DAUX_VST3_API int sphere_daux_vst3_focus_editor(
    SphereDauxVst3Processor* processor);

/// Returns 1 if the processor has not been destroyed, 0 if it has.
/// The audio callback should call this before processing and bypass the insert
/// if it returns 0, to avoid use-after-free crashes.
SPHERE_DAUX_VST3_API int sphere_daux_vst3_is_valid(
    SphereDauxVst3Processor* processor);

/// Returns the plugin's reported latency in samples (0 if not available).
/// Divide by sample_rate to get latency in seconds.
SPHERE_DAUX_VST3_API int sphere_daux_vst3_get_latency_samples(
    SphereDauxVst3Processor* processor);

}
