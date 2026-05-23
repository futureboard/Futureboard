//! Per-track and master peak metering.

use serde::{Deserialize, Serialize};

use crate::buffer::AudioBuffer;
use crate::ids::TrackId;

/// Decay constant for meter smoothing.
/// At 60 FPS with 512-sample blocks at 44.1 kHz (~86 blocks/s),
/// this gives a visually smooth falloff.
const METER_DECAY: f32 = 0.93;

/// Single-channel peak meter with smoothed decay.
#[derive(Debug, Clone)]
pub struct PeakMeter {
    pub current: f32,
    pub peak_hold: f32,
    hold_countdown: u32,
}

impl Default for PeakMeter {
    fn default() -> Self {
        Self {
            current: 0.0,
            peak_hold: 0.0,
            hold_countdown: 0,
        }
    }
}

impl PeakMeter {
    /// Feed a new peak value. Applies decay to current, updates hold.
    pub fn update(&mut self, peak: f32) {
        // Instant attack, smooth release
        if peak >= self.current {
            self.current = peak;
        } else {
            self.current *= METER_DECAY;
            if self.current < 1e-6 {
                self.current = 0.0;
            }
        }

        // Peak hold with countdown (hold ~30 blocks ≈ 0.35s at typical rates)
        if peak > self.peak_hold {
            self.peak_hold = peak;
            self.hold_countdown = 30;
        } else if self.hold_countdown > 0 {
            self.hold_countdown -= 1;
        } else {
            self.peak_hold *= METER_DECAY;
            if self.peak_hold < 1e-6 {
                self.peak_hold = 0.0;
            }
        }
    }

    pub fn reset(&mut self) {
        self.current = 0.0;
        self.peak_hold = 0.0;
        self.hold_countdown = 0;
    }
}

/// Stereo meter pair.
#[derive(Debug, Clone, Default)]
pub struct StereoMeter {
    pub left: PeakMeter,
    pub right: PeakMeter,
}

impl StereoMeter {
    /// Update from an AudioBuffer (reads per-channel peaks).
    pub fn update_from_buffer(&mut self, buffer: &AudioBuffer) {
        let peaks = buffer.channel_peaks();
        if let Some(&l) = peaks.first() {
            self.left.update(l);
        }
        if let Some(&r) = peaks.get(1) {
            self.right.update(r);
        }
    }

    pub fn reset(&mut self) {
        self.left.reset();
        self.right.reset();
    }
}

/// Snapshot of meter values for event emission to UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeterSnapshot {
    pub track_id: TrackId,
    pub left: f32,
    pub right: f32,
    pub peak_left: f32,
    pub peak_right: f32,
}

impl StereoMeter {
    pub fn snapshot(&self, track_id: &TrackId) -> MeterSnapshot {
        MeterSnapshot {
            track_id: track_id.clone(),
            left: self.left.current,
            right: self.right.current,
            peak_left: self.left.peak_hold,
            peak_right: self.right.peak_hold,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meter_attack_is_instant() {
        let mut m = PeakMeter::default();
        m.update(0.8);
        assert_eq!(m.current, 0.8);
    }

    #[test]
    fn meter_decays_on_silence() {
        let mut m = PeakMeter::default();
        m.update(1.0);
        for _ in 0..100 {
            m.update(0.0);
        }
        assert!(m.current < 0.01, "Should have decayed: {}", m.current);
    }

    #[test]
    fn stereo_meter_from_buffer() {
        let mut buf = AudioBuffer::new(2, 4);
        buf.channel_mut(0).copy_from_slice(&[0.5, 0.8, 0.3, 0.1]);
        buf.channel_mut(1).copy_from_slice(&[0.2, 0.4, 0.6, 0.9]);

        let mut meter = StereoMeter::default();
        meter.update_from_buffer(&buf);
        assert_eq!(meter.left.current, 0.8);
        assert_eq!(meter.right.current, 0.9);
    }
}
