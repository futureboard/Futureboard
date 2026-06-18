use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StretchMode {
    Off,
    Manual,
    TempoSync,
    Warp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StretchAlgorithm {
    Off,
    RePitch,
    PreservePitch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StretchBackend {
    InternalRePitch,
    Signalsmith,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StretchParams {
    pub mode: StretchMode,
    pub algorithm: StretchAlgorithm,

    /// Timeline/display duration multiplier. `2.0` means the clip is twice as long.
    pub time_ratio: f32,

    /// Pitch multiplier. `1.0` means unchanged.
    pub pitch_ratio: f32,

    pub source_bpm: Option<f32>,
    pub target_bpm: Option<f32>,

    /// `true` = use Signalsmith preserve-pitch stretch.
    pub preserve_pitch: bool,

    /// Reserved for Signalsmith tuning.
    pub quality: f32,
}

impl Default for StretchParams {
    fn default() -> Self {
        Self {
            mode: StretchMode::Off,
            algorithm: StretchAlgorithm::Off,
            time_ratio: 1.0,
            pitch_ratio: 1.0,
            source_bpm: None,
            target_bpm: None,
            preserve_pitch: false,
            quality: 0.75,
        }
    }
}
