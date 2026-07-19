//! 70s analog-style stereo chorus — dual modulated delay lines in quadrature.

use builtin_dsp_core::mix;

use super::{InterpDelay, Lfo};

/// Longest modulated delay we ever read (ms) → buffer capacity.
const MAX_DELAY_MS: f32 = 32.0;

#[derive(Debug, Clone)]
pub(super) struct Chorus {
    sample_rate: f32,
    line_l: InterpDelay,
    line_r: InterpDelay,
    lfo_l: Lfo,
    lfo_r: Lfo,
    base_samples: f32,
    depth_samples: f32,
    mix: f32,
}

impl Chorus {
    pub(super) fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        let capacity = ((sr * MAX_DELAY_MS * 0.001) as usize).max(4);
        let mut lfo_r = Lfo::new();
        lfo_r.set_phase(0.25); // 90° apart for stereo width
        Self {
            sample_rate: sr,
            line_l: InterpDelay::new(capacity),
            line_r: InterpDelay::new(capacity),
            lfo_l: Lfo::new(),
            lfo_r,
            base_samples: 0.010 * sr,
            depth_samples: 0.0,
            mix: 0.4,
        }
    }

    pub(super) fn set_sample_rate(&mut self, sample_rate: f32) {
        let sr = sample_rate.max(1.0);
        let capacity = ((sr * MAX_DELAY_MS * 0.001) as usize).max(4);
        self.sample_rate = sr;
        self.line_l = InterpDelay::new(capacity);
        self.line_r = InterpDelay::new(capacity);
    }

    pub(super) fn reset(&mut self) {
        self.line_l.clear();
        self.line_r.clear();
        self.lfo_l.reset();
        self.lfo_r.reset();
        self.lfo_r.set_phase(0.25);
    }

    /// `rate` and `depth` are 0..10; `mix` is 0..100 %.
    pub(super) fn configure(&mut self, rate: f32, depth: f32, mix: f32) {
        let sr = self.sample_rate;
        // 0.1 Hz → 6 Hz over the knob range.
        let rate_hz = 0.1 + (rate / 10.0).clamp(0.0, 1.0) * 5.9;
        self.lfo_l.set_rate(rate_hz, sr);
        self.lfo_r.set_rate(rate_hz, sr);
        // Base 10 ms, up to ±6 ms of modulation.
        self.base_samples = 0.010 * sr;
        self.depth_samples = (depth / 10.0).clamp(0.0, 1.0) * 0.006 * sr;
        self.mix = (mix / 100.0).clamp(0.0, 1.0);
    }

    #[inline]
    pub(super) fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        let mod_l = self.lfo_l.tick() * self.depth_samples;
        let mod_r = self.lfo_r.tick() * self.depth_samples;

        let wet_l = self.line_l.read_interp(self.base_samples + mod_l);
        let wet_r = self.line_r.read_interp(self.base_samples + mod_r);

        self.line_l.write_sample(left);
        self.line_r.write_sample(right);

        (mix(left, wet_l, self.mix), mix(right, wet_r, self.mix))
    }
}
