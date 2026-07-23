//! Overdrive / boost / fuzz pedals — multiple voicings sharing one processor.
//!
//! The legacy six voicings share a TS-inspired split topology: a phase-coherent
//! one-pole split keeps the low band clean while the high band is mid-emphasized
//! *before* the clipper (voicing into, not after, the nonlinearity — full-band
//! clipping is what made these muddy), then re-blends the clean lows on the way
//! out so the pedal stays tight without going thin. An envelope-driven bias
//! shifts the shaper's operating point with pick attack, so harmonics bloom
//! with dynamics instead of a static waveshape; the resulting moving DC offset
//! is absorbed by a post DC blocker. The waveshaper runs 2× oversampled
//! ([`Oversampler2x`]) so high-gain voicings (Rat, Fuzz) fold back far less
//! aliasing, and the gain/mix controls are smoothed ([`Smoothed`]) so live
//! knob drags don't zipper.

use builtin_dsp_core::{make_eq_biquad, mix};

use super::drive_models::{DcBlock, DsClassic, EnvFollower, MetalCore, SuperDrive, TightRift};
use super::smooth::{Oversampler2x, Smoothed};
use super::{DriveModel, StereoBiquad, soft_clip};

/// Glide time for gain/level/mix edits (fast enough to feel immediate).
const SMOOTH_SECONDS: f32 = 0.010;

/// Dynamic-bias envelope: fast enough to catch pick attack, slow enough that
/// the bloom rides the note instead of buzzing.
const ENV_ATTACK_SECONDS: f32 = 0.002;
const ENV_RELEASE_SECONDS: f32 = 0.060;

/// Stereo one-pole low-pass used as a phase-coherent band splitter:
/// `low = lp(x)`, `high = x - low` reconstructs exactly.
#[derive(Debug, Clone)]
struct StereoOnePoleLp {
    a: f32,
    y_l: f32,
    y_r: f32,
}

impl StereoOnePoleLp {
    fn new() -> Self {
        Self {
            a: 1.0,
            y_l: 0.0,
            y_r: 0.0,
        }
    }

    fn set(&mut self, freq: f32, sample_rate: f32) {
        let sr = sample_rate.max(1.0);
        let f = freq.clamp(1.0, sr * 0.45);
        self.a = 1.0 - (-std::f32::consts::TAU * f / sr).exp();
    }

    fn reset(&mut self) {
        self.y_l = 0.0;
        self.y_r = 0.0;
    }

    #[inline]
    fn run(&mut self, l: f32, r: f32) -> (f32, f32) {
        self.y_l += self.a * (l - self.y_l);
        self.y_r += self.a * (r - self.y_r);
        if !self.y_l.is_finite() {
            self.y_l = 0.0;
        }
        if !self.y_r.is_finite() {
            self.y_r = 0.0;
        }
        (self.y_l, self.y_r)
    }
}

