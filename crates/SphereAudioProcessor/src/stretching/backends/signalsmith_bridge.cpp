#include "signalsmith_bridge.h"

#include <algorithm>
#include <cmath>
#include <cstring>
#include <vector>

#include "signalsmith-stretch.h"

namespace {

using Stretch = signalsmith::stretch::SignalsmithStretch<float>;

constexpr int kErrorNull = -1;
constexpr int kErrorInvalid = -2;
constexpr int kErrorProcess = -3;

bool valid_ratio(float value) {
    return std::isfinite(value) && value > 0.0f;
}

template <typename T>
struct ChannelInput {
    const T *channels[2];
    int length;

    const T *operator[](int channel) const {
        return channels[channel];
    }
};

template <typename T>
struct ChannelOutput {
    T *channels[2];
    int length;

    T *operator[](int channel) {
        return channels[channel];
    }
};

struct FbSignalsmithHandle {
    Stretch stretch;
    int channels = 2;
    float sample_rate = 48'000.0f;
    float time_ratio = 1.0f;
    float pitch_ratio = 1.0f;
    float quality = 0.75f;
    bool configured = false;
    std::vector<float> pending_l;
    std::vector<float> pending_r;

    void apply_preset() {
        if (channels <= 0 || !std::isfinite(sample_rate) || sample_rate <= 0.0f) {
            configured = false;
            return;
        }

        if (quality < 0.5f) {
            stretch.presetCheaper(channels, sample_rate, true);
        } else {
            stretch.presetDefault(channels, sample_rate, false);
        }
        configured = true;
    }

    void apply_ratios() {
        if (!configured) {
            apply_preset();
        }
        stretch.setTransposeFactor(valid_ratio(pitch_ratio) ? pitch_ratio : 1.0f);
    }
};

} // namespace

extern "C" {

void *fb_signalsmith_create(float sample_rate, int channels) {
    if (!std::isfinite(sample_rate) || sample_rate <= 0.0f || channels <= 0 || channels > 2) {
        return nullptr;
    }

    auto *handle = new (std::nothrow) FbSignalsmithHandle();
    if (handle == nullptr) {
        return nullptr;
    }

    handle->sample_rate = sample_rate;
    handle->channels = channels;
    handle->apply_preset();
    handle->apply_ratios();
    return handle;
}

void fb_signalsmith_destroy(void *handle) {
    delete static_cast<FbSignalsmithHandle *>(handle);
}

void fb_signalsmith_reset(void *handle) {
    if (handle == nullptr) {
        return;
    }
    auto *state = static_cast<FbSignalsmithHandle *>(handle);
    state->stretch.reset();
    state->pending_l.clear();
    state->pending_r.clear();
}

int fb_signalsmith_process_stereo(
    void *handle,
    const float *input_l,
    const float *input_r,
    float *output_l,
    float *output_r,
    int frames,
    float time_ratio,
    float pitch_ratio,
    float quality
) {
    if (handle == nullptr || input_l == nullptr || input_r == nullptr || output_l == nullptr
        || output_r == nullptr || frames <= 0) {
        return kErrorNull;
    }

    auto *state = static_cast<FbSignalsmithHandle *>(handle);
    if (!valid_ratio(time_ratio)) {
        time_ratio = 1.0f;
    }
    if (!valid_ratio(pitch_ratio)) {
        pitch_ratio = 1.0f;
    }
    if (!std::isfinite(quality)) {
        quality = 0.75f;
    }

    const bool reconfigure = !state->configured || state->quality != quality;
    state->time_ratio = time_ratio;
    state->pitch_ratio = pitch_ratio;
    state->quality = quality;

    if (reconfigure) {
        state->apply_preset();
    }
    state->apply_ratios();

    state->pending_l.insert(state->pending_l.end(), input_l, input_l + frames);
    state->pending_r.insert(state->pending_r.end(), input_r, input_r + frames);

    const int input_frames =
        std::max(1, static_cast<int>(std::lround(static_cast<double>(frames) / time_ratio)));

    if (static_cast<int>(state->pending_l.size()) < input_frames
        || static_cast<int>(state->pending_r.size()) < input_frames) {
        std::fill(output_l, output_l + frames, 0.0f);
        std::fill(output_r, output_r + frames, 0.0f);
        return 0;
    }

    ChannelInput<float> inputs{ { state->pending_l.data(), state->pending_r.data() }, input_frames };
    ChannelOutput<float> outputs{ { output_l, output_r }, frames };

    try {
        if (state->channels == 1) {
            std::vector<float> mono_in(static_cast<size_t>(input_frames));
            std::vector<float> mono_out(static_cast<size_t>(frames));
            for (int i = 0; i < input_frames; ++i) {
                mono_in[static_cast<size_t>(i)] =
                    0.5f * (state->pending_l[static_cast<size_t>(i)]
                        + state->pending_r[static_cast<size_t>(i)]);
            }

            ChannelInput<float> mono_in_io{ { mono_in.data(), mono_in.data() }, input_frames };
            ChannelOutput<float> mono_out_io{ { mono_out.data(), mono_out.data() }, frames };
            state->stretch.process(mono_in_io, input_frames, mono_out_io, frames);

            for (int i = 0; i < frames; ++i) {
                const float sample = mono_out[static_cast<size_t>(i)];
                output_l[i] = sample;
                output_r[i] = sample;
            }
        } else {
            state->stretch.process(inputs, input_frames, outputs, frames);
        }

        state->pending_l.erase(state->pending_l.begin(), state->pending_l.begin() + input_frames);
        state->pending_r.erase(state->pending_r.begin(), state->pending_r.begin() + input_frames);
        return 0;
    } catch (...) {
        return kErrorProcess;
    }
}

int fb_signalsmith_latency_samples(void *handle) {
    if (handle == nullptr) {
        return 0;
    }
    auto *state = static_cast<FbSignalsmithHandle *>(handle);
    return state->stretch.inputLatency() + state->stretch.outputLatency();
}

} // extern "C"
