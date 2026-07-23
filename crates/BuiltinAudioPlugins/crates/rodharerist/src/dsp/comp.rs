//! Studio compressor stage — a thin voicing layer over the shared
//! [`SoftKneeCompressor`] (stereo-linked, soft-knee, sidechain HPF), so the
//! detector/gain math lives in `builtin_dsp_core` like the other reusable
//! blocks.

use builtin_dsp_core::SoftKneeCompressor;

/// Fixed sidechain high-pass so low-string energy doesn't pump the whole rig.
const SIDECHAIN_HPF_HZ: f32 = 80.0;
/// Fixed knee width — one less knob, always musical.
const KNEE_DB: f32 = 6.0;

#[derive(Debug, Clone)]
pub(super) struct CompStage {
    comp: SoftKneeCompressor,
    attack_sec: f32,
    release_sec: f32,
}

impl CompStage {
    pub(super) fn new(sample_rate: f32) -> Self {
        let mut comp = SoftKneeCompressor::new(sample_rate.max(1.0));
        comp.set_sidechain_hpf(SIDECHAIN_HPF_HZ);
        Self {
            comp,
            attack_sec: 0.010,
            release_sec: 0.120,
        }
    }

    pub(super) fn set_sample_rate(&mut self, sample_rate: f32) {
        self.comp.set_sample_rate(sample_rate.max(1.0));
        // The shared compressor does not re-derive envelope coefficients on a
        // rate change — re-apply the stored timing explicitly.
        self.comp.set_timing(self.attack_sec, self.release_sec);
        self.comp.set_sidechain_hpf(SIDECHAIN_HPF_HZ);
    }

    pub(super) fn reset(&mut self) {
        self.comp.reset();
    }

    /// Editor units: `thresh` −60..0 dB, `ratio` 1..20, `attack` 0.1..100 ms,
    /// `release` 10..1000 ms, `makeup` 0..24 dB.
    pub(super) fn configure(
        &mut self,
        thresh_db: f32,
        ratio: f32,
        attack_ms: f32,
        release_ms: f32,
        makeup_db: f32,
    ) {
        self.attack_sec = (attack_ms.clamp(0.1, 100.0)) * 0.001;
        self.release_sec = (release_ms.clamp(10.0, 1_000.0)) * 0.001;
        self.comp.set_timing(self.attack_sec, self.release_sec);
        self.comp.set_curve(
            thresh_db.clamp(-60.0, 0.0),
            ratio.clamp(1.0, 20.0),
            KNEE_DB,
            makeup_db.clamp(0.0, 24.0),
        );
    }

    /// Current gain reduction in positive dB, for a future GR meter.
    #[allow(dead_code)]
    pub(super) fn gain_reduction_db(&self) -> f32 {
        self.comp.gain_reduction_db()
    }

    #[inline]
    pub(super) fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        self.comp.process_stereo_linked(left, right)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_settings_are_near_transparent() {
        // Ratio 1:1 means no gain change regardless of threshold.
        let mut c = CompStage::new(48_000.0);
        c.configure(-24.0, 1.0, 10.0, 120.0, 0.0);
        let mut max_err: f32 = 0.0;
        for n in 0..8_000 {
            let x = (n as f32 * 0.01).sin() * 0.5;
            let (l, _) = c.process(x, x);
            if n > 4_000 {
                max_err = max_err.max((l - x).abs());
            }
        }
        assert!(max_err < 1.0e-3, "1:1 ratio not transparent: {max_err}");
    }

    #[test]
    fn heavy_settings_reduce_gain_and_stay_finite() {
        let mut c = CompStage::new(48_000.0);
        c.configure(-40.0, 20.0, 1.0, 50.0, 0.0);
        let mut peak: f32 = 0.0;
        for n in 0..24_000 {
            let x = (n as f32 * 0.02).sin() * 0.9;
            let (l, r) = c.process(x, x);
            assert!(l.is_finite() && r.is_finite());
            if n > 12_000 {
                peak = peak.max(l.abs());
            }
        }
        assert!(peak < 0.5, "20:1 at -40 dB should clamp 0.9 peaks: {peak}");
        assert!(c.gain_reduction_db() > 6.0);
    }
}
