use crate::types::{VoiceTuneAnalysisConfig, VoiceNote};
use crate::analysis::VoiceTuneAnalyzer;
use crate::pitch_detect::detect_pitch_yin;
use crate::note_model::{hz_to_midi_cents, midi_to_hz};
use crate::correction::update_note_correction;
use crate::render_plan::generate_render_plan;

fn generate_sine_wave(
    frequency: f64,
    sample_rate: u32,
    duration_seconds: f64,
    amplitude: f32,
) -> Vec<f32> {
    let num_samples = (sample_rate as f64 * duration_seconds) as usize;
    let mut samples = Vec::with_capacity(num_samples);
    for i in 0..num_samples {
        let t = i as f64 / sample_rate as f64;
        let sample = amplitude * (2.0 * std::f64::consts::PI * frequency * t).sin() as f32;
        samples.push(sample);
    }
    samples
}

#[test]
fn test_pitch_detection_sine_wave() {
    let sample_rate = 44100;
    let freq = 440.0; // A4
    // 2048 samples of pure A4 sine wave
    let samples = generate_sine_wave(freq, sample_rate, 2048.0 / sample_rate as f64, 0.8);

    let (detected_freq, confidence) = detect_pitch_yin(
        &samples,
        sample_rate,
        60.0,
        800.0,
        0.35,
    );

    assert!(confidence > 0.8, "Confidence should be high for pure sine wave");
    assert!((detected_freq - freq).abs() < 2.0, "Detected frequency {} should be close to 440.0", detected_freq);
}

#[test]
fn test_cents_offset_calculation() {
    // 440 Hz is exactly A4 (MIDI 69)
    let (midi_exact, cents_exact) = hz_to_midi_cents(440.0);
    assert_eq!(midi_exact, 69);
    assert!(cents_exact.abs() < 1e-5);

    // 450 Hz is slightly sharp of A4
    let (midi_sharp, cents_sharp) = hz_to_midi_cents(450.0);
    assert_eq!(midi_sharp, 69);
    assert!(cents_sharp > 0.0);

    // 430 Hz is slightly flat of A4
    let (midi_flat, cents_flat) = hz_to_midi_cents(430.0);
    assert_eq!(midi_flat, 69);
    assert!(cents_flat < 0.0);

    // Check MIDI to HZ roundtrip
    let hz_roundtrip = midi_to_hz(69);
    assert!((hz_roundtrip - 440.0).abs() < 1e-5);
}

#[test]
fn test_unvoiced_low_energy_regions_ignored() {
    let sample_rate = 44100;
    // Generate silent samples
    let silence = vec![0.0f32; 8192];

    let config = VoiceTuneAnalysisConfig::default();
    let doc = VoiceTuneAnalyzer::analyze_mono(&silence, sample_rate, &config).unwrap();

    assert!(doc.notes.is_empty(), "Silent buffer should produce zero notes");
    for frame in doc.pitch_frames {
        assert_eq!(frame.frequency_hz, 0.0, "Frequency should be 0.0 in silence");
        assert!(frame.confidence < config.voiced_threshold);
    }
}

#[test]
fn test_note_segmentation_stable_tones() {
    let sample_rate = 44100;
    // 0.4s A4 (440Hz), 0.2s silence, 0.4s C5 (523.25Hz)
    let part1 = generate_sine_wave(440.0, sample_rate, 0.4, 0.8);
    let silence = vec![0.0f32; (sample_rate as f64 * 0.2) as usize];
    let part2 = generate_sine_wave(523.25, sample_rate, 0.4, 0.8);

    let mut samples = Vec::new();
    samples.extend(part1);
    samples.extend(silence);
    samples.extend(part2);

    let config = VoiceTuneAnalysisConfig {
        frame_size: 2048,
        hop_size: 512,
        ..Default::default()
    };

    let doc = VoiceTuneAnalyzer::analyze_mono(&samples, sample_rate, &config).unwrap();

    assert_eq!(doc.notes.len(), 2, "Should segment into exactly 2 notes");

    let note1 = &doc.notes[0];
    assert_eq!(note1.detected_midi_note, 69, "First note should be A4 (69)");
    assert!(note1.start_time < 0.05);
    assert!((note1.end_time - 0.4).abs() < 0.1);

    let note2 = &doc.notes[1];
    assert_eq!(note2.detected_midi_note, 72, "Second note should be C5 (72)");
    assert!((note2.start_time - 0.6).abs() < 0.1);
    assert!((note2.end_time - 1.0).abs() < 0.1);
}

#[test]
fn test_correction_parameter_model() {
    let mut note = VoiceNote::new(
        "note_1".to_string(),
        0.0,
        1.0,
        440.0,
        0.9,
        0.5,
    );

    assert_eq!(note.corrected_midi_note, 69);
    assert_eq!(note.pitch_offset_cents, 0.0);

    // Apply correction edits
    update_note_correction(
        &mut note,
        Some(70),
        Some(15.0),
        Some(0.1),
        Some(1.2),
        Some(0.8),
    );

    assert_eq!(note.corrected_midi_note, 70);
    assert_eq!(note.pitch_offset_cents, 15.0);
    assert_eq!(note.timing_offset, 0.1);
    assert_eq!(note.formant_shift, 1.2);
    assert_eq!(note.gain, 0.8);

    // Reset edits
    note.reset_edits();
    assert_eq!(note.corrected_midi_note, 69);
    assert_eq!(note.pitch_offset_cents, 0.0);
    assert_eq!(note.timing_offset, 0.0);
    assert_eq!(note.formant_shift, 0.0);
}

#[test]
fn test_render_plan_generation() {
    let note1 = VoiceNote::new(
        "note_1".to_string(),
        0.0,
        1.0,
        440.0,
        0.9,
        0.5,
    );
    let mut note2 = VoiceNote::new(
        "note_2".to_string(),
        1.0,
        2.0,
        523.25,
        0.9,
        0.5,
    );

    // Edit note2
    note2.corrected_midi_note = 73;

    let notes = vec![note1, note2];
    let plan = generate_render_plan(&notes, "auto-tune");

    assert_eq!(plan.note_edits.len(), 1, "Only edited notes should be in the plan");
    assert_eq!(plan.note_edits[0].id, "note_2");
    assert_eq!(plan.correction_mode, "auto-tune");
    assert!(plan.offline_instructions.contains("1 modified notes"));
}
