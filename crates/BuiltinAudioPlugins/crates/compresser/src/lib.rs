//! Compresser — soft-knee VCA compressor.
//!
//! Phase 1 (easy). Dynamics use `builtin_dsp_core::SoftKneeCompressor`, which
//! builds its optional sidechain HPF with the MIT/Apache [`biquad`] crate.

use builtin_dsp_core::{
    clamp, mix, ParamDescriptor, PluginCategory, PluginDescriptor, SoftKneeCompressor, StereoEffect,
};

pub const PLUGIN_ID: &str = "futureboard.compresser";

#[derive(Debug, Clone)]
pub struct Params {
    pub power: bool,
    pub threshold_db: f32,
    pub ratio: f32,
    pub knee_db: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub makeup_db: f32,
    pub mix: f32,
    pub sidechain_hpf_hz: f32,
}

pub fn default_params() -> Params {
    Params {
        power: true,
        threshold_db: -18.0,
        ratio: 4.0,
        knee_db: 6.0,
        attack_ms: 10.0,
        release_ms: 100.0,
        makeup_db: 0.0,
        mix: 100.0,
        sidechain_hpf_hz: 80.0,
    }
}

pub fn descriptor() -> PluginDescriptor {
    PluginDescriptor {
        id: PLUGIN_ID,
        name: "Compresser",
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
                id: "thresholdDb",
                name: "Threshold",
                default_value: -18.0,
                min: -60.0,
                max: 0.0,
                unit: "dB",
            },
            ParamDescriptor {
                id: "ratio",
                name: "Ratio",
                default_value: 4.0,
                min: 1.0,
                max: 20.0,
                unit: ":1",
            },
            ParamDescriptor {
                id: "attackMs",
                name: "Attack",
                default_value: 10.0,
                min: 0.1,
                max: 100.0,
                unit: "ms",
            },
            ParamDescriptor {
                id: "releaseMs",
                name: "Release",
                default_value: 100.0,
                min: 10.0,
                max: 2_000.0,
                unit: "ms",
            },
            ParamDescriptor {
                id: "makeupDb",
                name: "Makeup",
                default_value: 0.0,
                min: -12.0,
                max: 24.0,
                unit: "dB",
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
}

impl Dsp {
    pub fn new(sample_rate: f32) -> Self {
        let mut dsp = Self {
            params: default_params(),
            compressor: SoftKneeCompressor::new(sample_rate),
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
            threshold_db: clamp(params.threshold_db, -60.0, 0.0),
            ratio: clamp(params.ratio, 1.0, 20.0),
            knee_db: clamp(params.knee_db, 0.0, 24.0),
            attack_ms: clamp(params.attack_ms, 0.1, 100.0),
            release_ms: clamp(params.release_ms, 10.0, 2_000.0),
            makeup_db: clamp(params.makeup_db, -12.0, 24.0),
            mix: clamp(params.mix, 0.0, 100.0),
            sidechain_hpf_hz: clamp(params.sidechain_hpf_hz, 0.0, 500.0),
        };
        self.apply_params();
    }

    fn apply_params(&mut self) {
        self.compressor.set_curve(
            self.params.threshold_db,
            self.params.ratio,
            self.params.knee_db,
            self.params.makeup_db,
        );
        self.compressor.set_timing(
            self.params.attack_ms * 0.001,
            self.params.release_ms * 0.001,
        );
        self.compressor
            .set_sidechain_hpf(self.params.sidechain_hpf_hz);
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
        let (wet_l, wet_r) = self.compressor.process_stereo_linked(left, right);
        let amount = self.params.mix / 100.0;
        (mix(left, wet_l, amount), mix(right, wet_r, amount))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compresses_hot_signal() {
        let mut dsp = Dsp::new(48_000.0);
        let mut params = default_params();
        params.threshold_db = -24.0;
        params.ratio = 8.0;
        params.attack_ms = 1.0;
        params.release_ms = 50.0;
        params.makeup_db = 0.0;
        dsp.set_params(params);

        for _ in 0..4_000 {
            let _ = dsp.process_stereo(0.95, 0.95);
        }
        let mut peak_out = 0.0f32;
        for _ in 0..256 {
            let (l, _) = dsp.process_stereo(0.95, 0.95);
            peak_out = peak_out.max(l.abs());
        }
        assert!(peak_out < 0.95 * 0.95);
        assert!(dsp.gain_reduction_db() > 1.0);
    }
}
