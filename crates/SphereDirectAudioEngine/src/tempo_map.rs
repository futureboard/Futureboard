//! Beat/time conversion for transport and scheduling.
//!
//! Phase T foundation: static tempo plus step-hold tempo points. Full tempo-map
//! playback (clip stretch, tempo automation curves) is layered on later.

use serde::{Deserialize, Serialize};

/// Minimum/maximum project BPM (matches automation spec).
pub const BPM_MIN: f64 = 20.0;
pub const BPM_MAX: f64 = 999.0;

/// A tempo change anchored at a musical beat.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TempoPoint {
    pub beat: f64,
    pub bpm: f64,
}

/// Cached segment for O(log n) beat/time lookup without allocation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TempoSegment {
    pub start_beat: f64,
    pub end_beat: f64,
    pub start_seconds: f64,
    pub bpm: f64,
}

/// Runtime-ready tempo map snapshot (immutable, built off the audio thread).
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeTempoMapSnapshot {
    pub segments: Vec<TempoSegment>,
    /// Bumped whenever segments are rebuilt so caches can invalidate.
    pub revision: u64,
}

impl Default for RuntimeTempoMapSnapshot {
    fn default() -> Self {
        Self::static_tempo(120.0)
    }
}

impl RuntimeTempoMapSnapshot {
    pub fn static_tempo(bpm: f64) -> Self {
        TempoMap::static_tempo(bpm).into_snapshot()
    }

    pub fn bpm_at_beat(&self, beat: f64) -> f64 {
        segment_at_beat(&self.segments, beat).bpm
    }

    pub fn seconds_at_beat(&self, beat: f64) -> f64 {
        let beat = beat.max(0.0);
        let seg = segment_at_beat(&self.segments, beat);
        seg.start_seconds + (beat - seg.start_beat) * 60.0 / seg.bpm.max(BPM_MIN)
    }

    pub fn beat_at_seconds(&self, seconds: f64) -> f64 {
        beat_at_seconds_in_segments(&self.segments, seconds)
    }

    pub fn samples_at_beat(&self, beat: f64, sample_rate: f64) -> u64 {
        (self.seconds_at_beat(beat) * sample_rate.max(1.0))
            .round()
            .max(0.0) as u64
    }

    pub fn beat_at_samples(&self, samples: u64, sample_rate: f64) -> f64 {
        let seconds = samples as f64 / sample_rate.max(1.0);
        self.beat_at_seconds(seconds)
    }
}

/// Project tempo map with step-hold segments between tempo points.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TempoMap {
    /// Fallback BPM when `points` is empty.
    pub default_bpm: f64,
    #[serde(default)]
    pub points: Vec<TempoPoint>,
    #[serde(skip)]
    segments: Vec<TempoSegment>,
    #[serde(skip)]
    revision: u64,
}

impl TempoMap {
    pub fn static_tempo(bpm: f64) -> Self {
        let mut map = Self {
            default_bpm: clamp_bpm(bpm),
            points: Vec::new(),
            segments: Vec::new(),
            revision: 0,
        };
        map.rebuild_segments();
        map
    }

