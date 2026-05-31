use crate::types::{VoiceNote, VoiceTuneRenderPlan};

/// Generates a render plan from a list of edited VoiceNotes.
pub fn generate_render_plan(notes: &[VoiceNote], correction_mode: &str) -> VoiceTuneRenderPlan {
    let mut note_edits = Vec::new();
    for note in notes {
        // Detect if the note has any edits relative to its originally detected values
        let has_edits = note.corrected_midi_note != note.detected_midi_note
            || note.pitch_offset_cents != 0.0
            || note.timing_offset != 0.0
            || note.formant_shift != 0.0;

        if has_edits {
            note_edits.push(note.clone());
        }
    }

    let offline_instructions = format!(
        "Offline render plan: Apply pitch/time corrections to {} modified notes using mode '{}'.",
        note_edits.len(),
        correction_mode
    );

    VoiceTuneRenderPlan {
        note_edits,
        correction_mode: correction_mode.to_string(),
        offline_instructions,
    }
}
