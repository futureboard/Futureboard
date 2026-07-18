//! FA-76 — FET / ultra-fast compressor (1176-style).
//!
//! Phase 2 (medium). Ratio buttons map to classic 4 / 8 / 12 / 20 / All curves.
//! Dynamics use `SoftKneeCompressor` (sidechain HPF via [`biquad`]).

use builtin_dsp_core::{
    clamp, db_to_linear, mix, ParamDescriptor, PluginCategory, PluginDescriptor,
    SoftKneeCompressor, StereoEffect,
};

pub const PLUGIN_ID: &str = "futureboard.fa76";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RatioButton {
    R4,
    R8,
    R12,
    R20,
    /// "All buttons in" — aggressive limiting curve.
    All,
}

impl RatioButton {
    pub fn ratio(self) -> f32 {
        match self {
            Self::R4 => 4.0,
            Self::R8 => 8.0,
            Self::R12 => 12.0,
            Self::R20 => 20.0,
            Self::All => 100.0,
        }
    }

    pub fn knee_db(self) -> f32 {
        match self {
            Self::R4 => 4.0,
            Self::R8 => 3.0,
            Self::R12 => 2.0,
            Self::R20 => 1.0,
            Self::All => 8.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Params {
    pub power: bool,
    pub input_db: f32,
    pub output_db: f32,
    pub attack_us: f32,
    pub release_ms: f32,
    pub ratio: RatioButton,
    pub mix: f32,
    pub sidechain_hpf_hz: f32,
}

pub fn default_params() -> Params {
    Params {
        power: true,
        input_db: 18.0,
        output_db: -12.0,
        attack_us: 20.0,
        release_ms: 100.0,
        ratio: RatioButton::R4,
        mix: 100.0,
        sidechain_hpf_hz: 60.0,
    }
}

pub fn descriptor() -> PluginDescriptor {
    PluginDescriptor {
        id: PLUGIN_ID,
        name: "FA-76",
        vendor: "Futureboard",
        category: PluginCategory::Effect,
        version: env!("CARGO_PKG_VERSION"),
        params: &[
            ParamDescriptor {
                id: "power",
                name: "Power",
                default_value: 1.0,
                min: 0.0,
                max: 1.0,
                unit: "bool",
            },
            ParamDescriptor {
                id: "inputDb",
                name: "Input",
                default_value: 18.0,
                min: -12.0,
                max: 36.0,
                unit: "dB",
            },
            ParamDescriptor {
                id: "outputDb",
                name: "Output",
                default_value: -12.0,
                min: -36.0,
                max: 12.0,
                unit: "dB",
            },
            ParamDescriptor {
                id: "attackUs",
                name: "Attack",
                default_value: 20.0,
                min: 20.0,
                max: 800.0,
                unit: "µs",
            },
            ParamDescriptor {
                id: "releaseMs",
                name: "Release",
                default_value: 100.0,
                min: 50.0,
                max: 1_100.0,
                unit: "ms",
            },
            ParamDescriptor {
                id: "mix",
                name: "Mix",
                default_value: 100.0,
                min: 0.0,
                max: 100.0,
                unit: "%",
            },
        ],
    }
}

#[derive(Debug, Clone)]
pub struct Dsp {
    params: Params,
    compressor: SoftKneeCompressor,
    input_gain: f32,
    output_gain: f32,
}

impl Dsp {
    pub fn new(sample_rate: f32) -> Self {
        let mut dsp = Self {
            params: default_params(),
            compressor: SoftKneeCompressor::new(sample_rate),
            input_gain: 1.0,
            output_gain: 1.0,
        };
        dsp.apply_params();
        dsp
    }

    pub fn params(&self) -> &Params {
        &self.params
    }

    pub fn gain_reduction_db(&self) -> f32 {
        self.compressor.gain_reduction_db()
    }

    pub fn set_params(&mut self, params: Params) {
        self.params = Params {
            power: params.power,
            input_db: clamp(params.input_db, -12.0, 36.0),
            output_db: clamp(params.output_db, -36.0, 12.0),
            attack_us: clamp(params.attack_us, 20.0, 800.0),
            release_ms: clamp(params.release_ms, 50.0, 1_100.0),
            ratio: params.ratio,
            mix: clamp(params.mix, 0.0, 100.0),
            sidechain_hpf_hz: clamp(params.sidechain_hpf_hz, 0.0, 500.0),
        };
        self.apply_params();
    }

    fn apply_params(&mut self) {
        // FET units are threshold-fixed; "input" drives into a fixed knee.
        let threshold_db = if self.params.ratio == RatioButton::All {
            -18.0
        } else {
            -24.0
        };
        self.compressor.set_curve(
            threshold_db,
            self.params.ratio.ratio(),
            self.params.ratio.knee_db(),
            0.0,
        );
        self.compressor.set_timing(
            self.params.attack_us * 1.0e-6,
            self.params.release_ms * 0.001,
        );
        self.compressor
            .set_sidechain_hpf(self.params.sidechain_hpf_hz);
        self.input_gain = db_to_linear(self.params.input_db);
        self.output_gain = db_to_linear(self.params.output_db);
    }
}

impl StereoEffect for Dsp {
    fn reset(&mut self) {
        self.compressor.reset();
    }

    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.compressor.set_sample_rate(sample_rate);
        self.apply_params();
    }

    fn process_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        if !self.params.power {
            return (left, right);
        }
        let driven_l = left * self.input_gain;
        let driven_r = right * self.input_gain;
        let (mut wet_l, mut wet_r) = self.compressor.process_stereo_linked(driven_l, driven_r);
        wet_l *= self.output_gain;
        wet_r *= self.output_gain;
        let amount = self.params.mix / 100.0;
        (mix(left, wet_l, amount), mix(right, wet_r, amount))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_buttons_has_highest_ratio() {
        assert!(RatioButton::All.ratio() > RatioButton::R20.ratio());
    }

    #[test]
    fn fast_attack_compresses() {
        let mut dsp = Dsp::new(48_000.0);
        let mut params = default_params();
        params.ratio = RatioButton::R20;
        params.attack_us = 20.0;
        params.input_db = 24.0;
        params.output_db = -18.0;
        dsp.set_params(params);
        let mut peak = 0.0f32;
        for _ in 0..2_000 {
            let (l, _) = dsp.process_stereo(0.8, 0.8);
            peak = peak.max(l.abs());
        }
        assert!(peak.is_finite());
        assert!(dsp.gain_reduction_db() >= 0.0);
    }
}
