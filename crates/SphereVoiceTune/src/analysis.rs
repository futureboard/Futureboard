use crate::error::VoiceTuneError;
use crate::pitch_detect::{calculate_rms, detect_pitch_yin};
use crate::segmentation::segment_notes;
use crate::types::{PitchFrame, VoiceTuneAnalysisConfig, VoiceTuneDocument};
use std::collections::HashMap;

pub struct VoiceTuneAnalyzer;

impl VoiceTuneAnalyzer {
    /// Analyzes a monophonic buffer of f32 samples and outputs a VoiceTuneDocument.
    pub fn analyze_mono(
        samples: &[f32],
        sample_rate: u32,
        config: &VoiceTuneAnalysisConfig,
    ) -> Result<VoiceTuneDocument, VoiceTuneError> {
        if sample_rate == 0 {
            return Err(VoiceTuneError::InvalidSampleRate(0));
        }
        if config.frame_size == 0 || config.hop_size == 0 {
            return Err(VoiceTuneError::InvalidParameters(
                "Frame size and hop size must be greater than zero".to_string(),
            ));
        }

        let total_samples = samples.len();
        let duration_seconds = total_samples as f64 / sample_rate as f64;
        let mut pitch_frames = Vec::new();

        let mut i = 0;
        while i < total_samples {
            // Extract frame with zero-padding if necessary
            let mut frame = vec![0.0f32; config.frame_size];
            let end = (i + config.frame_size).min(total_samples);
            let len = end - i;
            frame[..len].copy_from_slice(&samples[i..end]);

            let time_seconds = i as f64 / sample_rate as f64;
            let rms = calculate_rms(&frame);

            let (frequency_hz, confidence) = if rms >= config.rms_threshold {
                detect_pitch_yin(
                    &frame,
                    sample_rate,
                    config.min_frequency,
                    config.max_frequency,
                    config.voiced_threshold,
                )
            } else {
                (0.0, 0.0)
            };

            pitch_frames.push(PitchFrame {
                time_seconds,
                frequency_hz,
                confidence,
                rms,
            });

            i += config.hop_size;
        }

        // Default minimum note duration of 80ms (0.08 seconds)
        let min_note_duration = 0.08;
        let notes = segment_notes(
            &pitch_frames,
            config.voiced_threshold,
            config.rms_threshold,
            min_note_duration,
        );

        let mut analysis_metadata = HashMap::new();
        analysis_metadata.insert("pitch_detection_algorithm".to_string(), "YIN".to_string());
        analysis_metadata.insert("frame_size".to_string(), config.frame_size.to_string());
        analysis_metadata.insert("hop_size".to_string(), config.hop_size.to_string());
        analysis_metadata.insert("total_frames".to_string(), pitch_frames.len().to_string());

        Ok(VoiceTuneDocument {
            sample_rate,
            duration_seconds,
            pitch_frames,
            notes,
            analysis_metadata,
        })
    }
}
