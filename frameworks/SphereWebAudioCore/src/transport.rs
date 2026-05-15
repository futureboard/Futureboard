//! Transport state machine.
//!
//! Manages play/pause/stop, beat↔sample conversion, BPM, time signature,
//! loop region, and sample-accurate position advancement.

use serde::{Deserialize, Serialize};

/// Transport playback state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayState {
    Stopped,
    Playing,
    Paused,
}

/// Transport configuration and state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transport {
    /// Current playback state.
    pub state: PlayState,
    /// Current position in samples.
    pub sample_position: u64,
    /// Beats per minute.
    pub bpm: f64,
    /// Time signature numerator.
    pub time_sig_num: u32,
    /// Time signature denominator.
    pub time_sig_den: u32,
    /// Audio sample rate.
    pub sample_rate: f64,
    /// Loop enabled.
    pub loop_enabled: bool,
    /// Loop start position in beats.
    pub loop_start_beat: f64,
    /// Loop end position in beats.
    pub loop_end_beat: f64,
}

impl Transport {
    pub fn new(sample_rate: f64, bpm: f64) -> Self {
        Self {
            state: PlayState::Stopped,
            sample_position: 0,
            bpm,
            time_sig_num: 4,
            time_sig_den: 4,
            sample_rate,
            loop_enabled: false,
            loop_start_beat: 0.0,
            loop_end_beat: 4.0,
        }
    }

    // ── Conversions ──────────────────────────────────────────

    /// Convert a beat position to a sample position.
    pub fn beat_to_samples(&self, beat: f64) -> u64 {
        let seconds = beat * 60.0 / self.bpm;
        (seconds * self.sample_rate) as u64
    }

    /// Convert a sample position to a beat position.
    pub fn samples_to_beat(&self, samples: u64) -> f64 {
        let seconds = samples as f64 / self.sample_rate;
        seconds * self.bpm / 60.0
    }

    /// Current beat position.
    pub fn beat_position(&self) -> f64 {
        self.samples_to_beat(self.sample_position)
    }

    /// Current time in seconds.
    pub fn time_seconds(&self) -> f64 {
        self.sample_position as f64 / self.sample_rate
    }

    // ── State transitions ────────────────────────────────────

    pub fn play(&mut self) {
        self.state = PlayState::Playing;
    }

    pub fn pause(&mut self) {
        if self.state == PlayState::Playing {
            self.state = PlayState::Paused;
        }
    }

    pub fn stop(&mut self) {
        self.state = PlayState::Stopped;
        self.sample_position = 0;
    }

    pub fn seek_beat(&mut self, beat: f64) {
        self.sample_position = self.beat_to_samples(beat.max(0.0));
    }

    pub fn set_bpm(&mut self, bpm: f64) {
        // Preserve beat position across BPM change
        let current_beat = self.beat_position();
        self.bpm = bpm.clamp(20.0, 999.0);
        self.sample_position = self.beat_to_samples(current_beat);
    }

    pub fn set_loop(&mut self, enabled: bool, start_beat: f64, end_beat: f64) {
        self.loop_enabled = enabled;
        self.loop_start_beat = start_beat.max(0.0);
        self.loop_end_beat = end_beat.max(self.loop_start_beat + 0.001);
    }

    // ── Processing ───────────────────────────────────────────

    /// Advance transport by `frames` samples. Returns true if loop wrapped.
    /// Should be called once per process block.
    pub fn advance(&mut self, frames: usize) -> bool {
        if self.state != PlayState::Playing {
            return false;
        }

        self.sample_position += frames as u64;

        // Handle loop wrap
        if self.loop_enabled {
            let loop_end_sample = self.beat_to_samples(self.loop_end_beat);
            if self.sample_position >= loop_end_sample {
                let loop_start_sample = self.beat_to_samples(self.loop_start_beat);
                let loop_len = loop_end_sample.saturating_sub(loop_start_sample).max(1);
                let overshoot = self.sample_position - loop_end_sample;
                self.sample_position = loop_start_sample + (overshoot % loop_len);
                return true;
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beat_sample_conversion_roundtrip() {
        let t = Transport::new(44100.0, 120.0);
        // At 120 BPM, 1 beat = 0.5s = 22050 samples
        assert_eq!(t.beat_to_samples(1.0), 22050);
        assert!((t.samples_to_beat(22050) - 1.0).abs() < 1e-10);
    }

    #[test]
    fn play_stop_resets_position() {
        let mut t = Transport::new(44100.0, 120.0);
        t.play();
        t.advance(44100);
        assert!(t.sample_position > 0);
        t.stop();
        assert_eq!(t.sample_position, 0);
        assert_eq!(t.state, PlayState::Stopped);
    }

    #[test]
    fn seek_beat_sets_position() {
        let mut t = Transport::new(44100.0, 120.0);
        t.seek_beat(4.0);
        assert_eq!(t.sample_position, 22050 * 4);
    }

    #[test]
    fn loop_wraps_correctly() {
        let mut t = Transport::new(44100.0, 120.0);
        t.set_loop(true, 0.0, 1.0); // Loop 1 beat = 22050 samples
        t.play();
        // Advance past loop end
        let wrapped = t.advance(22050 + 100);
        assert!(wrapped);
        assert_eq!(t.sample_position, 100);
    }

    #[test]
    fn bpm_change_preserves_beat_position() {
        let mut t = Transport::new(44100.0, 120.0);
        t.seek_beat(4.0);
        let beat_before = t.beat_position();
        t.set_bpm(140.0);
        let beat_after = t.beat_position();
        assert!(
            (beat_before - beat_after).abs() < 0.01,
            "Beat position should be preserved: before={beat_before} after={beat_after}"
        );
    }

    #[test]
    fn no_advance_when_stopped() {
        let mut t = Transport::new(44100.0, 120.0);
        t.advance(1000);
        assert_eq!(t.sample_position, 0);
    }
}
