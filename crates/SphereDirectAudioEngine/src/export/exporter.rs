//! Arrangement exporter: render offline → encode with `sphere_encoder` → write
//! atomically. Streaming, cancellable, progress-reporting.

use std::path::{Path, PathBuf};

use sphere_encoder::{
    create_encoder, AudioEncodeOptions, AudioEncodeSpec, AudioFileFormat, AudioSampleFormat,
};

use crate::types::EngineProjectSnapshot;

use super::offline_renderer::render_offline;
use super::render_progress::{ExportCancelToken, ExportProgress, ExportStage};
use super::render_request::{ExportNormalizeMode, OfflineRenderRequest};
use super::ExportError;

/// A full arrangement export request: where to write, in what container, and
/// how to render.
#[derive(Debug, Clone)]
pub struct ArrangementExportRequest {
    pub output_path: PathBuf,
    pub format: AudioFileFormat,
    pub sample_format: AudioSampleFormat,
    pub render: OfflineRenderRequest,
    /// Per-format encoder options (WAV/FLAC/MP3 + metadata).
    pub encode_options: AudioEncodeOptions,
}

#[derive(Debug, Clone)]
pub struct ArrangementExportSummary {
    pub output_path: PathBuf,
    pub format: AudioFileFormat,
    pub sample_rate: u32,
    pub channels: u16,
    pub frames_written: u64,
    pub duration_seconds: f64,
    pub peak_db: Option<f32>,
}

/// Temp path an export writes to before atomically replacing the final file.
pub fn partial_path_for(output: &Path) -> PathBuf {
    let mut s = output.as_os_str().to_os_string();
    s.push(".partial");
    PathBuf::from(s)
}

fn linear_to_db(peak: f32) -> Option<f32> {
    if peak <= 0.0 {
        None
    } else {
        Some(20.0 * peak.log10())
    }
}

