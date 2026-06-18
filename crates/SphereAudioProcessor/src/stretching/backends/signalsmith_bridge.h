#pragma once

#ifdef __cplusplus
extern "C" {
#endif

void *fb_signalsmith_create(float sample_rate, int channels);
void fb_signalsmith_destroy(void *handle);
void fb_signalsmith_reset(void *handle);

// Time-stretch is expressed by the input/output sample-count ratio
// (`output_frames / input_frames`); the caller supplies exactly `input_frames`
// source samples and requests `output_frames` output samples. This keeps the
// bridge a thin, allocation-free pass-through to Signalsmith (no internal
// pending/grow buffers in the realtime path). `pitch_ratio` is the independent
// transpose factor.
int fb_signalsmith_process_stereo(
    void *handle,
    const float *input_l,
    const float *input_r,
    float *output_l,
    float *output_r,
    int input_frames,
    int output_frames,
    float pitch_ratio,
    float quality
);

int fb_signalsmith_latency_samples(void *handle);

#ifdef __cplusplus
}
#endif
