use crate::types::VoiceNote;

/// Converts a frequency in Hz to the nearest MIDI note number and cents offset.
pub fn hz_to_midi_cents(hz: f64) -> (u8, f64) {
    if hz <= 0.0 {
        return (0, 0.0);
    }
    // MIDI note formula: d = 12 * log2(freq / 440) + 69
    let midi_f = 12.0 * (hz / 440.0).log2() + 69.0;
    let midi = midi_f.round();
    let midi_u8 = midi.clamp(0.0, 127.0) as u8;
    let target_hz = midi_to_hz(midi_u8);
    let cents = 1200.0 * (hz / target_hz).log2();
    (midi_u8, cents)
}

/// Converts a MIDI note number to its corresponding frequency in Hz.
pub fn midi_to_hz(midi: u8) -> f64 {
    440.0 * 2.0f64.powf((midi as f64 - 69.0) / 12.0)
}

impl VoiceNote {
    /// Creates a new `VoiceNote` with the detected pitch, and computes MIDI note & cents offset automatically.
    pub fn new(
        id: String,
        start_time: f64,
        end_time: f64,
        detected_pitch_hz: f64,
        confidence: f64,
        gain: f32,
    ) -> Self {
        let (midi, cents) = hz_to_midi_cents(detected_pitch_hz);
        Self {
            id,
            start_time,
            end_time,
            detected_pitch_hz,
            detected_midi_note: midi,
            average_cents_offset: cents,
            confidence,
            gain,
            corrected_midi_note: midi,
            pitch_offset_cents: 0.0,
            timing_offset: 0.0,
            formant_shift: 0.0,
        }
    }

    /// Reset any manual edits back to the detected state.
    pub fn reset_edits(&mut self) {
        self.corrected_midi_note = self.detected_midi_note;
        self.pitch_offset_cents = 0.0;
        self.timing_offset = 0.0;
        self.formant_shift = 0.0;
    }
}
