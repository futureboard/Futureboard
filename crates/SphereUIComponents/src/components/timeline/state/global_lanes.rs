use super::*;

/// Global/system lanes rendered between the ruler and normal tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalLaneKind {
    Tempo,
    TimeSignature,
    SongText,
    Marker,
    Arranger,
}

/// Map a BPM value to a lane-local y coordinate (high BPM near the top).
pub fn bpm_to_y(bpm: f64, lane_height: f32, min_bpm: f64, max_bpm: f64) -> f32 {
    let pad = TEMPO_LANE_PAD;
    let usable = (lane_height - 2.0 * pad).max(1.0);
    let span = (max_bpm - min_bpm).max(1e-9);
    let t = ((bpm - min_bpm) / span).clamp(0.0, 1.0);
    pad + ((1.0 - t) as f32) * usable
}

/// Inverse of [`bpm_to_y`]: lane-local y → BPM.
pub fn y_to_bpm(y: f32, lane_height: f32, min_bpm: f64, max_bpm: f64) -> f64 {
    let pad = TEMPO_LANE_PAD;
    let usable = (lane_height - 2.0 * pad).max(1.0);
    let t = ((y - pad) / usable).clamp(0.0, 1.0);
    let span = (max_bpm - min_bpm).max(1e-9);
    (max_bpm - t as f64 * span).clamp(TEMPO_BPM_MIN, TEMPO_BPM_MAX)
}

impl TimelineState {
    pub fn global_lanes_height(&self) -> f32 {
        self.tempo_track_height()
            + self.time_signature_track_height()
            + crate::components::timeline::song_text_track::SONG_TEXT_LANE_HEIGHT
    }

    /// Visible global/system lanes (Tempo then Time Signature when shown).
    pub fn visible_global_lanes(&self) -> Vec<GlobalLaneKind> {
        let mut lanes = Vec::new();
        if self.show_tempo_track {
            lanes.push(GlobalLaneKind::Tempo);
        }
        if self.show_time_signature_track {
            lanes.push(GlobalLaneKind::TimeSignature);
        }
        lanes.push(GlobalLaneKind::SongText);
        lanes
    }
}