    pub fn from_points(default_bpm: f64, mut points: Vec<TempoPoint>) -> Self {
        points.sort_by(|a, b| {
            a.beat
                .partial_cmp(&b.beat)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        points.dedup_by(|a, b| (a.beat - b.beat).abs() < 1e-9);
        for point in &mut points {
            point.bpm = clamp_bpm(point.bpm);
        }
        let mut map = Self {
            default_bpm: clamp_bpm(default_bpm),
            points,
            segments: Vec::new(),
            revision: 0,
        };
        map.rebuild_segments();
        map
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn into_snapshot(self) -> RuntimeTempoMapSnapshot {
        RuntimeTempoMapSnapshot {
            segments: self.segments,
            revision: self.revision,
        }
    }

    pub fn snapshot(&self) -> RuntimeTempoMapSnapshot {
        RuntimeTempoMapSnapshot {
            segments: self.segments.clone(),
            revision: self.revision,
        }
    }

    pub fn segments(&self) -> &[TempoSegment] {
        &self.segments
    }

    pub fn tempo_at_beat(&self, beat: f64) -> f64 {
        self.bpm_at_beat(beat)
    }

    pub fn bpm_at_beat(&self, beat: f64) -> f64 {
        let beat = beat.max(0.0);
        if self.segments.is_empty() {
            return self.default_bpm;
        }
        segment_at_beat(&self.segments, beat).bpm
    }

    pub fn seconds_at_beat(&self, beat: f64) -> f64 {
        let beat = beat.max(0.0);
        if self.segments.is_empty() {
            return beat * 60.0 / self.default_bpm.max(BPM_MIN);
        }
        let seg = segment_at_beat(&self.segments, beat);
        seg.start_seconds + (beat - seg.start_beat) * 60.0 / seg.bpm.max(BPM_MIN)
    }

    pub fn beat_at_seconds(&self, seconds: f64) -> f64 {
        beat_at_seconds_in_segments(&self.segments, seconds)
    }

    pub fn samples_at_beat(&self, beat: f64, sample_rate: f64) -> u64 {
        (self.seconds_at_beat(beat) * sample_rate.max(1.0))
            .round()
            .max(0.0) as u64
    }

    pub fn beat_at_samples(&self, samples: u64, sample_rate: f64) -> f64 {
        let seconds = samples as f64 / sample_rate.max(1.0);
        self.beat_at_seconds(seconds)
    }

    fn rebuild_segments(&mut self) {
        self.segments.clear();
        let mut points: Vec<TempoPoint> = Vec::new();
        if self.points.is_empty() {
            points.push(TempoPoint {
                beat: 0.0,
                bpm: self.default_bpm,
            });
        } else {
            if self.points[0].beat > 0.0 {
                points.push(TempoPoint {
                    beat: 0.0,
                    bpm: self.default_bpm,
                });
            }
            points.extend(self.points.iter().cloned());
        }

        let mut start_seconds = 0.0;
        for (i, point) in points.iter().enumerate() {
            let end_beat = points
                .get(i + 1)
                .map(|next| next.beat)
                .unwrap_or(f64::INFINITY);
            self.segments.push(TempoSegment {
                start_beat: point.beat,
                end_beat,
                start_seconds,
                bpm: point.bpm,
            });
            if end_beat.is_finite() {
                start_seconds += (end_beat - point.beat) * 60.0 / point.bpm.max(BPM_MIN);
            }
        }
        self.revision = self.revision.wrapping_add(1);
    }
}

fn segment_at_beat(segments: &[TempoSegment], beat: f64) -> TempoSegment {
    if segments.is_empty() {
        return TempoSegment {
            start_beat: 0.0,
            end_beat: f64::INFINITY,
            start_seconds: 0.0,
            bpm: BPM_MIN,
        };
    }
    let idx = segments
        .partition_point(|seg| seg.start_beat <= beat)
        .saturating_sub(1);
    segments[idx.min(segments.len() - 1)]
}

fn beat_at_seconds_in_segments(segments: &[TempoSegment], seconds: f64) -> f64 {
    let seconds = seconds.max(0.0);
    if segments.is_empty() {
        return 0.0;
    }
    if seconds <= segments[0].start_seconds {
        return 0.0;
    }
    let idx = segments
        .partition_point(|seg| seg.start_seconds <= seconds)
        .saturating_sub(1);
    let seg = &segments[idx.min(segments.len() - 1)];
    let elapsed = seconds - seg.start_seconds;
    seg.start_beat + elapsed * seg.bpm.max(BPM_MIN) / 60.0
}

fn clamp_bpm(bpm: f64) -> f64 {
    bpm.clamp(BPM_MIN, BPM_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_tempo_conversions() {
        let map = TempoMap::static_tempo(120.0);
        assert!((map.tempo_at_beat(0.0) - 120.0).abs() < 1e-9);
        assert!((map.seconds_at_beat(2.0) - 1.0).abs() < 1e-9);
        assert!((map.beat_at_seconds(1.0) - 2.0).abs() < 1e-9);
        assert!((map.beat_at_seconds(0.0) - 0.0).abs() < 1e-9);
    }

    #[test]
    fn step_tempo_point_changes_bpm() {
        let map = TempoMap::from_points(
            120.0,
            vec![
                TempoPoint {
                    beat: 4.0,
                    bpm: 60.0,
                },
                TempoPoint {
                    beat: 8.0,
                    bpm: 240.0,
                },
            ],
        );
        assert!((map.tempo_at_beat(0.0) - 120.0).abs() < 1e-9);
        assert!((map.tempo_at_beat(4.0) - 60.0).abs() < 1e-9);
        assert!((map.tempo_at_beat(7.9) - 60.0).abs() < 1e-9);
        assert!((map.tempo_at_beat(8.0) - 240.0).abs() < 1e-9);

        // 4 beats @ 120 BPM = 2s, then 4 beats @ 60 BPM = 4s → beat 8 at 6s.
        assert!((map.seconds_at_beat(8.0) - 6.0).abs() < 1e-6);
        assert!((map.beat_at_seconds(6.0) - 8.0).abs() < 1e-6);
        assert!((map.beat_at_seconds(1.0) - 2.0).abs() < 1e-6);
    }

    fn map_120_160() -> TempoMap {
        TempoMap::from_points(
            120.0,
            vec![TempoPoint {
                beat: 4.0,
                bpm: 160.0,
            }],
        )
    }

    #[test]
    fn step_tempo_seconds_and_beats() {
        let map = map_120_160();
        assert!((map.seconds_at_beat(0.0) - 0.0).abs() < 1e-9);
        assert!((map.seconds_at_beat(4.0) - 2.0).abs() < 1e-9);
        assert!((map.seconds_at_beat(8.0) - 3.5).abs() < 1e-9);
        assert!((map.beat_at_seconds(2.0) - 4.0).abs() < 1e-9);
        assert!((map.beat_at_seconds(3.5) - 8.0).abs() < 1e-9);
    }

    #[test]
    fn step_tempo_sample_conversions() {
        let map = map_120_160();
        let sr = 48_000.0;
        assert_eq!(map.samples_at_beat(4.0, sr), 96_000);
        assert_eq!(map.samples_at_beat(8.0, sr), 168_000);
        assert!((map.beat_at_samples(96_000, sr) - 4.0).abs() < 1e-6);
        assert!((map.beat_at_samples(168_000, sr) - 8.0).abs() < 1e-6);
    }

    #[test]
    fn runtime_snapshot_matches_tempo_map() {
        let map = map_120_160();
        let snap = map.snapshot();
        assert_eq!(snap.samples_at_beat(4.0, 48_000.0), 96_000);
        assert_eq!(snap.samples_at_beat(8.0, 48_000.0), 168_000);
    }

    #[test]
    fn bpm_math_roundtrips_across_supported_sample_rates() {
        let bpm = 128.0;
        let map = TempoMap::static_tempo(bpm);
        for sr in [44_100.0, 48_000.0, 88_200.0, 96_000.0, 192_000.0] {
            assert!((map.tempo_at_beat(0.0) - bpm).abs() < 1e-9);
            let samples_per_beat = sr * 60.0 / bpm;
            assert_eq!(
                map.samples_at_beat(1.0, sr),
                samples_per_beat.round() as u64
            );
            let half_sample_ppq = bpm / (sr * 60.0) * 0.5;
            assert!(
                (map.beat_at_samples(samples_per_beat.round() as u64, sr) - 1.0).abs()
                    <= half_sample_ppq + 1e-12,
                "sr={sr}"
            );

            let ppq = 17.25;
            let sample = map.samples_at_beat(ppq, sr);
            let roundtrip = map.beat_at_samples(sample, sr);
            assert!(
                (roundtrip - ppq).abs() <= half_sample_ppq + 1e-12,
                "sr={sr} sample={sample} roundtrip={roundtrip}"
            );
        }

        assert_eq!(map.samples_at_beat(1.0, 48_000.0), 22_500);
        assert_eq!(map.samples_at_beat(1.0, 96_000.0), 45_000);
    }

    #[test]
    fn metronome_click_samples_follow_tempo_map() {
        let snap = map_120_160().snapshot();
        let sr = 48_000.0;
        let expected = [
            (0.0, 0_u64),
            (1.0, 24_000),
            (2.0, 48_000),
            (3.0, 72_000),
            (4.0, 96_000),
            (5.0, 114_000),
            (6.0, 132_000),
            (7.0, 150_000),
            (8.0, 168_000),
        ];
        for (beat, samples) in expected {
            assert_eq!(snap.samples_at_beat(beat, sr), samples, "beat {beat}");
        }
    }

    #[test]
    fn segments_are_sorted_and_cover_origin() {
        let map = TempoMap::from_points(
            100.0,
            vec![TempoPoint {
                beat: 2.0,
                bpm: 200.0,
            }],
        );
        let segs = map.segments();
        assert_eq!(segs.len(), 2);
        assert!((segs[0].start_beat - 0.0).abs() < 1e-9);
        assert!((segs[0].bpm - 100.0).abs() < 1e-9);
        assert!((segs[1].start_beat - 2.0).abs() < 1e-9);
        assert!((segs[1].bpm - 200.0).abs() < 1e-9);
    }
}
