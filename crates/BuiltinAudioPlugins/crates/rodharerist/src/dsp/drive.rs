//! Overdrive / boost / fuzz pedals — multiple voicings sharing one processor.
//!
//! The waveshaper runs 2× oversampled ([`Oversampler2x`]) so high-gain
//! voicings (Rat, Fuzz) fold back far less aliasing, and the gain/mix
//! controls are smoothed ([`Smoothed`]) so live knob drags don't zipper.

use builtin_dsp_core::{make_eq_biquad, mix};

use super::drive_models::{DsClassic, MetalCore, SuperDrive, TightRift};
use super::smooth::{Oversampler2x, Smoothed};
use super::{soft_clip, DriveModel, StereoBiquad};

/// Glide time for gain/level/mix edits (fast enough to feel immediate).
const SMOOTH_SECONDS: f32 = 0.010;

#[derive(Debug, Clone)]
pub(super) struct Drive {
    sample_rate: f32,
    model: DriveModel,
    pre_gain: Smoothed,
    out_gain: Smoothed,
    mix: Smoothed,
    input_hpf: StereoBiquad,
    mid_boost: StereoBiquad,
    tone_lpf: StereoBiquad,
    oversampler: Oversampler2x,
    // The four modern models own full dedicated topologies (multi-stage,
    // higher oversampling, dynamics) — see `drive_models.rs`. The legacy six
    // keep the generic path above.
    ds_one: DsClassic,
    super_drive: SuperDrive,
    metal_core: MetalCore,
    tight_rift: TightRift,
}

