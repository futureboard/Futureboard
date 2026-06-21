mod backends;
mod error;
mod factory;
mod params;
mod processor;
mod ratios;

pub use error::StretchError;
pub use factory::{create_stretch_processor, resolve_backend, signalsmith_stretch_available};
pub use params::{StretchAlgorithm, StretchBackend, StretchMode, StretchParams};
pub use processor::StretchProcessor;
pub use ratios::{
    effective_pitch_ratio, effective_time_ratio, pitch_ratio_to_semitone_cents,
    semitone_to_pitch_ratio, source_read_rate_for_repitch, stretched_duration_samples,
};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    fn manual(time_ratio: f32) -> StretchParams {
        StretchParams {
            mode: StretchMode::Manual,
            algorithm: StretchAlgorithm::RePitch,
            time_ratio,
            preserve_pitch: false,
            ..StretchParams::default()
        }
    }

    fn approx(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 1e-5,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn effective_time_ratio_off_is_one() {
        let params = StretchParams {
            mode: StretchMode::Off,
            algorithm: StretchAlgorithm::Off,
            time_ratio: 2.0,
            ..StretchParams::default()
        };
        approx(effective_time_ratio(&params, None), 1.0);
    }

    #[test]
    fn effective_time_ratio_manual_two() {
        approx(effective_time_ratio(&manual(2.0), None), 2.0);
    }

    #[test]
    fn effective_time_ratio_manual_half() {
        approx(effective_time_ratio(&manual(0.5), None), 0.5);
    }

    #[test]
    fn effective_time_ratio_tempo_sync_120_to_60() {
        let params = StretchParams {
            mode: StretchMode::TempoSync,
            algorithm: StretchAlgorithm::PreservePitch,
            source_bpm: Some(120.0),
            target_bpm: Some(60.0),
            preserve_pitch: true,
            ..StretchParams::default()
        };
        approx(effective_time_ratio(&params, None), 2.0);
    }

    #[test]
    fn effective_time_ratio_tempo_sync_60_to_120() {
        let params = StretchParams {
            mode: StretchMode::TempoSync,
            algorithm: StretchAlgorithm::PreservePitch,
            source_bpm: Some(60.0),
            target_bpm: Some(120.0),
            preserve_pitch: true,
            ..StretchParams::default()
        };
        approx(effective_time_ratio(&params, None), 0.5);
    }

    #[test]
    fn effective_time_ratio_invalid_falls_back_to_one() {
        let params = StretchParams {
            mode: StretchMode::Manual,
            algorithm: StretchAlgorithm::RePitch,
            time_ratio: f32::NAN,
            ..StretchParams::default()
        };
        approx(effective_time_ratio(&params, None), 1.0);
    }

    #[test]
    fn source_read_rate_for_repitch_two() {
        approx(source_read_rate_for_repitch(&manual(2.0), None), 0.5);
    }

    #[test]
    fn source_read_rate_for_repitch_half() {
        approx(source_read_rate_for_repitch(&manual(0.5), None), 2.0);
    }

    #[test]
    fn source_read_rate_for_repitch_folds_pitch_ratio() {
        let params = StretchParams {
            pitch_ratio: 2.0,
            ..manual(1.0)
        };
        approx(source_read_rate_for_repitch(&params, None), 2.0);
    }

    #[test]
    fn stretched_duration_samples_scales() {
        assert_eq!(
            stretched_duration_samples(48_000, &manual(2.0), None),
            96_000
        );
        assert_eq!(
            stretched_duration_samples(48_000, &manual(0.5), None),
            24_000
        );
    }

    #[test]
    fn semitone_pitch_ratio_helpers_roundtrip() {
        approx(semitone_to_pitch_ratio(12.0, 0.0), 2.0);
        approx(
            semitone_to_pitch_ratio(0.0, 100.0),
            2.0_f32.powf(1.0 / 12.0),
        );
        let (semi, cents) = pitch_ratio_to_semitone_cents(2.0);
        approx(semi, 12.0);
        approx(cents, 0.0);
    }

    #[test]
    fn resolve_backend_off_uses_internal_repitch() {
        let params = StretchParams::default();
        assert_eq!(resolve_backend(&params), StretchBackend::InternalRePitch);
    }

    #[test]
    fn resolve_backend_repitch_uses_internal_repitch() {
        assert_eq!(
            resolve_backend(&manual(1.5)),
            StretchBackend::InternalRePitch
        );
    }

    #[test]
    fn resolve_backend_preserve_pitch_uses_signalsmith() {
        let params = StretchParams {
            mode: StretchMode::Manual,
            algorithm: StretchAlgorithm::PreservePitch,
            time_ratio: 1.5,
            preserve_pitch: true,
            ..StretchParams::default()
        };
        assert_eq!(resolve_backend(&params), StretchBackend::Signalsmith);
    }

    #[test]
    fn stretch_params_serde_roundtrip() {
        let params = StretchParams {
            mode: StretchMode::TempoSync,
            algorithm: StretchAlgorithm::PreservePitch,
            time_ratio: 1.25,
            pitch_ratio: 1.125,
            source_bpm: Some(100.0),
            target_bpm: Some(125.0),
            preserve_pitch: true,
            quality: 0.75,
        };

        let encoded = serde_json::to_string(&params).expect("serialize");
        let decoded: StretchParams = serde_json::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded, params);
    }

    #[test]
    fn internal_repitch_process_stereo_does_not_crash() {
        let params = manual(2.0);
        let mut processor =
            create_stretch_processor(StretchBackend::InternalRePitch, 48_000.0, 2, params)
                .expect("create repitch processor");

        let input_l = [0.0_f32, 0.25, 0.5, 0.75];
        let input_r = [1.0_f32, 0.75, 0.5, 0.25];
        let mut output_l = [0.0; 4];
        let mut output_r = [0.0; 4];

        processor
            .process_stereo(&input_l, &input_r, &mut output_l, &mut output_r)
            .expect("process repitch");

        assert_eq!(output_l.len(), 4);
        assert_eq!(output_r.len(), 4);
    }

    #[test]
    fn internal_repitch_buffer_length_mismatch_returns_error() {
        let params = manual(1.0);
        let mut processor =
            create_stretch_processor(StretchBackend::InternalRePitch, 48_000.0, 2, params)
                .expect("create repitch processor");

        let input_l = [0.0_f32; 4];
        let input_r = [0.0_f32; 3];
        let mut output_l = [0.0; 4];
        let mut output_r = [0.0; 4];

        let err = processor
            .process_stereo(&input_l, &input_r, &mut output_l, &mut output_r)
            .expect_err("expected mismatch error");
        assert!(matches!(err, StretchError::BufferLengthMismatch));
    }

    #[test]
    fn signalsmith_create_and_process_if_available() {
        let params = StretchParams {
            mode: StretchMode::Manual,
            algorithm: StretchAlgorithm::PreservePitch,
            time_ratio: 1.25,
            preserve_pitch: true,
            ..StretchParams::default()
        };

        let mut processor =
            match create_stretch_processor(StretchBackend::Signalsmith, 48_000.0, 2, params) {
                Ok(processor) => processor,
                Err(StretchError::BackendUnavailable(StretchBackend::Signalsmith)) => return,
                Err(err) => panic!("unexpected create error: {err}"),
            };

        let input_l = [0.0_f32, 0.25, 0.5, 0.75, 1.0, 0.5, 0.0, -0.5];
        let input_r = [0.0_f32; 8];
        let mut output_l = [0.0; 8];
        let mut output_r = [0.0; 8];

        processor
            .process_stereo(&input_l, &input_r, &mut output_l, &mut output_r)
            .expect("signalsmith process");
    }

    #[test]
    fn signalsmith_time_stretch_via_unequal_counts_if_available() {
        // The bridge expresses time-stretch as output_len / input_len. Feeding
        // fewer input samples than requested output (here 2× → slow down) must
        // succeed without buffer-length errors and fill the whole output.
        let params = StretchParams {
            mode: StretchMode::Manual,
            algorithm: StretchAlgorithm::PreservePitch,
            time_ratio: 2.0,
            preserve_pitch: true,
            ..StretchParams::default()
        };
        let mut processor =
            match create_stretch_processor(StretchBackend::Signalsmith, 48_000.0, 2, params) {
                Ok(processor) => processor,
                Err(StretchError::BackendUnavailable(StretchBackend::Signalsmith)) => return,
                Err(err) => panic!("unexpected create error: {err}"),
            };

        let input_l = [0.1_f32; 256];
        let input_r = [0.1_f32; 256];
        let mut output_l = [f32::NAN; 512];
        let mut output_r = [f32::NAN; 512];

        processor
            .process_stereo(&input_l, &input_r, &mut output_l, &mut output_r)
            .expect("signalsmith stretch process");

        assert!(output_l.iter().all(|v| v.is_finite()));
        assert!(output_r.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn signalsmith_reports_bounded_latency_if_available() {
        let mut processor =
            match create_stretch_processor(StretchBackend::Signalsmith, 48_000.0, 2, manual(1.0)) {
                Ok(processor) => processor,
                Err(StretchError::BackendUnavailable(StretchBackend::Signalsmith)) => return,
                Err(err) => panic!("unexpected create error: {err}"),
            };
        processor.set_params(StretchParams {
            mode: StretchMode::Manual,
            algorithm: StretchAlgorithm::PreservePitch,
            preserve_pitch: true,
            ..manual(1.0)
        });
        let latency = processor.latency_samples();
        eprintln!("SIGNALSMITH_LATENCY_SAMPLES={latency}");
        // Sanity bound: a sane preset stays well under a second of latency.
        assert!(latency < 48_000, "unexpectedly large latency: {latency}");
    }
}
