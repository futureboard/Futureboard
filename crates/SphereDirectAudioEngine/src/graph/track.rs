/// In-engine mutable state for one mixer track.
///
/// Updated via the command queue so the audio callback can read atomics
/// without any locking.
#[derive(Debug, Clone)]
pub struct TrackState {
    pub id: String,
    pub volume: f32, // linear 0..2
    pub pan: f32,    // -1..1
    pub muted: bool,
    pub solo: bool,
}

impl TrackState {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            volume: 1.0,
            pan: 0.0,
            muted: false,
            solo: false,
        }
    }

    /// Effective gain accounting for mute.
    #[inline]
    pub fn effective_gain(&self) -> f32 {
        if self.muted {
            0.0
        } else {
            self.volume
        }
    }
}
