pub mod analysis;
pub mod correction;
pub mod error;
pub mod formant;
pub mod note_model;
pub mod offline;
pub mod pitch_detect;
pub mod render_plan;
pub mod segmentation;
pub mod timing;
pub mod types;

#[cfg(test)]
mod tests;

pub use analysis::VoiceTuneAnalyzer;
pub use correction::{update_note_correction, VoiceCorrectionParams};
pub use error::VoiceTuneError;
pub use formant::FormantConfig;
pub use note_model::{hz_to_midi_cents, midi_to_hz};
pub use offline::OfflineVoiceRenderer;
pub use pitch_detect::{detect_pitch_yin, calculate_rms};
pub use render_plan::generate_render_plan;
pub use segmentation::segment_notes;
pub use timing::{apply_timing_offset, TimingCorrection};
pub use types::{
    PitchFrame, VoiceNote, VoiceTuneAnalysisConfig, VoiceTuneDocument, VoiceTuneRenderPlan,
};