/// Export the arrangement to `request.output_path`.
///
/// Flow: Preparing → (AnalyzingPeak if normalizing) → Rendering/Encoding →
/// Finalizing → Complete. Writes to a `.partial` temp file and only replaces the
/// final output once the encoder finalizes successfully. On cancel or error the
/// partial file is removed and an existing final file is left untouched.
pub fn export_arrangement(
    snapshot: &EngineProjectSnapshot,
    request: &ArrangementExportRequest,
    cancel: &ExportCancelToken,
    mut on_progress: impl FnMut(ExportProgress),
) -> Result<ArrangementExportSummary, ExportError> {
    request.render.validate().map_err(ExportError::Settings)?;

    let total = request
        .render
        .content_frames()
        .saturating_add(request.render.max_tail_frames());

    on_progress(ExportProgress::stage_only(ExportStage::Preparing, total));

    if let Some(parent) = request.output_path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            return Err(ExportError::Settings(format!(
                "output directory does not exist: {}",
                parent.display()
            )));
        }
    }

    let spec = AudioEncodeSpec {
        sample_rate: request.render.sample_rate,
        channels: request.render.channels,
        sample_format: request.sample_format,
    };

    // ── Pass 1 (optional): peak analysis for normalization ──────────────────
    let gain = match request.render.normalize {
        ExportNormalizeMode::None => 1.0,
        ExportNormalizeMode::PeakDb(target_db) => {
            on_progress(ExportProgress::stage_only(
                ExportStage::AnalyzingPeak,
                total,
            ));
            let analysis = render_offline(
                snapshot,
                &request.render,
                cancel,
                1.0,
                |_block| Ok(()),
                |_p| {},
            )?;
            if analysis.peak <= f32::EPSILON {
                1.0
            } else {
                let target_lin = 10f32.powf(target_db / 20.0);
                (target_lin / analysis.peak).clamp(0.0, 64.0)
            }
        }
    };

    // ── Encode pass: render → encoder, written to a .partial temp file ──────
    let partial = partial_path_for(&request.output_path);
    // Remove any stale partial from a previous aborted run.
    let _ = std::fs::remove_file(&partial);

    let mut encoder = create_encoder(&partial, spec, request.encode_options.clone())?;

    on_progress(ExportProgress::new(ExportStage::Encoding, 0, total));

    let render_result = render_offline(
        snapshot,
        &request.render,
        cancel,
        gain,
        |block| {
            encoder.write_interleaved_f32(block)?;
            Ok(())
        },
        |p| {
            // Surface render progress as the Encoding stage (render+encode are
            // fused in this single streaming pass).
            on_progress(ExportProgress::new(
                ExportStage::Encoding,
                p.rendered_frames,
                p.total_frames,
            ));
        },
    );

    let summary = match render_result {
        Ok(summary) => summary,
        Err(err) => {
            // finalize() is intentionally skipped; drop the encoder and remove
            // the partial so an existing final file is never touched.
            drop(encoder);
            let _ = std::fs::remove_file(&partial);
            return Err(err);
        }
    };

    on_progress(ExportProgress::new(ExportStage::Finalizing, total, total));
    let encode_summary = match encoder.finalize() {
        Ok(s) => s,
        Err(err) => {
            let _ = std::fs::remove_file(&partial);
            return Err(ExportError::Encode(err));
        }
    };

    // Atomic-ish replace: only now, after a successful finalize, do we touch the
    // final path. Remove an existing file then rename the partial into place.
    if request.output_path.exists() {
        if let Err(err) = std::fs::remove_file(&request.output_path) {
            let _ = std::fs::remove_file(&partial);
            return Err(ExportError::Io(err));
        }
    }
    if let Err(err) = std::fs::rename(&partial, &request.output_path) {
        let _ = std::fs::remove_file(&partial);
        return Err(ExportError::Io(err));
    }

    let duration_seconds = if request.render.sample_rate > 0 {
        encode_summary.frames_written as f64 / request.render.sample_rate as f64
    } else {
        0.0
    };

    on_progress(ExportProgress::stage_only(ExportStage::Complete, total));

    Ok(ArrangementExportSummary {
        output_path: request.output_path.clone(),
        format: request.format,
        sample_rate: encode_summary.sample_rate,
        channels: encode_summary.channels,
        frames_written: encode_summary.frames_written,
        duration_seconds,
        peak_db: linear_to_db(summary.peak),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::offline_renderer::silence_snapshot;
    use crate::export::render_request::{ExportNormalizeMode, ExportTailMode};
    use sphere_encoder::AudioEncodeOptions;

    fn temp_path(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "futureboard-export-{name}-{}-{}.wav",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        path
    }

    fn wav_request(output: PathBuf, end: u64) -> ArrangementExportRequest {
        let mut encode_options = AudioEncodeOptions::default();
        encode_options.format = AudioFileFormat::Wav;
        ArrangementExportRequest {
            output_path: output,
            format: AudioFileFormat::Wav,
            sample_format: AudioSampleFormat::F32,
            render: OfflineRenderRequest {
                sample_rate: 48_000,
                channels: 2,
                start_sample: 0,
                end_sample: end,
                master_volume: 1.0,
                block_size: 256,
                tail: ExportTailMode::None,
                normalize: ExportNormalizeMode::None,
            },
            encode_options,
        }
    }

    #[test]
    fn exports_silence_to_wav_atomically() {
        let out = temp_path("ok");
        let req = wav_request(out.clone(), 1000);
        let snapshot = silence_snapshot(48_000);
        let cancel = ExportCancelToken::new();
        let summary = export_arrangement(&snapshot, &req, &cancel, |_p| {}).unwrap();
        assert_eq!(summary.frames_written, 1000);
        assert!(out.exists());
        assert!(
            !partial_path_for(&out).exists(),
            "partial should be renamed away"
        );
        let bytes = std::fs::read(&out).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        let _ = std::fs::remove_file(out);
    }

    #[test]
    fn cancelled_export_removes_partial_and_leaves_existing_output() {
        let out = temp_path("cancel");
        std::fs::write(&out, b"ORIGINAL").unwrap();
        let req = wav_request(out.clone(), 500_000);
        let snapshot = silence_snapshot(48_000);
        let cancel = ExportCancelToken::new();
        cancel.cancel();
        let result = export_arrangement(&snapshot, &req, &cancel, |_p| {});
        assert!(matches!(result, Err(ExportError::Cancelled)));
        // Existing file untouched, no leftover partial.
        assert_eq!(std::fs::read(&out).unwrap(), b"ORIGINAL");
        assert!(!partial_path_for(&out).exists());
        let _ = std::fs::remove_file(out);
    }

    #[test]
    fn rejects_missing_output_directory() {
        let mut out = std::env::temp_dir();
        out.push("futureboard-nonexistent-dir-xyz");
        out.push("file.wav");
        let req = wav_request(out, 1000);
        let snapshot = silence_snapshot(48_000);
        let cancel = ExportCancelToken::new();
        let result = export_arrangement(&snapshot, &req, &cancel, |_p| {});
        assert!(matches!(result, Err(ExportError::Settings(_))));
    }

    #[test]
    fn success_replaces_existing_output_file() {
        let out = temp_path("replace");
        std::fs::write(&out, b"OLD-SMALL").unwrap();
        let req = wav_request(out.clone(), 2000);
        let snapshot = silence_snapshot(48_000);
        let cancel = ExportCancelToken::new();
        let summary = export_arrangement(&snapshot, &req, &cancel, |_p| {}).unwrap();
        assert_eq!(summary.frames_written, 2000);
        let bytes = std::fs::read(&out).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert!(bytes.len() > 9, "new export should be larger than the stub");
        let _ = std::fs::remove_file(out);
    }
}
