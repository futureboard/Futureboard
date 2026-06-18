#pragma once

#ifdef __cplusplus
extern "C" {
#endif

void *fb_signalsmith_create(float sample_rate, int channels);
void fb_signalsmith_destroy(void *handle);
void fb_signalsmith_reset(void *handle);

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
);

int fb_signalsmith_latency_samples(void *handle);

#ifdef __cplusplus
}
#endif
