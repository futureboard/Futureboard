//! Cabinet simulation — an IR-free 4x12 voicing built from a short cascade of
//! biquads (low-cut, cabinet resonance bump, presence peak, speaker roll-off).
//! Mic position brightens the top; distance rolls it off and trims level.

use builtin_dsp_core::make_eq_biquad;

use super::smooth::Smoothed;
use super::{CabModel, StereoBiquad};

/// Glide time for the distance-derived level (see `smooth.rs`).
const SMOOTH_SECONDS: f32 = 0.010;

#[derive(Debug, Clone)]
pub(super) struct Cabinet {
    sample_rate: f32,
    hpf: StereoBiquad,
    body: StereoBiquad,
    presence: StereoBiquad,
    lpf: StereoBiquad,
    level: Smoothed,
}

impl Cabinet {
    pub(super) fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        Self {
            sample_rate: sr,
            hpf: StereoBiquad::none(),
            body: StereoBiquad::none(),
            presence: StereoBiquad::none(),
            lpf: StereoBiquad::none(),
            level: Smoothed::new(sr, SMOOTH_SECONDS, 1.0),
        }
    }

    pub(super) fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.level.set_time(self.sample_rate, SMOOTH_SECONDS);
    }

    pub(super) fn reset(&mut self) {
        self.hpf.reset();
        self.body.reset();
        self.presence.reset();
        self.lpf.reset();
        self.level.snap();
    }

    /// `mic` and `dist` are the editor's 0..100 % knobs.
    pub(super) fn configure(&mut self, model: CabModel, mic: f32, dist: f32) {
        let sr = self.sample_rate;
        let m = (mic / 100.0).clamp(0.0, 1.0); // brighter when closer to the cone
        let d = (dist / 100.0).clamp(0.0, 1.0); // darker/quieter with distance

        // Voicing constants per cabinet: (hpf_hz, body_hz, body_db, presence_hz,
        // presence_base_db, base_cutoff_hz).
        let (hpf_hz, body_hz, body_db, presence_hz, presence_base_db, base_cutoff) = match model {
            CabModel::Vintage4x12 => (80.0, 120.0, 3.0, 2_600.0, 1.0, 3_000.0),
            CabModel::American2x12 => (95.0, 150.0, 1.5, 3_200.0, 2.0, 4_200.0),
            CabModel::Tweed1x12 => (110.0, 180.0, 4.5, 2_000.0, 0.0, 2_400.0),
            CabModel::Modern4x12 => (70.0, 100.0, 2.0, 3_600.0, 2.5, 5_000.0),
        };

        // Fixed low-cut and cabinet body resonance.
        self.hpf
            .set(make_eq_biquad("highpass", hpf_hz, 0.0, 0.707, sr));
        self.body
            .set(make_eq_biquad("bell", body_hz, body_db, 0.9, sr));

        // Presence peak: emphasised on-axis, tamed off-axis.
        let presence_db = presence_base_db + m * 4.0 - d * 1.5;
        self.presence
            .set(make_eq_biquad("bell", presence_hz, presence_db, 1.1, sr));

        // Speaker roll-off: each cabinet falls off past its own knee.
        let cutoff = (base_cutoff + m * 2_500.0 - d * 1_200.0).clamp(2_000.0, sr * 0.45);
        self.lpf
            .set(make_eq_biquad("lowpass", cutoff, 0.0, 0.707, sr));

        self.level.set_target(1.0 - d * 0.2);
    }

    #[inline]
    pub(super) fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        let level = self.level.tick();
        let (mut l, mut r) = self.hpf.run(left, right);
        (l, r) = self.body.run(l, r);
        (l, r) = self.presence.run(l, r);
        (l, r) = self.lpf.run(l, r);
        (l * level, r * level)
    }
}
