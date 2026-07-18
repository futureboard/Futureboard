//! FA-2A — optical / program-dependent compressor (LA-2A-style).
//!
//! Phase 1 (easy). Parameter model mirrors `plugins/FB2AComp/Core`. Dynamics
//! use `SoftKneeCompressor` (sidechain HPF via [`biquad`]).

use builtin_dsp_core::{
    clamp, db_to_linear, mix, ParamDescriptor, PluginCategory, PluginDescriptor,
    SoftKneeCompressor, StereoEffect,
};

pub const PLUGIN_ID: &str = "futureboard.fa2a";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Compress,
    Limit,
}

#[derive(Debug, Clone)]
pub struct Params {
    pub power: bool,
    pub peak_reduction: f32,
    pub gain_db: f32,
    pub mode: Mode,
    pub emphasis: f32,
    pub mix: f32,
    pub color: f32,
    pub sidechain_low_cut_hz: f32,
    pub output_trim_db: f32,
}

pub fn default_params() -> Params {
    Params {
        power: true,
        peak_reduction: 35.0,
        gain_db: 0.0,
        mode: Mode::Compress,
        emphasis: 45.0,
        mix: 100.0,
        color: 12.0,
        sidechain_low_cut_hz: 90.0,
        output_trim_db: 0.0,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct OpticalModel {
    pub threshold_db: f32,
    pub ratio: f32,
    pub knee_db: f32,
    pub attack_sec: f32,
    pub release_sec: f32,
}

pub fn optical_model_from_params(params: &Params) -> OpticalModel {
    let amount = clamp(params.peak_reduction, 0.0, 100.0) / 100.0;
    let emphasis = clamp(params.emphasis, 0.0, 100.0) / 100.0;
    let sc_cut = clamp(params.sidechain_low_cut_hz, 20.0, 500.0);
    let sc_relief = ((sc_cut - 20.0) / 480.0) * 5.5;
    let emphasis_push = (emphasis - 0.5) * 7.0;
    let limit = params.mode == Mode::Limit;
    let threshold_db = clamp(
        peak_reduction_to_threshold_db(params.peak_reduction) - emphasis_push + sc_relief,
        -54.0,
        -3.0,
    );
    OpticalModel {
        threshold_db,
        ratio: if limit {
            12.0 + amount * 8.0
        } else {
            2.2 + amount * 1.6
        },
        knee_db: if limit {
            2.5 + (1.0 - amount) * 2.0
        } else {
            8.0 + (1.0 - amount) * 8.0
        },
        attack_sec: clamp(
            (if limit { 0.004 } else { 0.008 }) + (1.0 - amount) * 0.032 - emphasis * 0.003,
            0.002,
            0.07,
        ),
        release_sec: clamp(0.12 + amount * 0.68 + emphasis * 0.12, 0.08, 1.1),
    }
}

pub fn peak_reduction_to_threshold_db(peak_reduction: f32) -> f32 {
    let t = clamp(peak_reduction, 0.0, 100.0) / 100.0;
    -8.0 - t.powf(1.18) * 38.0
}

pub fn descriptor() -> PluginDescriptor {
    PluginDescriptor {
        id: PLUGIN_ID,
        name: "FA-2A",
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
                id: "peakReduction",
                name: "Peak Reduction",
                default_value: 35.0,
                min: 0.0,
                max: 100.0,
                unit: "%",
            },
            ParamDescriptor {
                id: "gainDb",
                name: "Gain",
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
            ParamDescriptor {
                id: "color",
                name: "Color",
                default_value: 12.0,
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
    output_gain: f32,
}

impl Dsp {
    pub fn new(sample_rate: f32) -> Self {
        let mut dsp = Self {
            params: default_params(),
            compressor: SoftKneeCompressor::new(sample_rate),
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
            peak_reduction: clamp(params.peak_reduction, 0.0, 100.0),
            gain_db: clamp(params.gain_db, -12.0, 24.0),
            mode: params.mode,
            emphasis: clamp(params.emphasis, 0.0, 100.0),
            mix: clamp(params.mix, 0.0, 100.0),
            color: clamp(params.color, 0.0, 100.0),
            sidechain_low_cut_hz: clamp(params.sidechain_low_cut_hz, 20.0, 500.0),
            output_trim_db: clamp(params.output_trim_db, -12.0, 12.0),
        };
        self.apply_params();
    }

    fn apply_params(&mut self) {
        let model = optical_model_from_params(&self.params);
        self.compressor.set_curve(
            model.threshold_db,
            model.ratio,
            model.knee_db,
            self.params.gain_db,
        );
        self.compressor
            .set_timing(model.attack_sec, model.release_sec);
        self.compressor
            .set_sidechain_hpf(self.params.sidechain_low_cut_hz);
        self.output_gain = db_to_linear(self.params.output_trim_db);
    }

    #[inline]
    fn apply_color(sample: f32, drive: f32) -> f32 {
        if drive <= 0.0 {
            return sample;
        }
        let amount = 1.0 + drive * 4.0;
        (sample * amount).tanh() / amount.tanh().max(0.001)
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
        let (mut wet_l, mut wet_r) = self.compressor.process_stereo_linked(left, right);
        let drive = self.params.color / 100.0;
        wet_l = Self::apply_color(wet_l, drive) * self.output_gain;
        wet_r = Self::apply_color(wet_r, drive) * self.output_gain;
        let amount = self.params.mix / 100.0;
        (mix(left, wet_l, amount), mix(right, wet_r, amount))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optical_model_limit_is_harder() {
        let mut compress = default_params();
        compress.mode = Mode::Compress;
        let mut limit = default_params();
        limit.mode = Mode::Limit;
        let c = optical_model_from_params(&compress);
        let l = optical_model_from_params(&limit);
        assert!(l.ratio > c.ratio);
        assert!(l.knee_db < c.knee_db);
    }

    #[test]
    fn processes_audio() {
        let mut dsp = Dsp::new(48_000.0);
        let (l, r) = dsp.process_stereo(0.8, -0.8);
        assert!(l.is_finite() && r.is_finite());
    }
}
