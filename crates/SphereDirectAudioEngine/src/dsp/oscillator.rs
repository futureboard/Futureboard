/// Band-limited sine oscillator.
///
/// Designed for the audio callback hot path:
///   - no allocation
///   - no branching on phase wraparound (subtraction)
///   - f64 phase for precision over long runs
///
/// Output amplitude is raw (±1.0); scale in the caller.
pub struct SineOscillator {
    phase: f64,
    phase_inc: f64, // freq / sample_rate
    sample_rate: f64,
}

impl SineOscillator {
    pub fn new(frequency: f64, sample_rate: f64) -> Self {
        let phase_inc = if sample_rate > 0.0 {
            frequency / sample_rate
        } else {
            0.0
        };
        Self {
            phase: 0.0,
            phase_inc,
            sample_rate,
        }
    }

    #[inline]
    pub fn next_sample(&mut self) -> f32 {
        let s = (self.phase * std::f64::consts::TAU).sin();
        self.phase += self.phase_inc;
        // Wrap without branching
        self.phase -= self.phase.floor();
        s as f32
    }

    pub fn set_frequency(&mut self, frequency: f64) {
        if self.sample_rate > 0.0 {
            self.phase_inc = frequency / self.sample_rate;
        }
    }

    pub fn reset(&mut self) {
        self.phase = 0.0;
    }
}
