//! Classic 4-stage analog-style stereo phaser — swept first-order allpasses
//! with feedback, LFOs in quadrature for stereo width.

use builtin_dsp_core::mix;

use super::Lfo;
use super::smooth::Smoothed;

/// Glide time for depth/mix edits (see `smooth.rs`).
const SMOOTH_SECONDS: f32 = 0.010;

/// Number of cascaded allpass stages per channel (two notches).
const STAGES: usize = 4;

/// Fixed regeneration amount — enough for the vocal "swoosh" without ringing.
const FEEDBACK: f32 = 0.35;

/// Sweep bounds in Hz. The LFO moves the allpass corner between these,
/// scaled by the depth knob.
const SWEEP_LO_HZ: f32 = 220.0;
const SWEEP_HI_HZ: f32 = 3_200.0;

/// One channel: allpass state plus the feedback sample.
#[derive(Debug, Clone, Default)]
struct Channel {
    /// First-order allpass unit delays (x[n-1] shared form: keeps y[n-1]).
    z: [f32; STAGES],
    feedback: f32,
}

impl Channel {
    fn reset(&mut self) {
        self.z = [0.0; STAGES];
        self.feedback = 0.0;
    }

    /// Run the cascade with allpass coefficient `a` (same for all stages, the
    /// classic OTA/FET phaser topology).
    #[inline]
    fn run(&mut self, input: f32, a: f32) -> f32 {
        let mut x = input + self.feedback * FEEDBACK;
        for z in self.z.iter_mut() {
            // First-order allpass, transposed direct form:
            //   y[n] = a * x[n] + z ; z = x[n] - a * y[n]
            let y = a * x + *z;
            *z = x - a * y;
            x = y;
        }
        self.feedback = x;
        x
    }
}

#[derive(Debug, Clone)]
pub(super) struct Phaser {
    sample_rate: f32,
    lfo_l: Lfo,
    lfo_r: Lfo,
    left: Channel,
    right: Channel,
    depth: Smoothed,
    mix: Smoothed,
}

impl Phaser {
    pub(super) fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        let mut lfo_r = Lfo::new();
        lfo_r.set_phase(0.25); // 90° apart for stereo width
        Self {
            sample_rate: sr,
            lfo_l: Lfo::new(),
            lfo_r,
            left: Channel::default(),
            right: Channel::default(),
            depth: Smoothed::new(sr, SMOOTH_SECONDS, 0.5),
            mix: Smoothed::new(sr, SMOOTH_SECONDS, 0.5),
        }
    }

    pub(super) fn set_sample_rate(&mut self, sample_rate: f32) {
        let sr = sample_rate.max(1.0);
        self.sample_rate = sr;
        self.depth.set_time(sr, SMOOTH_SECONDS);
        self.mix.set_time(sr, SMOOTH_SECONDS);
    }

    pub(super) fn reset(&mut self) {
        self.left.reset();
        self.right.reset();
        self.lfo_l.reset();
        self.lfo_r.reset();
        self.lfo_r.set_phase(0.25);
        self.depth.snap();
        self.mix.snap();
    }

    /// `rate` and `depth` are 0..10; `mix` is 0..100 %.
    pub(super) fn configure(&mut self, rate: f32, depth: f32, mix: f32) {
        // 0.05 Hz → 8 Hz over the knob range, biased slow like a pedal.
        let t = (rate / 10.0).clamp(0.0, 1.0);
        let rate_hz = 0.05 + t * t * 7.95;
        self.lfo_l.set_rate(rate_hz, self.sample_rate);
        self.lfo_r.set_rate(rate_hz, self.sample_rate);
        self.depth.set_target((depth / 10.0).clamp(0.0, 1.0));
        self.mix.set_target((mix / 100.0).clamp(0.0, 1.0));
    }

    /// Allpass coefficient for a corner at `freq` Hz (bilinear-matched
    /// one-pole form, cheap and stable for freq << Nyquist).
    #[inline]
    fn coeff(&self, freq: f32) -> f32 {
        let w = (std::f32::consts::PI * freq / self.sample_rate).clamp(0.0, 1.4);
        // tan-free approximation: a = (1 - w) / (1 + w) tracks the corner
        // closely over the audio sweep range and never leaves (-1, 1).
        (1.0 - w) / (1.0 + w)
    }

    #[inline]
    pub(super) fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        let depth = self.depth.tick();
        let mix_amount = self.mix.tick();

        // LFO in [0,1], exponential-ish sweep between the bounds.
        let sweep_l = (self.lfo_l.tick() * 0.5 + 0.5) * depth;
        let sweep_r = (self.lfo_r.tick() * 0.5 + 0.5) * depth;
        let freq_l = SWEEP_LO_HZ * (SWEEP_HI_HZ / SWEEP_LO_HZ).powf(sweep_l);
        let freq_r = SWEEP_LO_HZ * (SWEEP_HI_HZ / SWEEP_LO_HZ).powf(sweep_r);

        let a_l = self.coeff(freq_l);
        let a_r = self.coeff(freq_r);

        let wet_l = self.left.run(left, a_l);
        let wet_r = self.right.run(right, a_r);

        (mix(left, wet_l, mix_amount), mix(right, wet_r, mix_amount))
    }
}
