//! Noise gate — downward expander with fast open / slow close smoothing.

use builtin_dsp_core::{db_to_linear, time_constant};

#[derive(Debug, Clone)]
pub(super) struct NoiseGate {
    sample_rate: f32,
    threshold_lin: f32,
    envelope: f32,
    gain: f32,
    open_coeff: f32,
    close_coeff: f32,
}

impl NoiseGate {
    pub(super) fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        let mut gate = Self {
            sample_rate: sr,
            threshold_lin: db_to_linear(-55.0),
            envelope: 0.0,
            gain: 1.0,
            open_coeff: 0.0,
            close_coeff: 0.0,
        };
        gate.recompute_times();
        gate
    }

    pub(super) fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.recompute_times();
    }

    pub(super) fn set_threshold_db(&mut self, threshold_db: f32) {
        self.threshold_lin = db_to_linear(threshold_db);
    }

    pub(super) fn reset(&mut self) {
        self.envelope = 0.0;
        self.gain = 1.0;
    }

    fn recompute_times(&mut self) {
        // Snappy open (1 ms), musical close (120 ms) so decays are not chopped.
        self.open_coeff = time_constant(self.sample_rate, 0.001);
        self.close_coeff = time_constant(self.sample_rate, 0.120);
    }

    #[inline]
    pub(super) fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        // A fully-open threshold (-80 dB region) effectively disables the gate.
        if self.threshold_lin <= 1.0e-4 {
            return (left, right);
        }
        let detector = left.abs().max(right.abs());
        // Peak-hold-ish envelope (instant attack, slow release).
        self.envelope = if detector > self.envelope {
            detector
        } else {
            detector + (self.envelope - detector) * self.close_coeff
        };

        // Target gain: open above threshold, squared taper just below it.
        let target = if self.envelope >= self.threshold_lin {
            1.0
        } else {
            let ratio = self.envelope / self.threshold_lin;
            ratio * ratio
        };
        let coeff = if target > self.gain {
            self.open_coeff
        } else {
            self.close_coeff
        };
        self.gain = target + (self.gain - target) * coeff;
        (left * self.gain, right * self.gain)
    }
}