impl Drive {
    pub(super) fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        Self {
            sample_rate: sr,
            model: DriveModel::Screamer,
            pre_gain: Smoothed::new(sr, SMOOTH_SECONDS, 1.0),
            out_gain: Smoothed::new(sr, SMOOTH_SECONDS, 1.0),
            mix: Smoothed::new(sr, SMOOTH_SECONDS, 1.0),
            input_hpf: StereoBiquad::none(),
            mid_boost: StereoBiquad::none(),
            tone_lpf: StereoBiquad::none(),
            oversampler: Oversampler2x::new(),
            ds_one: DsClassic::new(sr),
            super_drive: SuperDrive::new(sr),
            metal_core: MetalCore::new(sr),
            tight_rift: TightRift::new(sr),
        }
    }

    pub(super) fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.pre_gain.set_time(self.sample_rate, SMOOTH_SECONDS);
        self.out_gain.set_time(self.sample_rate, SMOOTH_SECONDS);
        self.mix.set_time(self.sample_rate, SMOOTH_SECONDS);
        self.ds_one.set_sample_rate(self.sample_rate);
        self.super_drive.set_sample_rate(self.sample_rate);
        self.metal_core.set_sample_rate(self.sample_rate);
        self.tight_rift.set_sample_rate(self.sample_rate);
    }

    pub(super) fn reset(&mut self) {
        self.input_hpf.reset();
        self.mid_boost.reset();
        self.tone_lpf.reset();
        self.oversampler.reset();
        self.pre_gain.snap();
        self.out_gain.snap();
        self.mix.snap();
        self.ds_one.reset();
        self.super_drive.reset();
        self.metal_core.reset();
        self.tight_rift.reset();
    }

    /// `gain`, `tone`, `level` are the editor's 0..10 knobs.
    pub(super) fn configure(&mut self, model: DriveModel, gain: f32, tone: f32, level: f32) {
        self.model = model;
        // Dedicated-topology models: route and return — the generic voicing
        // table below only serves the legacy six.
        match model {
            DriveModel::DsOne => return self.ds_one.configure(gain, tone, level),
            DriveModel::SuperDrive => return self.super_drive.configure(gain, tone, level),
            DriveModel::MetalCore => return self.metal_core.configure(gain, tone, level),
            DriveModel::TightRift => return self.tight_rift.configure(gain, tone, level),
            _ => {}
        }
        let g = (gain / 10.0).clamp(0.0, 1.0);
        let t = (tone / 10.0).clamp(0.0, 1.0);
        let lvl = (level / 10.0).clamp(0.0, 1.0);
        let sr = self.sample_rate;

        // (pre_gain, out_gain, mix) targets — glided, not jumped.
        let (pre, out, mix_amount) = match model {
            DriveModel::Screamer => (1.0 + g * 24.0, 0.25 + lvl * 1.1, 1.0),
            DriveModel::Minotaur => (1.0 + g * 8.0, 0.4 + lvl * 1.3, 0.85),
            DriveModel::Rat => (1.0 + g * 36.0, 0.18 + lvl * 0.95, 1.0),
            DriveModel::Breaker => (1.0 + g * 12.0, 0.35 + lvl * 1.15, 0.92),
            DriveModel::Fuzz => (1.0 + g * 48.0, 0.15 + lvl * 0.85, 1.0),
            DriveModel::Centurion => (1.0 + g * 14.0, 0.32 + lvl * 1.2, 0.88),
            // Dedicated-topology models returned above.
            _ => (1.0, 1.0, 1.0),
        };
        self.pre_gain.set_target(pre);
        self.out_gain.set_target(out);
        self.mix.set_target(mix_amount);

        match model {
            DriveModel::Screamer => {
                self.input_hpf
                    .set(make_eq_biquad("highpass", 220.0, 0.0, 0.7, sr));
                self.mid_boost
                    .set(make_eq_biquad("bell", 720.0, 6.0, 0.7, sr));
                let cutoff = 2_000.0 + t * 4_000.0;
                self.tone_lpf
                    .set(make_eq_biquad("lowpass", cutoff, 0.0, 0.707, sr));
            }
            DriveModel::Minotaur => {
                self.input_hpf
                    .set(make_eq_biquad("highpass", 35.0, 0.0, 0.7, sr));
                self.mid_boost.set(None);
                let cutoff = (4_000.0 + t * 8_000.0).min(sr * 0.45);
                self.tone_lpf
                    .set(make_eq_biquad("lowpass", cutoff, 0.0, 0.707, sr));
            }
            DriveModel::Rat => {
                self.input_hpf
                    .set(make_eq_biquad("highpass", 120.0, 0.0, 0.7, sr));
                self.mid_boost
                    .set(make_eq_biquad("bell", 1_100.0, 3.0, 0.9, sr));
                let cutoff = 1_200.0 + t * 5_500.0;
                self.tone_lpf
                    .set(make_eq_biquad("lowpass", cutoff, 0.0, 0.707, sr));
            }
            DriveModel::Breaker => {
                self.input_hpf
                    .set(make_eq_biquad("highpass", 80.0, 0.0, 0.7, sr));
                self.mid_boost
                    .set(make_eq_biquad("bell", 650.0, 2.5, 0.8, sr));
                let cutoff = 2_500.0 + t * 6_000.0;
                self.tone_lpf
                    .set(make_eq_biquad("lowpass", cutoff, 0.0, 0.707, sr));
            }
            DriveModel::Fuzz => {
                self.input_hpf
                    .set(make_eq_biquad("highpass", 60.0, 0.0, 0.7, sr));
                self.mid_boost
                    .set(make_eq_biquad("bell", 400.0, 4.0, 0.6, sr));
                let cutoff = 900.0 + t * 3_500.0;
                self.tone_lpf
                    .set(make_eq_biquad("lowpass", cutoff, 0.0, 0.707, sr));
            }
            DriveModel::Centurion => {
                self.input_hpf
                    .set(make_eq_biquad("highpass", 90.0, 0.0, 0.7, sr));
                self.mid_boost
                    .set(make_eq_biquad("bell", 780.0, 4.5, 0.75, sr));
                let cutoff = 3_000.0 + t * 7_000.0;
                self.tone_lpf.set(make_eq_biquad(
                    "lowpass",
                    cutoff.min(sr * 0.45),
                    0.0,
                    0.707,
                    sr,
                ));
            }
            // Dedicated-topology models returned above.
            _ => {}
        }
    }

    #[inline]
    fn shape(model: DriveModel, x: f32) -> f32 {
        match model {
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
            // Dedicated-topology models never reach the generic shaper.
            _ => x,
        }
    }

    #[inline]
    pub(super) fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        match self.model {
            DriveModel::DsOne => return self.ds_one.process(left, right),
            DriveModel::SuperDrive => return self.super_drive.process(left, right),
            DriveModel::MetalCore => return self.metal_core.process(left, right),
            DriveModel::TightRift => return self.tight_rift.process(left, right),
            _ => {}
        }
        let pre = self.pre_gain.tick();
        let out = self.out_gain.tick();
        let mix_amount = self.mix.tick();
        let model = self.model;

        let (mut l, mut r) = self.input_hpf.run(left, right);
        // Waveshape at 2× rate: the shaper is memoryless, so only the
        // up/down half-band filters carry state across the doubled rate.
        (l, r) = self.oversampler.process_stereo(l * pre, r * pre, |a, b| {
            (Self::shape(model, a), Self::shape(model, b))
        });
        (l, r) = self.mid_boost.run(l, r);
        (l, r) = self.tone_lpf.run(l, r);
        l *= out;
        r *= out;
        (mix(left, l, mix_amount), mix(right, r, mix_amount))
    }
}