#[derive(Debug, Clone)]
pub(super) struct Drive {
    sample_rate: f32,
    model: DriveModel,
    pre_gain: Smoothed,
    out_gain: Smoothed,
    mix: Smoothed,
    /// Clean low band level re-blended after the clipper (TS-style: lows pass
    /// un-clipped instead of being discarded or muddying the shaper).
    low_mix: Smoothed,
    /// How far the envelope pushes the shaper's operating point (per model).
    bias: f32,
    low_split: StereoOnePoleLp,
    mid_boost: StereoBiquad,
    tone_lpf: StereoBiquad,
    env: EnvFollower,
    dc: DcBlock,
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
            low_mix: Smoothed::new(sr, SMOOTH_SECONDS, 0.0),
            bias: 0.0,
            low_split: StereoOnePoleLp::new(),
            mid_boost: StereoBiquad::none(),
            tone_lpf: StereoBiquad::none(),
            env: EnvFollower::new(sr, ENV_ATTACK_SECONDS, ENV_RELEASE_SECONDS),
            dc: DcBlock::new(sr),
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
        self.low_mix.set_time(self.sample_rate, SMOOTH_SECONDS);
        self.env.set_sample_rate(self.sample_rate);
        self.dc.set_sample_rate(self.sample_rate);
        self.ds_one.set_sample_rate(self.sample_rate);
        self.super_drive.set_sample_rate(self.sample_rate);
        self.metal_core.set_sample_rate(self.sample_rate);
        self.tight_rift.set_sample_rate(self.sample_rate);
    }

    pub(super) fn reset(&mut self) {
        self.low_split.reset();
        self.mid_boost.reset();
        self.tone_lpf.reset();
        self.env.reset();
        self.dc.reset();
        self.oversampler.reset();
        self.pre_gain.snap();
        self.out_gain.snap();
        self.mix.snap();
        self.low_mix.snap();
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

        // (pre_gain, out_gain, low_mix, mix, bias) targets — the continuous
        // ones glide, `bias` steps with the (already discontinuous) model swap.
        // `low_mix` re-blends the clean low band post-clip: near-unity for
        // TS/Klon-style pedals whose lows famously pass un-clipped, low for
        // Rat, zero for Fuzz (a fuzz clips everything).
        let (pre, out, low, mix_amount, bias) = match model {
            DriveModel::Screamer => (1.0 + g * 18.0, 0.25 + lvl * 1.1, 0.85, 1.0, 0.10),
            DriveModel::Minotaur => (1.0 + g * 8.0, 0.4 + lvl * 1.3, 0.90, 0.85, 0.05),
            DriveModel::Rat => (1.0 + g * 30.0, 0.18 + lvl * 0.95, 0.35, 1.0, 0.16),
            DriveModel::Breaker => (1.0 + g * 10.0, 0.35 + lvl * 1.15, 0.60, 0.92, 0.12),
            DriveModel::Fuzz => (1.0 + g * 40.0, 0.15 + lvl * 0.85, 0.0, 1.0, 0.30),
            DriveModel::Centurion => (1.0 + g * 12.0, 0.32 + lvl * 1.2, 0.75, 0.88, 0.07),
            // Dedicated-topology models returned above.
            _ => (1.0, 1.0, 0.0, 1.0, 0.0),
        };
        self.pre_gain.set_target(pre);
        self.out_gain.set_target(out);
        self.low_mix.set_target(low);
        self.mix.set_target(mix_amount);
        self.bias = bias;

        // Split frequency (lows kept clean below it), pre-clip mid emphasis,
        // and post tone low-pass per model.
        match model {
            DriveModel::Screamer => {
                self.low_split.set(250.0, sr);
                self.mid_boost
                    .set(make_eq_biquad("bell", 720.0, 6.0, 0.7, sr));
                let cutoff = 2_400.0 + t * 5_600.0;
                self.tone_lpf.set(make_eq_biquad(
                    "lowpass",
                    cutoff.min(sr * 0.45),
                    0.0,
                    0.707,
                    sr,
                ));
            }
            DriveModel::Minotaur => {
                self.low_split.set(120.0, sr);
                self.mid_boost.set(None);
                let cutoff = (4_000.0 + t * 8_000.0).min(sr * 0.45);
                self.tone_lpf
                    .set(make_eq_biquad("lowpass", cutoff, 0.0, 0.707, sr));
            }
            DriveModel::Rat => {
                self.low_split.set(150.0, sr);
                self.mid_boost
                    .set(make_eq_biquad("bell", 1_100.0, 3.0, 0.9, sr));
                let cutoff = 1_200.0 + t * 6_300.0;
                self.tone_lpf.set(make_eq_biquad(
                    "lowpass",
                    cutoff.min(sr * 0.45),
                    0.0,
                    0.707,
                    sr,
                ));
            }
            DriveModel::Breaker => {
                self.low_split.set(110.0, sr);
                self.mid_boost
                    .set(make_eq_biquad("bell", 650.0, 2.5, 0.8, sr));
                let cutoff = 2_800.0 + t * 7_000.0;
                self.tone_lpf.set(make_eq_biquad(
                    "lowpass",
                    cutoff.min(sr * 0.45),
                    0.0,
                    0.707,
                    sr,
                ));
            }
            DriveModel::Fuzz => {
                self.low_split.set(60.0, sr);
                self.mid_boost
                    .set(make_eq_biquad("bell", 400.0, 4.0, 0.6, sr));
                let cutoff = 900.0 + t * 3_500.0;
                self.tone_lpf
                    .set(make_eq_biquad("lowpass", cutoff, 0.0, 0.707, sr));
            }
            DriveModel::Centurion => {
                self.low_split.set(130.0, sr);
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
        let low_mix = self.low_mix.tick();
        let mix_amount = self.mix.tick();
        let model = self.model;
        let bias = self.bias;

        // Phase-coherent band split: lows stay clean, highs feed the clipper.
        let (lo_l, lo_r) = self.low_split.run(left, right);
        let (hi_l, hi_r) = (left - lo_l, right - lo_r);
        // Voicing goes *into* the clipper (pre-emphasis), not after it.
        let (em_l, em_r) = self.mid_boost.run(hi_l, hi_r);
        let (dr_l, dr_r) = (em_l * pre, em_r * pre);
        // Envelope-driven bias: pick attack shifts the operating point, so
        // even harmonics bloom with dynamics instead of a static waveshape.
        let (env_l, env_r) = self.env.tick(dr_l, dr_r);
        let (bias_l, bias_r) = (bias * env_l.min(4.0), bias * env_r.min(4.0));
        // Waveshape at 2× rate: the shaper is memoryless, so only the
        // up/down half-band filters carry state across the doubled rate.
        let (sh_l, sh_r) = self.oversampler.process_stereo(dr_l, dr_r, |a, b| {
            (
                Self::shape(model, a + bias_l),
                Self::shape(model, b + bias_r),
            )
        });
        let (t_l, t_r) = self.tone_lpf.run(sh_l, sh_r);
        // The moving bias offset leaves a moving DC component — block it here.
        let (d_l, d_r) = self.dc.run(t_l, t_r);
        let wet_l = (d_l + lo_l * low_mix) * out;
        let wet_r = (d_r + lo_r * low_mix) * out;
        (mix(left, wet_l, mix_amount), mix(right, wet_r, mix_amount))
    }
}
