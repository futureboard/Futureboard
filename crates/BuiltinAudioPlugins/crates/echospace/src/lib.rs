//! EchoSpace — stereo / ping-pong delay with filtered feedback.
//!
//! Phase 2 (medium). Feedback tone controls use the MIT/Apache [`biquad`]
//! crate. Delay lines are preallocated ring buffers (no realtime heap growth).

use biquad::{Biquad, DirectForm1};
use builtin_dsp_core::{
    ParamDescriptor, PluginCategory, PluginDescriptor, StereoEffect, clamp, db_to_linear,
    make_eq_biquad, mix,
};

pub const PLUGIN_ID: &str = "futureboard.echospace";
const MAX_DELAY_MS: f32 = 4_000.0;
const MAX_DELAY_SAMPLES: usize = 192_000; // 4s @ 48k; clamped per sample-rate

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelayMode {
    Stereo,
    PingPong,
    Mono,
}

#[derive(Debug, Clone)]
pub struct Params {
    pub power: bool,
    pub mode: DelayMode,
    pub time_ms_l: f32,
    pub time_ms_r: f32,
    pub feedback: f32,
    pub cross_feedback: f32,
    pub low_cut_hz: f32,
    pub high_cut_hz: f32,
    pub saturation: f32,
    pub mix: f32,
    pub output_db: f32,
    pub freeze: bool,
}

pub fn default_params() -> Params {
    Params {
        power: true,
        mode: DelayMode::PingPong,
        time_ms_l: 375.0,
        time_ms_r: 563.0,
        feedback: 34.0,
        cross_feedback: 65.0,
        low_cut_hz: 180.0,
        high_cut_hz: 9_000.0,
        saturation: 8.0,
        mix: 20.0,
        output_db: 0.0,
        freeze: false,
    }
}

pub fn descriptor() -> PluginDescriptor {
    PluginDescriptor {
        id: PLUGIN_ID,
        name: "EchoSpace",
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
                id: "timeMsL",
                name: "Time L",
                default_value: 375.0,
                min: 1.0,
                max: MAX_DELAY_MS,
                unit: "ms",
            },
            ParamDescriptor {
                id: "timeMsR",
                name: "Time R",
                default_value: 563.0,
                min: 1.0,
                max: MAX_DELAY_MS,
                unit: "ms",
            },
            ParamDescriptor {
                id: "feedback",
                name: "Feedback",
                default_value: 34.0,
                min: 0.0,
                max: 98.0,
                unit: "%",
            },
            ParamDescriptor {
                id: "mix",
                name: "Mix",
                default_value: 20.0,
                min: 0.0,
                max: 100.0,
                unit: "%",
            },
        ],
    }
}

#[derive(Debug, Clone)]
struct DelayLine {
    buffer: Vec<f32>,
    write: usize,
}

impl DelayLine {
    fn new(capacity: usize) -> Self {
        Self {
            buffer: vec![0.0; capacity.max(1)],
            write: 0,
        }
    }

    fn clear(&mut self) {
        self.buffer.fill(0.0);
        self.write = 0;
    }

    #[inline]
    fn read(&self, delay_samples: usize) -> f32 {
        let len = self.buffer.len();
        let idx = (self.write + len - (delay_samples.min(len - 1))) % len;
        self.buffer[idx]
    }

    #[inline]
    fn write_sample(&mut self, sample: f32) {
        self.buffer[self.write] = sample;
        self.write += 1;
        if self.write >= self.buffer.len() {
            self.write = 0;
        }
    }
}

#[derive(Debug, Clone)]
pub struct Dsp {
    sample_rate: f32,
    params: Params,
    delay_l: DelayLine,
    delay_r: DelayLine,
    delay_samples_l: usize,
    delay_samples_r: usize,
    hpf_l: Option<DirectForm1<f32>>,
    hpf_r: Option<DirectForm1<f32>>,
    lpf_l: Option<DirectForm1<f32>>,
    lpf_r: Option<DirectForm1<f32>>,
    output_gain: f32,
}

