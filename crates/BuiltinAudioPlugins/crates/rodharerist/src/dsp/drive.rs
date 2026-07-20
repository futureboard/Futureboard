//! Overdrive / boost / fuzz pedals — multiple voicings sharing one processor.

use builtin_dsp_core::{make_eq_biquad, mix};

use super::{DriveModel, StereoBiquad, soft_clip};

#[derive(Debug, Clone)]
pub(super) struct Drive {
    sample_rate: f32,
    model: DriveModel,
    pre_gain: f32,
    out_gain: f32,
    mix: f32,
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
                self.pre_gain = 1.0 + g * 24.0;
                self.out_gain = 0.25 + lvl * 1.1;
                self.mix = 1.0;
                self.input_hpf.set(make_eq_biquad("highpass", 220.0, 0.0, 0.7, sr));
                self.mid_boost.set(make_eq_biquad("bell", 720.0, 6.0, 0.7, sr));
                let cutoff = 2_000.0 + t * 4_000.0;
                self.tone_lpf.set(make_eq_biquad("lowpass", cutoff, 0.0, 0.707, sr));
            }
            DriveModel::Minotaur => {
                self.pre_gain = 1.0 + g * 8.0;
                self.out_gain = 0.4 + lvl * 1.3;
                self.mix = 0.85;
                self.input_hpf.set(make_eq_biquad("highpass", 35.0, 0.0, 0.7, sr));
                self.mid_boost.set(None);
                let cutoff = (4_000.0 + t * 8_000.0).min(sr * 0.45);
                self.tone_lpf.set(make_eq_biquad("lowpass", cutoff, 0.0, 0.707, sr));
            }
            DriveModel::Rat => {
                self.pre_gain = 1.0 + g * 36.0;
                self.out_gain = 0.18 + lvl * 0.95;
                self.mix = 1.0;
                self.input_hpf.set(make_eq_biquad("highpass", 120.0, 0.0, 0.7, sr));
                self.mid_boost.set(make_eq_biquad("bell", 1_100.0, 3.0, 0.9, sr));
                let cutoff = 1_200.0 + t * 5_500.0;
                self.tone_lpf.set(make_eq_biquad("lowpass", cutoff, 0.0, 0.707, sr));
            }
            DriveModel::Breaker => {
                self.pre_gain = 1.0 + g * 12.0;
                self.out_gain = 0.35 + lvl * 1.15;
                self.mix = 0.92;
                self.input_hpf.set(make_eq_biquad("highpass", 80.0, 0.0, 0.7, sr));
                self.mid_boost.set(make_eq_biquad("bell", 650.0, 2.5, 0.8, sr));
                let cutoff = 2_500.0 + t * 6_000.0;
                self.tone_lpf.set(make_eq_biquad("lowpass", cutoff, 0.0, 0.707, sr));
            }
            DriveModel::Fuzz => {
                self.pre_gain = 1.0 + g * 48.0;
                self.out_gain = 0.15 + lvl * 0.85;
                self.mix = 1.0;
                self.input_hpf.set(make_eq_biquad("highpass", 60.0, 0.0, 0.7, sr));
                self.mid_boost.set(make_eq_biquad("bell", 400.0, 4.0, 0.6, sr));
                let cutoff = 900.0 + t * 3_500.0;
                self.tone_lpf.set(make_eq_biquad("lowpass", cutoff, 0.0, 0.707, sr));
            }
            DriveModel::Centurion => {
                self.pre_gain = 1.0 + g * 14.0;
                self.out_gain = 0.32 + lvl * 1.2;
                self.mix = 0.88;
                self.input_hpf.set(make_eq_biquad("highpass", 90.0, 0.0, 0.7, sr));
                self.mid_boost.set(make_eq_biquad("bell", 780.0, 4.5, 0.75, sr));
                let cutoff = 3_000.0 + t * 7_000.0;
                self.tone_lpf.set(make_eq_biquad("lowpass", cutoff.min(sr * 0.45), 0.0, 0.707, sr));
            }
        }
    }

    #[inline]
    fn shape(&self, x: f32) -> f32 {
        match self.model {
            DriveModel::Screamer => soft_clip(x + 0.05 * x * x),
            DriveModel::Minotaur => soft_clip(x * 0.7) / 0.7f32.tanh(),
            DriveModel::Rat => {
                // Harder fold for rat-like grit.
                let y = soft_clip(x * 1.4);
                soft_clip(y * 1.1)
            }
            DriveModel::Breaker => soft_clip(x * 0.85) / 0.85f32.tanh(),
            DriveModel::Fuzz => {
                // Heavy asymmetric square-ish fuzz.
                let biased = x + 0.18 * x.abs() * x;
                soft_clip(biased * 1.8)
            }
            DriveModel::Centurion => soft_clip(x + 0.02 * x * x),
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
