//! Tape echo — interpolated delay with wow/flutter, tape saturation and a
//! band-limited feedback path (no realtime allocation; ring buffers are sized up
//! front for the maximum delay time).

use builtin_dsp_core::{make_eq_biquad, mix};

use super::{InterpDelay, Lfo, StereoBiquad, soft_clip};

const MAX_DELAY_MS: f32 = 1_300.0; // headroom above the 1200 ms max knob

#[derive(Debug, Clone)]
pub(super) struct TapeDelay {
    sample_rate: f32,
    line_l: InterpDelay,
    line_r: InterpDelay,
    delay_samples: f32,
    feedback: f32,
    mix: f32,
    flutter: Lfo,
    flutter_depth: f32,
    tone: StereoBiquad, // band-limits the feedback (tape head bump/roll-off)
    fb_l: f32,
    fb_r: f32,
}

impl TapeDelay {
    pub(super) fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        let capacity = ((sr * MAX_DELAY_MS * 0.001) as usize).max(4);
        let mut flutter = Lfo::new();
        flutter.set_rate(3.0, sr);
        Self {
            sample_rate: sr,
            line_l: InterpDelay::new(capacity),
            line_r: InterpDelay::new(capacity),
            delay_samples: 0.42 * sr,
            feedback: 0.35,
            mix: 0.3,
            flutter,
            flutter_depth: 0.0003 * sr,
            tone: StereoBiquad::none(),
            fb_l: 0.0,
            fb_r: 0.0,
        }
    }

    pub(super) fn set_sample_rate(&mut self, sample_rate: f32) {
        let sr = sample_rate.max(1.0);
        let capacity = ((sr * MAX_DELAY_MS * 0.001) as usize).max(4);
        self.sample_rate = sr;
        self.line_l = InterpDelay::new(capacity);
        self.line_r = InterpDelay::new(capacity);
        self.flutter.set_rate(3.0, sr);
        self.flutter_depth = 0.0003 * sr;
    }

    pub(super) fn reset(&mut self) {
        self.line_l.clear();
        self.line_r.clear();
        self.flutter.reset();
        self.tone.reset();
        self.fb_l = 0.0;
        self.fb_r = 0.0;
    }

    /// `time_ms` 40..1200, `fb` 0..100 %, `mix` 0..100 %.
    pub(super) fn configure(&mut self, time_ms: f32, fb: f32, mix: f32) {
        self.delay_samples = (time_ms.clamp(40.0, 1_200.0) * 0.001 * self.sample_rate).max(1.0);
        self.feedback = (fb / 100.0).clamp(0.0, 0.95);
        self.mix = (mix / 100.0).clamp(0.0, 1.0);
        // Warm tape tone in the feedback loop.
        self.tone.set(make_eq_biquad(
            "lowpass",
            4_000.0,
            0.0,
            0.707,
            self.sample_rate,
        ));
    }

    #[inline]
    pub(super) fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        // Wow/flutter: opposite modulation per channel for a little width.
        let flut = self.flutter.tick() * self.flutter_depth;
        let read_l = self.delay_samples + flut;
        let read_r = self.delay_samples - flut;

        let echo_l = self.line_l.read_interp(read_l);
        let echo_r = self.line_r.read_interp(read_r);

        // Filter + saturate the feedback (tape compression), then write in.
        let (fb_l, fb_r) = self.tone.run(echo_l, echo_r);
        self.fb_l = soft_clip(fb_l * self.feedback);
        self.fb_r = soft_clip(fb_r * self.feedback);

        self.line_l.write_sample(left + self.fb_l);
        self.line_r.write_sample(right + self.fb_r);

        (mix(left, echo_l, self.mix), mix(right, echo_r, self.mix))
    }
}
