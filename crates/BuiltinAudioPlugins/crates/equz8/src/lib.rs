//! Equz8 — 8-band parametric EQ.
//!
//! Phase 1 (easy). Filter coefficients and runtime state use the MIT/Apache
//! [`biquad`] crate. No DirectAudioEngine dependency.

use biquad::{Biquad, DirectForm1};
use builtin_dsp_core::{
    clamp, db_to_linear, make_eq_biquad, mix, ParamDescriptor, PluginCategory, PluginDescriptor,
    StereoEffect,
};

pub const PLUGIN_ID: &str = "futureboard.equz8";
pub const BAND_COUNT: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BandType {
    HighPass,
    LowShelf,
    Bell,
    Notch,
    HighShelf,
    LowPass,
}

impl BandType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::HighPass => "highpass",
            Self::LowShelf => "lowshelf",
            Self::Bell => "bell",
            Self::Notch => "notch",
            Self::HighShelf => "highshelf",
            Self::LowPass => "lowpass",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "highpass" | "hp" => Some(Self::HighPass),
            "lowshelf" | "ls" => Some(Self::LowShelf),
            "bell" | "peak" | "peaking" => Some(Self::Bell),
            "notch" => Some(Self::Notch),
            "highshelf" | "hs" => Some(Self::HighShelf),
            "lowpass" | "lp" => Some(Self::LowPass),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BandParams {
    pub active: bool,
    pub band_type: BandType,
    pub freq: f32,
    pub gain_db: f32,
    pub q: f32,
}

#[derive(Debug, Clone)]
pub struct Params {
    pub power: bool,
    pub output_db: f32,
    pub mix: f32,
    pub bands: [BandParams; BAND_COUNT],
}

pub fn default_params() -> Params {
    Params {
        power: true,
        output_db: 0.0,
        mix: 100.0,
        bands: [
            BandParams {
                active: true,
                band_type: BandType::HighPass,
                freq: 50.0,
                gain_db: 0.0,
                q: 0.7,
            },
            BandParams {
                active: true,
                band_type: BandType::LowShelf,
                freq: 120.0,
                gain_db: 0.0,
                q: 0.8,
            },
            BandParams {
                active: true,
                band_type: BandType::Bell,
                freq: 250.0,
                gain_db: 2.5,
                q: 1.2,
            },
            BandParams {
                active: true,
                band_type: BandType::Bell,
                freq: 750.0,
                gain_db: -1.5,
                q: 1.4,
            },
            BandParams {
                active: true,
                band_type: BandType::Bell,
                freq: 1_500.0,
                gain_db: 1.0,
                q: 1.0,
            },
            BandParams {
                active: true,
                band_type: BandType::Bell,
                freq: 3_500.0,
                gain_db: 0.0,
                q: 1.1,
            },
            BandParams {
                active: true,
                band_type: BandType::HighShelf,
                freq: 8_000.0,
                gain_db: 1.5,
                q: 0.8,
            },
            BandParams {
                active: true,
                band_type: BandType::LowPass,
                freq: 16_000.0,
                gain_db: 0.0,
                q: 0.7,
            },
        ],
    }
}

pub fn descriptor() -> PluginDescriptor {
    PluginDescriptor {
        id: PLUGIN_ID,
        name: "Equz8",
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
                id: "mix",
                name: "Mix",
                default_value: 100.0,
                min: 0.0,
                max: 100.0,
                unit: "%",
            },
            ParamDescriptor {
                id: "outputDb",
                name: "Output",
                default_value: 0.0,
                min: -24.0,
                max: 12.0,
                unit: "dB",
            },
        ],
    }
}

#[derive(Debug, Clone)]
pub struct Dsp {
    sample_rate: f32,
    params: Params,
    left: [Option<DirectForm1<f32>>; BAND_COUNT],
    right: [Option<DirectForm1<f32>>; BAND_COUNT],
    output_gain: f32,
}

impl Dsp {
    pub fn new(sample_rate: f32) -> Self {
        let mut dsp = Self {
            sample_rate: sample_rate.max(1.0),
            params: default_params(),
            left: [None, None, None, None, None, None, None, None],
            right: [None, None, None, None, None, None, None, None],
            output_gain: 1.0,
        };
        dsp.rebuild();
        dsp
    }

    pub fn params(&self) -> &Params {
        &self.params
    }

    pub fn set_params(&mut self, params: Params) {
        self.params = params;
        self.params.output_db = clamp(self.params.output_db, -24.0, 12.0);
        self.params.mix = clamp(self.params.mix, 0.0, 100.0);
        for band in &mut self.params.bands {
            band.freq = clamp(band.freq, 20.0, 20_000.0);
            band.gain_db = clamp(band.gain_db, -18.0, 18.0);
            band.q = clamp(band.q, 0.1, 12.0);
        }
        self.rebuild();
    }

    fn rebuild(&mut self) {
        self.output_gain = db_to_linear(self.params.output_db);
        for i in 0..BAND_COUNT {
            let band = self.params.bands[i];
            if !band.active {
                self.left[i] = None;
                self.right[i] = None;
                continue;
            }
            let filter = make_eq_biquad(
                band.band_type.as_str(),
                band.freq,
                band.gain_db,
                band.q,
                self.sample_rate,
            );
            self.left[i] = filter;
            self.right[i] = filter;
        }
    }
}

impl StereoEffect for Dsp {
    fn reset(&mut self) {
        for filter in self.left.iter_mut().flatten() {
            filter.reset_state();
        }
        for filter in self.right.iter_mut().flatten() {
            filter.reset_state();
        }
    }

    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.rebuild();
    }

    fn process_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        if !self.params.power {
            return (left, right);
        }

        let mut wet_l = left;
        let mut wet_r = right;
        for filter in self.left.iter_mut().flatten() {
            wet_l = filter.run(wet_l);
        }
        for filter in self.right.iter_mut().flatten() {
            wet_r = filter.run(wet_r);
        }
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
    fn descriptor_id() {
        assert_eq!(descriptor().id, PLUGIN_ID);
    }

    #[test]
    fn bypass_when_power_off() {
        let mut dsp = Dsp::new(48_000.0);
        let mut params = default_params();
        params.power = false;
        dsp.set_params(params);
        assert_eq!(dsp.process_stereo(0.25, -0.25), (0.25, -0.25));
    }

    #[test]
    fn processes_without_nan() {
        let mut dsp = Dsp::new(48_000.0);
        let (l, r) = dsp.process_stereo(0.5, -0.5);
        assert!(l.is_finite() && r.is_finite());
    }
}
