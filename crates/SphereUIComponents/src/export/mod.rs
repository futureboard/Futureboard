//! Arrangement export UI: settings model, the external export window, and the
//! background export job. Rendering/encoding lives in the engine + SphereEncoder;
//! this layer collects settings, builds a plain request + snapshot, and drives a
//! cancellable background job without holding any GPUI borrow during the work.

mod export_settings;
mod export_window;

pub use export_settings::{
    ExportChannelMode, ExportNormalizeChoice, ExportProjectDefaults, ExportRangeChoice,
    ExportSampleRateChoice, ExportSettings, ExportSettingsError, ExportTailChoice,
};
pub use export_window::{
    open_export_arrangement_window, ExportArrangementWindow, ExportJobState, EXPORT_WINDOW_WIDTH,
};

#[cfg(test)]
mod tests {
    use super::*;
    use sphere_encoder::{AudioFileFormat, AudioSampleFormat};
    use DAUx::types::{EngineProjectSnapshot, EngineRoutingSnapshot, EngineTrackSnapshot};

    fn snapshot_with_content(end_beat: f64) -> EngineProjectSnapshot {
        use DAUx::types::EngineClipSnapshot;
        let clips = if end_beat > 0.0 {
            vec![EngineClipSnapshot {
                id: "clip-1".to_string(),
                track_id: "track-1".to_string(),
                asset_id: "asset-1".to_string(),
                media_path: None,
                start_beat: 0.0,
                duration_beats: end_beat,
                offset_seconds: 0.0,
                gain: 1.0,
                muted: false,
                fades: None,
                audio_process: None,
            }]
        } else {
            Vec::new()
        };
        EngineProjectSnapshot {
            project_id: "p".to_string(),
            project_root: None,
            preferred_input_device: None,
            bpm: 120.0,
            tempo_points: Vec::new(),
            time_signature: [4, 4],
            sample_rate: 48_000,
            tracks: vec![EngineTrackSnapshot {
                id: "track-1".to_string(),
                track_type: "audio".to_string(),
                volume: 1.0,
                pan: 0.0,
                muted: false,
                solo: false,
                armed: false,
                input_monitor: false,
                input_source: Default::default(),
                preview_mode: "stereo".to_string(),
                output_track_id: None,
                inserts: Vec::new(),
                sends: Vec::new(),
                automation_lanes: Vec::new(),
            }],
            clips,
            midi_clips: Vec::new(),
            routing: EngineRoutingSnapshot {
                master_output_device: None,
                sample_rate: 48_000,
                buffer_size: 512,
            },
        }
    }

    fn defaults() -> ExportProjectDefaults {
        ExportProjectDefaults {
            project_sample_rate: 48_000,
            master_volume: 1.0,
            content_end_beat: 4.0,
            time_selection: None,
            loop_range: None,
            mp3_available: false,
        }
    }

    fn valid_wav() -> ExportSettings {
        let mut s = ExportSettings::default();
        s.output_path = Some(std::env::temp_dir().join("fb-export-test.wav"));
        s
    }

    #[test]
    fn valid_wav_settings_pass() {
        assert!(valid_wav().validate(&defaults()).is_ok());
    }

    #[test]
    fn missing_output_path_fails() {
        let s = ExportSettings::default();
        assert_eq!(
            s.validate(&defaults()),
            Err(ExportSettingsError::NoOutputPath)
        );
    }

    #[test]
    fn mp3_disabled_fails_cleanly() {
        let mut s = valid_wav();
        s.format = AudioFileFormat::Mp3;
        s.output_path = Some(std::env::temp_dir().join("fb-export-test.mp3"));
        assert_eq!(
            s.validate(&defaults()),
            Err(ExportSettingsError::Mp3Unavailable)
        );
    }

    #[test]
    fn mp3_enabled_passes_validation() {
        let mut d = defaults();
        d.mp3_available = true;
        let mut s = valid_wav();
        s.format = AudioFileFormat::Mp3;
        s.sample_rate = ExportSampleRateChoice::Hz48000;
        assert!(s.validate(&d).is_ok());
    }

    #[test]
    fn extension_follows_format_deterministically() {
        let mut s = valid_wav();
        s.format = AudioFileFormat::Flac;
        let path = s.normalized_output_path().unwrap();
        assert_eq!(path.extension().unwrap(), "flac");
    }

    #[test]
    fn entire_arrangement_resolves_from_content_bounds() {
        let snapshot = snapshot_with_content(4.0); // 4 beats @ 120bpm = 2.0s
        let req = valid_wav().to_request(&snapshot, &defaults()).unwrap();
        // 2 seconds at 48k = 96000 frames.
        assert_eq!(req.render.start_sample, 0);
        assert_eq!(req.render.end_sample, 96_000);
        assert_eq!(req.render.sample_rate, 48_000);
        assert_eq!(req.render.channels, 2);
    }

    #[test]
    fn empty_arrangement_reports_no_content() {
        let snapshot = snapshot_with_content(0.0);
        let result = valid_wav().to_request(&snapshot, &defaults());
        assert_eq!(result.err(), Some(ExportSettingsError::NoContent));
    }

    #[test]
    fn custom_range_converts_beats_to_samples() {
        let snapshot = snapshot_with_content(16.0);
        let mut s = valid_wav();
        // beats 4..8 @ 120bpm = 2.0s..4.0s = 96000..192000 frames @ 48k.
        s.range = ExportRangeChoice::Custom {
            start_beat: 4.0,
            end_beat: 8.0,
        };
        let req = s.to_request(&snapshot, &defaults()).unwrap();
        assert_eq!(req.render.start_sample, 96_000);
        assert_eq!(req.render.end_sample, 192_000);
    }

    #[test]
    fn inverted_custom_range_fails() {
        let snapshot = snapshot_with_content(16.0);
        let mut s = valid_wav();
        s.range = ExportRangeChoice::Custom {
            start_beat: 8.0,
            end_beat: 4.0,
        };
        assert_eq!(
            s.to_request(&snapshot, &defaults()).err(),
            Some(ExportSettingsError::InvalidRange)
        );
    }

    #[test]
    fn project_sample_rate_resolves() {
        let mut d = defaults();
        d.project_sample_rate = 44_100;
        let snapshot = snapshot_with_content(4.0);
        let mut s = valid_wav();
        s.sample_rate = ExportSampleRateChoice::Project;
        let req = s.to_request(&snapshot, &d).unwrap();
        assert_eq!(req.render.sample_rate, 44_100);
    }

    #[test]
    fn flac_bit_depth_maps_to_sample_format() {
        let snapshot = snapshot_with_content(4.0);
        let mut s = valid_wav();
        s.format = AudioFileFormat::Flac;
        s.flac_bit_depth = 16;
        let req = s.to_request(&snapshot, &defaults()).unwrap();
        assert_eq!(req.sample_format, AudioSampleFormat::I16);
    }
}
