//! Template native DSP backend for a Futureboard audio-plugin extension.
//!
//! This crate is intentionally tiny. Production extensions should keep the
//! realtime process function allocation-free and avoid locks, file I/O, logging,
//! or dynamic dispatch inside the sample loop.

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct StereoSample {
    pub l: f32,
    pub r: f32,
}

#[no_mangle]
pub extern "C" fn futureboard_audio_plugin_version() -> u32 {
    1
}

#[no_mangle]
pub extern "C" fn futureboard_process_stereo_sample(
    input: StereoSample,
    power: f32,
    gain_db: f32,
    mix: f32,
) -> StereoSample {
    if power < 0.5 {
        return input;
    }

    let gain = 10.0f32.powf(gain_db.clamp(-24.0, 24.0) / 20.0);
    let wet_l = (input.l * gain).clamp(-1.5, 1.5);
    let wet_r = (input.r * gain).clamp(-1.5, 1.5);
    let mix = (mix.clamp(0.0, 100.0)) / 100.0;

    StereoSample {
        l: input.l * (1.0 - mix) + wet_l * mix,
        r: input.r * (1.0 - mix) + wet_r * mix,
    }
}
