use crate::types::VoiceNote;

/// Correction parameters applied to notes.
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceCorrectionParams {
    pub pitch_center_correction: f64, // 0.0 to 1.0 (how close to snap to target MIDI note)
    pub pitch_drift_correction: f64,  // 0.0 to 1.0 (drift smoothing factor)
    pub vibrato_scale: f64,           // 0.0 to 2.0 (vibrato modulation depth scaling)
    pub timing_stretch: f64,          // time stretch factor
    pub formant_shift: f64,           // formant shifting in semitones
    pub gain_db: f32,                 // gain modification in dB
}

impl Default for VoiceCorrectionParams {
    fn default() -> Self {
        Self {
            pitch_center_correction: 1.0,
            pitch_drift_correction: 0.0,
            vibrato_scale: 1.0,
            timing_stretch: 1.0,
            formant_shift: 0.0,
            gain_db: 0.0,
        }
    }
}

/// Applies non-destructive correction parameters to a VoiceNote.
pub fn update_note_correction(
    note: &mut VoiceNote,
    corrected_midi_note: Option<u8>,
    pitch_offset_cents: Option<f64>,
    timing_offset: Option<f64>,
    formant_shift: Option<f64>,
    gain: Option<f32>,
) {
    if let Some(midi) = corrected_midi_note {
        note.corrected_midi_note = midi;
    }
    if let Some(cents) = pitch_offset_cents {
        note.pitch_offset_cents = cents;
    }
    if let Some(timing) = timing_offset {
        note.timing_offset = timing;
    }
    if let Some(formant) = formant_shift {
        note.formant_shift = formant;
    }
    if let Some(g) = gain {
        note.gain = g;
    }
}
