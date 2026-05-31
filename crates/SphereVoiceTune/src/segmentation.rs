use crate::note_model::hz_to_midi_cents;
use crate::types::{PitchFrame, VoiceNote};

/// Segments a list of pitch frames into discrete voice notes.
pub fn segment_notes(
    frames: &[PitchFrame],
    voiced_threshold: f64,
    rms_threshold: f64,
    min_note_duration: f64,
) -> Vec<VoiceNote> {
    let mut notes = Vec::new();
    let mut note_counter = 1;

    // 1. Group contiguous voiced frames into segments.
    let mut voiced_segments: Vec<Vec<&PitchFrame>> = Vec::new();
    let mut current_segment = Vec::new();

    for frame in frames {
        let is_voiced = frame.confidence >= voiced_threshold
            && frame.rms >= rms_threshold
            && frame.frequency_hz > 0.0;
        if is_voiced {
            current_segment.push(frame);
        } else {
            if !current_segment.is_empty() {
                voiced_segments.push(current_segment);
                current_segment = Vec::new();
            }
        }
    }
    if !current_segment.is_empty() {
        voiced_segments.push(current_segment);
    }

    // 2. Process each voiced segment, splitting it into separate notes if pitch changes.
    for segment in voiced_segments {
        if segment.is_empty() {
            continue;
        }

        // Convert frequency to MIDI values for smoothing.
        let raw_midi: Vec<f64> = segment
            .iter()
            .map(|f| {
                let (midi, cents) = hz_to_midi_cents(f.frequency_hz);
                midi as f64 + cents / 100.0
            })
            .collect();

        // Smooth the MIDI pitch curve using a 5-sample moving average window
        // to filter out jitter, vibrato, and transient glitches.
        let mut smoothed_midi = vec![0.0; raw_midi.len()];
        let window_size = 5;
        let half_win = window_size / 2;
        for i in 0..raw_midi.len() {
            let start = i.saturating_sub(half_win);
            let end = (i + half_win + 1).min(raw_midi.len());
            let sum: f64 = raw_midi[start..end].iter().sum();
            smoothed_midi[i] = sum / (end - start) as f64;
        }

        // Group adjacent frames by their rounded smoothed MIDI note value.
        let mut current_note_frames = Vec::new();
        let mut current_target_midi = smoothed_midi[0].round() as u8;

        for (idx, frame) in segment.iter().enumerate() {
            let frame_midi = smoothed_midi[idx].round() as u8;
            if frame_midi == current_target_midi {
                current_note_frames.push(*frame);
            } else {
                // MIDI note changed - finalize current note segment
                if !current_note_frames.is_empty() {
                    let note = create_voice_note_from_frames(
                        &format!("note_{}", note_counter),
                        &current_note_frames,
                    );
                    if (note.end_time - note.start_time) >= min_note_duration {
                        notes.push(note);
                        note_counter += 1;
                    }
                }
                current_note_frames = vec![*frame];
                current_target_midi = frame_midi;
            }
        }

        // Finalize last note segment in this voiced group.
        if !current_note_frames.is_empty() {
            let note = create_voice_note_from_frames(
                &format!("note_{}", note_counter),
                &current_note_frames,
            );
            if (note.end_time - note.start_time) >= min_note_duration {
                notes.push(note);
                note_counter += 1;
            }
        }
    }

    notes
}

fn create_voice_note_from_frames(id: &str, frames: &[&PitchFrame]) -> VoiceNote {
    let start_time = frames[0].time_seconds;
    let end_time = frames[frames.len() - 1].time_seconds;

    let count = frames.len() as f64;
    let mut sum_hz = 0.0;
    let mut sum_conf = 0.0;
    let mut sum_rms = 0.0;

    for f in frames {
        sum_hz += f.frequency_hz;
        sum_conf += f.confidence;
        sum_rms += f.rms;
    }

    let avg_hz = sum_hz / count;
    let avg_conf = sum_conf / count;
    let avg_rms = sum_rms / count;

    VoiceNote::new(
        id.to_string(),
        start_time,
        end_time,
        avg_hz,
        avg_conf,
        avg_rms as f32,
    )
}
