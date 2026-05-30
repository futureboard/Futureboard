use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct PitchFrame {
    pub time_seconds: f64,
    pub frequency_hz: f64,
    pub confidence: f64,
    pub rms: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VoiceNote {
    pub id: String,
    pub start_time: f64,
    pub end_time: f64,
    pub detected_pitch_hz: f64,
    pub detected_midi_note: u8,
    pub average_cents_offset: f64,
    pub confidence: f64,
    pub gain: f32,
    pub corrected_midi_note: u8,
    pub pitch_offset_cents: f64,
    pub timing_offset: f64,
    pub formant_shift: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VoiceTuneAnalysisConfig {
    pub min_frequency: f64,
    pub max_frequency: f64,
    pub frame_size: usize,
    pub hop_size: usize,
    pub voiced_threshold: f64,
    pub rms_threshold: f64,
}

impl Default for VoiceTuneAnalysisConfig {
    fn default() -> Self {
        Self {
            min_frequency: 60.0,
            max_frequency: 800.0,
            frame_size: 2048,
            hop_size: 512,
            voiced_threshold: 0.35,
            rms_threshold: 0.005,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VoiceTuneDocument {
    pub sample_rate: u32,
    pub duration_seconds: f64,
    pub pitch_frames: Vec<PitchFrame>,
    pub notes: Vec<VoiceNote>,
    pub analysis_metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VoiceTuneRenderPlan {
    pub note_edits: Vec<VoiceNote>,
    pub correction_mode: String,
    pub offline_instructions: String,
}
