//! Amp-style stereo tremolo — LFO amplitude modulation with a shape control
//! that morphs the wave from smooth sine toward a choppy square.

use super::smooth::Smoothed;
use super::Lfo;

/// Glide time for depth/shape edits (see `smooth.rs`).
const SMOOTH_SECONDS: f32 = 0.010;

#[derive(Debug, Clone)]
pub(super) struct Tremolo {
    sample_rate: f32,
    lfo: Lfo,
    depth: Smoothed,
    /// 0 = pure sine, 1 = hard chop. Reuses the Mod slot's "mix" wire id.
    shape: Smoothed,
}

impl Tremolo {
    pub(super) fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        Self {
            sample_rate: sr,
            lfo: Lfo::new(),
            depth: Smoothed::new(sr, SMOOTH_SECONDS, 0.5),
            shape: Smoothed::new(sr, SMOOTH_SECONDS, 0.0),
        }
    }

    pub(super) fn set_sample_rate(&mut self, sample_rate: f32) {
        let sr = sample_rate.max(1.0);
        self.sample_rate = sr;
        self.depth.set_time(sr, SMOOTH_SECONDS);
        self.shape.set_time(sr, SMOOTH_SECONDS);
    }

    pub(super) fn reset(&mut self) {
        self.lfo.reset();
        self.depth.snap();
        self.shape.snap();
    }

    /// `rate` and `depth` are 0..10; `shape` is the Mod slot's 0..100 % knob.
    pub(super) fn configure(&mut self, rate: f32, depth: f32, shape: f32) {
        // 0.5 Hz → 12 Hz over the knob range.
        let rate_hz = 0.5 + (rate / 10.0).clamp(0.0, 1.0) * 11.5;
        self.lfo.set_rate(rate_hz, self.sample_rate);
        self.depth.set_target((depth / 10.0).clamp(0.0, 1.0));
        self.shape.set_target((shape / 100.0).clamp(0.0, 1.0));
    }

    #[inline]
    pub(super) fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        let depth = self.depth.tick();
        let shape = self.shape.tick();
        let sine = self.lfo.tick();
        // Shape morph: drive the sine harder and soft-clip it toward a square.
        // `tanh(4x)` at full shape is a rounded chop with no aliasing spray.
        let drive = 1.0 + shape * 7.0;
        let shaped = (sine * drive).tanh() / (drive.min(4.0)).tanh().max(1.0e-3);
        // Unipolar gain, 1 at LFO peak, (1 - depth) at trough.
        let gain = 1.0 - depth * 0.5 * (1.0 - shaped.clamp(-1.0, 1.0));
        (left * gain, right * gain)
    }
}
