//! Overdrive / boost pedal — Green Screamer (mid-hump soft clip) and Minotaur
//! (clean transparent boost).

use builtin_dsp_core::{make_eq_biquad, mix};

use super::{DriveModel, StereoBiquad, soft_clip};

#[derive(Debug, Clone)]
pub(super) struct Drive {
    sample_rate: f32,
    model: DriveModel,
    pre_gain: f32,
    out_gain: f32,
    mix: f32,
    // Screamer voicing: input HPF + mid emphasis before clipping, tone LPF after.
    input_hpf: StereoBiquad,
    mid_boost: StereoBiquad,
    tone_lpf: StereoBiquad,
}

impl Drive {
    pub(super) fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate: sample_rate.max(1.0),
            model: DriveModel::Screamer,
            pre_gain: 1.0,
            out_gain: 1.0,
            mix: 1.0,
            input_hpf: StereoBiquad::none(),
            mid_boost: StereoBiquad::none(),
            tone_lpf: StereoBiquad::none(),
        }
    }

    pub(super) fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
    }

    pub(super) fn reset(&mut self) {
        self.input_hpf.reset();
        self.mid_boost.reset();
        self.tone_lpf.reset();
    }

    /// `gain`, `tone`, `level` are the editor's 0..10 knobs.
    pub(super) fn configure(&mut self, model: DriveModel, gain: f32, tone: f32, level: f32) {
        self.model = model;
        let g = (gain / 10.0).clamp(0.0, 1.0);
        let t = (tone / 10.0).clamp(0.0, 1.0);
        let lvl = (level / 10.0).clamp(0.0, 1.0);
        let sr = self.sample_rate;

        match model {
            DriveModel::Screamer => {
                // Tube-screamer style: strong pre-gain, tight low end, mid hump.
                self.pre_gain = 1.0 + g * 24.0;
                self.out_gain = 0.25 + lvl * 1.1;
                self.mix = 1.0;
                self.input_hpf.set(make_eq_biquad("highpass", 220.0, 0.0, 0.7, sr));
                self.mid_boost.set(make_eq_biquad("bell", 720.0, 6.0, 0.7, sr));
                // Tone sweeps the post low-pass 2 kHz → 6 kHz.
                let cutoff = 2_000.0 + t * 4_000.0;
                self.tone_lpf.set(make_eq_biquad("lowpass", cutoff, 0.0, 0.707, sr));
            }
            DriveModel::Minotaur => {
                // Clean-ish boost: gentle gain, full range, minimal coloration.
                self.pre_gain = 1.0 + g * 8.0;
                self.out_gain = 0.4 + lvl * 1.3;
                self.mix = 0.85; // keep some dry for transparency
                self.input_hpf.set(make_eq_biquad("highpass", 35.0, 0.0, 0.7, sr));
                self.mid_boost.set(None);
                let cutoff = (4_000.0 + t * 8_000.0).min(sr * 0.45);
                self.tone_lpf.set(make_eq_biquad("lowpass", cutoff, 0.0, 0.707, sr));
            }
        }
    }

    #[inline]
    fn shape(&self, x: f32) -> f32 {
        match self.model {
            // Slight asymmetry for the screamer → richer even harmonics.
            DriveModel::Screamer => soft_clip(x + 0.05 * x * x),
            // Symmetric, high-headroom clip for the transparent boost.
            DriveModel::Minotaur => soft_clip(x * 0.7) / 0.7f32.tanh(),
        }
    }

    #[inline]
    pub(super) fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        let (mut l, mut r) = self.input_hpf.run(left, right);
        l = self.shape(l * self.pre_gain);
        r = self.shape(r * self.pre_gain);
        (l, r) = self.mid_boost.run(l, r);
        (l, r) = self.tone_lpf.run(l, r);
        l *= self.out_gain;
        r *= self.out_gain;
        (mix(left, l, self.mix), mix(right, r, self.mix))
    }
}