impl Dsp {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        let capacity = ((sr * MAX_DELAY_MS * 0.001) as usize).clamp(1, MAX_DELAY_SAMPLES);
        let mut dsp = Self {
            sample_rate: sr,
            params: default_params(),
            delay_l: DelayLine::new(capacity),
            delay_r: DelayLine::new(capacity),
            delay_samples_l: 1,
            delay_samples_r: 1,
            hpf_l: None,
            hpf_r: None,
            lpf_l: None,
            lpf_r: None,
            output_gain: 1.0,
        };
        dsp.apply_params();
        dsp
    }

    pub fn params(&self) -> &Params {
        &self.params
    }

    pub fn set_params(&mut self, params: Params) {
        self.params = Params {
            power: params.power,
            mode: params.mode,
            time_ms_l: clamp(params.time_ms_l, 1.0, MAX_DELAY_MS),
            time_ms_r: clamp(params.time_ms_r, 1.0, MAX_DELAY_MS),
            feedback: clamp(params.feedback, 0.0, 98.0),
            cross_feedback: clamp(params.cross_feedback, 0.0, 100.0),
            low_cut_hz: clamp(params.low_cut_hz, 20.0, 2_000.0),
            high_cut_hz: clamp(params.high_cut_hz, 1_000.0, 20_000.0),
            saturation: clamp(params.saturation, 0.0, 100.0),
            mix: clamp(params.mix, 0.0, 100.0),
            output_db: clamp(params.output_db, -24.0, 12.0),
            freeze: params.freeze,
        };
        self.apply_params();
    }

    fn apply_params(&mut self) {
        self.delay_samples_l = ((self.params.time_ms_l * 0.001 * self.sample_rate) as usize).max(1);
        self.delay_samples_r = ((self.params.time_ms_r * 0.001 * self.sample_rate) as usize).max(1);
        self.output_gain = db_to_linear(self.params.output_db);

        let hpf = make_eq_biquad(
            "highpass",
            self.params.low_cut_hz,
            0.0,
            0.707,
            self.sample_rate,
        );
        self.hpf_l = hpf;
        self.hpf_r = hpf;
        let lpf = make_eq_biquad(
            "lowpass",
            self.params.high_cut_hz.min(self.sample_rate * 0.45),
            0.0,
            0.707,
            self.sample_rate,
        );
        self.lpf_l = lpf;
        self.lpf_r = lpf;
    }

    #[inline]
    fn saturate(sample: f32, amount: f32) -> f32 {
        if amount <= 0.0 {
            return sample;
        }
        let drive = 1.0 + amount * 6.0;
        (sample * drive).tanh() / drive.tanh().max(0.001)
    }

    #[inline]
    fn filter_feedback(
        sample: f32,
        hpf: &mut Option<DirectForm1<f32>>,
        lpf: &mut Option<DirectForm1<f32>>,
    ) -> f32 {
        let mut x = sample;
        if let Some(f) = hpf.as_mut() {
            x = f.run(x);
        }
        if let Some(f) = lpf.as_mut() {
            x = f.run(x);
        }
        x
    }
}

impl StereoEffect for Dsp {
    fn reset(&mut self) {
        self.delay_l.clear();
        self.delay_r.clear();
        if let Some(f) = self.hpf_l.as_mut() {
            f.reset_state();
        }
        if let Some(f) = self.hpf_r.as_mut() {
            f.reset_state();
        }
        if let Some(f) = self.lpf_l.as_mut() {
            f.reset_state();
        }
        if let Some(f) = self.lpf_r.as_mut() {
            f.reset_state();
        }
    }

    fn set_sample_rate(&mut self, sample_rate: f32) {
        let sr = sample_rate.max(1.0);
        let capacity = ((sr * MAX_DELAY_MS * 0.001) as usize).clamp(1, MAX_DELAY_SAMPLES);
        self.sample_rate = sr;
        self.delay_l = DelayLine::new(capacity);
        self.delay_r = DelayLine::new(capacity);
        self.apply_params();
    }

    fn process_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        if !self.params.power {
            return (left, right);
        }

        let delayed_l = self.delay_l.read(self.delay_samples_l);
        let delayed_r = self.delay_r.read(self.delay_samples_r);

        let fb = if self.params.freeze {
            1.0
        } else {
            self.params.feedback / 100.0
        };
        let xfb = self.params.cross_feedback / 100.0;
        let sat = self.params.saturation / 100.0;

        let (in_l, in_r) = match self.params.mode {
            DelayMode::Mono => {
                let m = (left + right) * 0.5;
                (m, m)
            }
            DelayMode::Stereo | DelayMode::PingPong => (left, right),
        };

        let mut fb_l = delayed_l * fb + delayed_r * xfb * fb;
        let mut fb_r = delayed_r * fb + delayed_l * xfb * fb;
        if self.params.mode == DelayMode::PingPong {
            // Swap cross paths for classic ping-pong bounce.
            std::mem::swap(&mut fb_l, &mut fb_r);
        }

        fb_l = Self::filter_feedback(fb_l, &mut self.hpf_l, &mut self.lpf_l);
        fb_r = Self::filter_feedback(fb_r, &mut self.hpf_r, &mut self.lpf_r);
        fb_l = Self::saturate(fb_l, sat);
        fb_r = Self::saturate(fb_r, sat);

        if !self.params.freeze {
            self.delay_l.write_sample(in_l + fb_l);
            self.delay_r.write_sample(in_r + fb_r);
        } else {
            self.delay_l.write_sample(fb_l);
            self.delay_r.write_sample(fb_r);
        }

        let wet_l = delayed_l * self.output_gain;
        let wet_r = delayed_r * self.output_gain;
        let amount = self.params.mix / 100.0;
        (mix(left, wet_l, amount), mix(right, wet_r, amount))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_produces_echo() {
        let mut dsp = Dsp::new(48_000.0);
        let mut params = default_params();
        params.time_ms_l = 10.0;
        params.time_ms_r = 10.0;
        params.feedback = 0.0;
        params.mix = 100.0;
        params.mode = DelayMode::Stereo;
        dsp.set_params(params);

        // Impulse
        let _ = dsp.process_stereo(1.0, 1.0);
        let mut heard = 0.0f32;
        for _ in 0..480 {
            let (l, _) = dsp.process_stereo(0.0, 0.0);
            heard = heard.max(l.abs());
        }
        assert!(heard > 0.1);
    }
}
