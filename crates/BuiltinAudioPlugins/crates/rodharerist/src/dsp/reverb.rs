//! Studio plate reverb — a Freeverb-style bank of parallel comb filters into
//! series all-pass diffusers (the classic Schroeder/Moorer topology popularised
//! by Jezar's public-domain Freeverb). All buffers are preallocated.

use builtin_dsp_core::mix;

// Freeverb tunings in samples at 44.1 kHz; scaled to the runtime rate.
const COMB_TUNINGS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS_TUNINGS: [usize; 4] = [556, 441, 341, 225];
const STEREO_SPREAD: usize = 23;

const FIXED_GAIN: f32 = 0.015;
const SCALE_ROOM: f32 = 0.28;
const OFFSET_ROOM: f32 = 0.7;
const SCALE_DAMP: f32 = 0.4;
const ALLPASS_FEEDBACK: f32 = 0.5;
const DAMP: f32 = 0.28;

#[derive(Debug, Clone)]
struct Comb {
    buffer: Vec<f32>,
    index: usize,
    filter_store: f32,
    feedback: f32,
    damp1: f32,
    damp2: f32,
}

impl Comb {
    fn new(size: usize) -> Self {
        Self {
            buffer: vec![0.0; size.max(1)],
            index: 0,
            filter_store: 0.0,
            feedback: 0.5,
            damp1: DAMP * SCALE_DAMP,
            damp2: 1.0 - DAMP * SCALE_DAMP,
        }
    }

    fn clear(&mut self) {
        self.buffer.fill(0.0);
        self.filter_store = 0.0;
        self.index = 0;
    }

    #[inline]
    fn process(&mut self, input: f32) -> f32 {
        let output = self.buffer[self.index];
        self.filter_store = output * self.damp2 + self.filter_store * self.damp1;
        // Flush denormals to keep the tail cheap.
        if self.filter_store.abs() < 1.0e-18 {
            self.filter_store = 0.0;
        }
        self.buffer[self.index] = input + self.filter_store * self.feedback;
        self.index += 1;
        if self.index >= self.buffer.len() {
            self.index = 0;
        }
        output
    }
}

#[derive(Debug, Clone)]
struct Allpass {
    buffer: Vec<f32>,
    index: usize,
}

impl Allpass {
    fn new(size: usize) -> Self {
        Self {
            buffer: vec![0.0; size.max(1)],
            index: 0,
        }
    }

    fn clear(&mut self) {
        self.buffer.fill(0.0);
        self.index = 0;
    }

    #[inline]
    fn process(&mut self, input: f32) -> f32 {
        let buffered = self.buffer[self.index];
        let output = -input + buffered;
        self.buffer[self.index] = input + buffered * ALLPASS_FEEDBACK;
        self.index += 1;
        if self.index >= self.buffer.len() {
            self.index = 0;
        }
        output
    }
}

#[derive(Debug, Clone)]
pub(super) struct PlateReverb {
    sample_rate: f32,
    combs_l: Vec<Comb>,
    combs_r: Vec<Comb>,
    allpass_l: Vec<Allpass>,
    allpass_r: Vec<Allpass>,
    mix: f32,
}

impl PlateReverb {
    pub(super) fn new(sample_rate: f32) -> Self {
        let mut reverb = Self {
            sample_rate: sample_rate.max(1.0),
            combs_l: Vec::new(),
            combs_r: Vec::new(),
            allpass_l: Vec::new(),
            allpass_r: Vec::new(),
            mix: 0.55,
        };
        reverb.rebuild();
        reverb
    }

    pub(super) fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.rebuild();
    }

    pub(super) fn reset(&mut self) {
        for c in self.combs_l.iter_mut().chain(self.combs_r.iter_mut()) {
            c.clear();
        }
        for a in self.allpass_l.iter_mut().chain(self.allpass_r.iter_mut()) {
            a.clear();
        }
    }

    /// Allocate all comb/allpass buffers for the current sample rate.
    fn rebuild(&mut self) {
        let scale = self.sample_rate / 44_100.0;
        let s = |len: usize| ((len as f32 * scale) as usize).max(1);

        self.combs_l = COMB_TUNINGS.iter().map(|&t| Comb::new(s(t))).collect();
        self.combs_r = COMB_TUNINGS.iter().map(|&t| Comb::new(s(t + STEREO_SPREAD))).collect();
        self.allpass_l = ALLPASS_TUNINGS.iter().map(|&t| Allpass::new(s(t))).collect();
        self.allpass_r = ALLPASS_TUNINGS.iter().map(|&t| Allpass::new(s(t + STEREO_SPREAD))).collect();
    }

    /// `decay_s` 0.5..15, `mix` 0..100 %.
    pub(super) fn configure(&mut self, decay_s: f32, mix: f32) {
        let room = ((decay_s - 0.5) / 14.5).clamp(0.0, 1.0);
        let feedback = room * SCALE_ROOM + OFFSET_ROOM; // 0.70 .. 0.98
        for c in self.combs_l.iter_mut().chain(self.combs_r.iter_mut()) {
            c.feedback = feedback;
        }
        self.mix = (mix / 100.0).clamp(0.0, 1.0);
    }

    #[inline]
    pub(super) fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        let input = (left + right) * FIXED_GAIN;

        let mut wet_l = 0.0;
        for comb in &mut self.combs_l {
            wet_l += comb.process(input);
        }
        let mut wet_r = 0.0;
        for comb in &mut self.combs_r {
            wet_r += comb.process(input);
        }

        for allpass in &mut self.allpass_l {
            wet_l = allpass.process(wet_l);
        }
        for allpass in &mut self.allpass_r {
            wet_r = allpass.process(wet_r);
        }

        (mix(left, wet_l, self.mix), mix(right, wet_r, self.mix))
    }
}
