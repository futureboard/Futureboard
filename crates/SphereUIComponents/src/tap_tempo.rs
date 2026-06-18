//! Rolling-window tap tempo detection for the transport bar.
//!
//! Transient session state only — not serialized. Calculated BPM is applied
//! immediately through the project tempo update path (same as manual BPM edit).

use crate::components::BPM_MAX;
use crate::components::BPM_MIN;

/// Gap between taps that starts a fresh tap session.
pub const TAP_TEMPO_GAP_TIMEOUT_SECS: f64 = 2.5;

const MAX_TAPS: usize = 12;

/// Minimum interval count before outlier filtering runs.
const OUTLIER_MIN_INTERVALS: usize = 4;

/// Human-timing tap session with rolling timestamps.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TapTempo {
    taps: Vec<f64>,
    bpm: Option<f64>,
}

impl TapTempo {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn bpm(&self) -> Option<f64> {
        self.bpm
    }

    pub fn tap_count(&self) -> usize {
        self.taps.len()
    }

    /// Record a tap at `now_secs`. Returns calculated BPM once at least two
    /// taps exist in the current session (first tap returns `None`).
    pub fn tap(&mut self, now_secs: f64) -> Option<f64> {
        if let Some(&last) = self.taps.last() {
            if now_secs - last > TAP_TEMPO_GAP_TIMEOUT_SECS {
                self.reset();
            }
        }
        self.taps.push(now_secs);
        if self.taps.len() > MAX_TAPS {
            let overflow = self.taps.len() - MAX_TAPS;
            self.taps.drain(0..overflow);
        }
        self.bpm = self.compute_bpm();
        self.bpm
    }

    pub fn reset(&mut self) {
        self.taps.clear();
        self.bpm = None;
    }

    fn compute_bpm(&self) -> Option<f64> {
        if self.taps.len() < 2 {
            return None;
        }
        let mut intervals: Vec<f64> = self
            .taps
            .windows(2)
            .map(|window| window[1] - window[0])
            .filter(|dt| *dt > 0.0)
            .collect();
        if intervals.is_empty() {
            return None;
        }
        if intervals.len() >= OUTLIER_MIN_INTERVALS {
            intervals = filter_outlier_intervals(intervals);
            if intervals.is_empty() {
                return None;
            }
        }
        let avg = intervals.iter().sum::<f64>() / intervals.len() as f64;
        if avg <= 0.0 {
            return None;
        }
        Some((60.0 / avg).clamp(BPM_MIN as f64, BPM_MAX as f64))
    }
}

fn filter_outlier_intervals(intervals: Vec<f64>) -> Vec<f64> {
    let mut sorted = intervals.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = sorted[sorted.len() / 2];
    if median <= 0.0 {
        return intervals;
    }
    let low = median * 0.5;
    let high = median * 1.5;
    intervals
        .into_iter()
        .filter(|dt| *dt >= low && *dt <= high)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::BPM_MIN;

    #[test]
    fn first_tap_returns_no_bpm() {
        let mut tap = TapTempo::new();
        assert_eq!(tap.tap(0.0), None);
        assert_eq!(tap.tap_count(), 1);
    }

    #[test]
    fn two_taps_produce_expected_bpm() {
        let mut tap = TapTempo::new();
        assert_eq!(tap.tap(0.0), None);
        assert_eq!(tap.tap(0.5), Some(120.0));
    }

    #[test]
    fn multiple_taps_smooth_bpm() {
        let mut tap = TapTempo::new();
        tap.tap(0.0);
        tap.tap(0.5);
        tap.tap(1.0);
        tap.tap(1.5);
        let bpm = tap.bpm().expect("bpm");
        assert!((bpm - 120.0).abs() < 0.01);
    }

    #[test]
    fn timeout_resets_session() {
        let mut tap = TapTempo::new();
        tap.tap(0.0);
        tap.tap(0.5);
        assert_eq!(tap.bpm(), Some(120.0));
        assert_eq!(tap.tap(3.1), None);
        assert_eq!(tap.tap(3.6), Some(120.0));
    }

    #[test]
    fn bpm_is_clamped_to_range() {
        let mut fast = TapTempo::new();
        fast.tap(0.0);
        assert_eq!(fast.tap(0.05), Some(BPM_MAX as f64));

        let mut slow = TapTempo::new();
        slow.tap(0.0);
        assert_eq!(slow.tap(2.5), Some(24.0));

        assert_eq!(24.0_f64.clamp(BPM_MIN as f64, BPM_MAX as f64), 24.0);
    }

    #[test]
    fn reset_clears_state() {
        let mut tap = TapTempo::new();
        tap.tap(0.0);
        tap.tap(0.5);
        assert!(tap.bpm().is_some());
        tap.reset();
        assert!(tap.bpm().is_none());
        assert_eq!(tap.tap_count(), 0);
        assert_eq!(tap.tap(1.0), None);
    }

    #[test]
    fn outlier_smoothing_preserves_steady_timing() {
        let mut tap = TapTempo::new();
        tap.tap(0.0);
        tap.tap(0.5);
        tap.tap(1.0);
        tap.tap(1.5);
        tap.tap(2.3);
        let bpm = tap.bpm().expect("bpm");
        assert!((bpm - 120.0).abs() < 2.0, "expected ~120 BPM, got {bpm}");
    }
}
