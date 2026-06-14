//! The offline render loop: builds a `RuntimeProject` from a snapshot and drives
//! the shared render kernel block by block with no audio device.

use std::collections::HashMap;
use std::sync::Arc;

use crate::audio_source::ClipAudioSource;
use crate::engine::{render_project_block_interleaved, schedule_midi_render_block};
use crate::runtime::{clip_dsp_debug_enabled, describe_clip_dsp_state, RuntimeProject};
use crate::types::EngineProjectSnapshot;

use super::render_progress::{ExportCancelToken, ExportProgress, ExportStage};
use super::render_request::{ExportTailMode, OfflineRenderRequest};
use super::ExportError;

/// Result of an offline render pass.
#[derive(Debug, Clone)]
pub struct OfflineRenderSummary {
    pub frames_rendered: u64,
    /// Absolute peak sample magnitude observed across the render.
    pub peak: f32,
}

/// Render the arrangement offline.
///
/// `on_block` receives interleaved frames at `request.channels`; returning an
/// error from it aborts the render (e.g. an encoder write failure). `on_gain`
/// is applied to every sample after the kernel render — use it to inject a
/// normalization gain on a second pass (pass `1.0` for the analysis pass).
///
/// This holds no GPUI/UI state: the snapshot is plain data and the loop runs on
/// whatever thread the caller chooses (a background worker, never the UI thread).
pub fn render_offline(
    snapshot: &EngineProjectSnapshot,
    request: &OfflineRenderRequest,
    cancel: &ExportCancelToken,
    gain: f32,
    mut on_block: impl FnMut(&[f32]) -> Result<(), ExportError>,
    mut on_progress: impl FnMut(ExportProgress),
) -> Result<OfflineRenderSummary, ExportError> {
    request.validate().map_err(ExportError::Settings)?;

    on_progress(ExportProgress::stage_only(
        ExportStage::Preparing,
        request.content_frames(),
    ));

    // Build the runtime graph from the snapshot. Fresh audio-source cache and no
    // VST3 reuse — this is an isolated offline graph.
    let mut audio_cache: HashMap<String, Arc<ClipAudioSource>> = HashMap::new();
    let mut runtime =
        RuntimeProject::build(snapshot, request.sample_rate, &mut audio_cache, None, false)
            .map_err(|e| ExportError::Build(e.to_string()))?;
    runtime.sample_rate = request.sample_rate;
    // Restore each in-process VST3 insert's saved state before any process() call
    // so instruments/effects render with the user's current tweaks. Runs once on
    // this worker thread, never the audio callback.
    restore_offline_plugin_states(snapshot, &mut runtime);
    runtime.reset_midi_playback(request.start_sample);

    let channels = request.channels.max(1) as usize;
    let block = request.block_size.max(1);
    let ts_num = snapshot.time_signature[0].max(1);
    let ts_den = snapshot.time_signature[1].max(1);

    if clip_dsp_debug_enabled() {
        for clip in &snapshot.clips {
            if let Some(process) = &clip.audio_process {
                eprintln!(
                    "[clip-dsp][export-start] {}",
                    describe_clip_dsp_state(clip, process, snapshot.bpm)
                );
            }
        }
    }

    // The kernel renders stereo (channels >= 2). We always render into a 2-ch
    // scratch and fold down to mono on output when requested.
    let mut stereo = vec![0.0f32; block * 2];
    let mut out = vec![0.0f32; block * channels];

    let content_frames = request.content_frames();
    let total_with_tail = content_frames.saturating_add(request.max_tail_frames());

    let mut pos = request.start_sample;
    let mut rendered = 0u64;
    let mut peak = 0.0f32;
    let mut progress_throttle = 0u32;

    // Phase 1: content. Phase 2: tail (rendered past content end so plugin /
    // instrument tails ring out).
    let mut tail_remaining = request.max_tail_frames();
    let until_silence = matches!(request.tail, ExportTailMode::UntilSilence { .. });
    let silence_threshold = match request.tail {
        ExportTailMode::UntilSilence { threshold_db, .. } => {
            10f32.powf(threshold_db / 20.0).clamp(0.0, 1.0)
        }
        _ => 0.0,
    };

    loop {
        if cancel.is_cancelled() {
            return Err(ExportError::Cancelled);
        }

        let in_content = rendered < content_frames;
        if !in_content {
            // Tail handling.
            match request.tail {
                ExportTailMode::None => break,
                _ => {
                    if tail_remaining == 0 {
                        break;
                    }
                }
            }
        }

        // Frames to render this block: clamp the content phase to the content
        // boundary so we don't over-render the body.
        let this_block = if in_content {
            block.min((content_frames - rendered) as usize).max(1)
        } else {
            block.min(tail_remaining as usize).max(1)
        };

        let stereo_slice = &mut stereo[..this_block * 2];
        for s in stereo_slice.iter_mut() {
            *s = 0.0;
        }

        // Schedule MIDI for this block, then render the kernel block. Mirrors the
        // realtime callback order in `fill_output_f32_inner`.
        let _ = schedule_midi_render_block(&mut runtime, pos, this_block as u64, None);
        let frames = render_project_block_interleaved(
            &mut runtime,
            pos,
            request.master_volume,
            stereo_slice,
            2,
            true,
            ts_num,
            ts_den,
            None,
        );
        // Match the realtime path: per-block MIDI events are consumed once.
        for track in &mut runtime.tracks {
            track.midi_block_events.clear();
        }

        let frames = frames as usize;
        if frames == 0 {
            // Defensive: avoid an infinite loop if the kernel returns nothing.
            break;
        }

        // Fold to the requested channel layout, apply normalization gain, and
        // measure peak.
        let mut block_peak = 0.0f32;
        let out_slice = &mut out[..frames * channels];
        for f in 0..frames {
            let l = stereo_slice[f * 2] * gain;
            let r = stereo_slice[f * 2 + 1] * gain;
            block_peak = block_peak.max(l.abs()).max(r.abs());
            if channels == 1 {
                out_slice[f] = (l + r) * 0.5;
            } else {
                out_slice[f * channels] = l;
                out_slice[f * channels + 1] = r;
                for c in 2..channels {
                    out_slice[f * channels + c] = 0.0;
                }
            }
        }
        peak = peak.max(block_peak);
        on_block(out_slice)?;

        let frames_u64 = frames as u64;
        rendered = rendered.saturating_add(frames_u64);
        pos = pos.saturating_add(frames_u64);
        if !in_content {
            tail_remaining = tail_remaining.saturating_sub(frames_u64);
            // UntilSilence: stop early once the block has decayed below threshold.
            if until_silence && block_peak < silence_threshold {
                break;
            }
        }

        // Throttle progress callbacks (~ every 16 blocks) to avoid flooding.
        progress_throttle = progress_throttle.wrapping_add(1);
        if progress_throttle.is_multiple_of(16) {
            on_progress(ExportProgress::new(
                ExportStage::Rendering,
                rendered.min(total_with_tail.max(1)),
                total_with_tail.max(1),
            ));
        }
    }

    if cancel.is_cancelled() {
        return Err(ExportError::Cancelled);
    }

    Ok(OfflineRenderSummary {
        frames_rendered: rendered,
        peak,
    })
}

#[inline]
fn plugin_state_debug_enabled() -> bool {
    std::env::var("FUTUREBOARD_PLUGIN_STATE_DEBUG")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Restore saved VST3 state into the freshly-built in-process processors.
///
/// Matches snapshot inserts to runtime inserts by track id + insert id, decodes
/// the packed `Vst3PluginState` blob carried by the offline-export snapshot, and
/// applies it. Inserts that failed to instantiate (`vst3 == None`, e.g. a plugin
/// that refuses a second instance) are skipped — the render still completes; the
/// kernel bypasses them (instrument → silence, effect → dry). Bad/empty blobs are
/// ignored. Control-thread only.
fn restore_offline_plugin_states(snapshot: &EngineProjectSnapshot, runtime: &mut RuntimeProject) {
    use crate::vst3_processor::Vst3PluginState;

    let debug = plugin_state_debug_enabled();
    for snap_track in &snapshot.tracks {
        let Some(rt_track) = runtime.tracks.iter_mut().find(|t| t.id == snap_track.id) else {
            continue;
        };
        for snap_insert in &snap_track.inserts {
            let Some(bytes) = snap_insert.state.as_ref() else {
                continue;
            };
            let Some(rt_insert) = rt_track.inserts.iter().find(|i| i.id == snap_insert.id) else {
                continue;
            };
            let Some(vst3) = rt_insert.vst3.as_ref() else {
                if debug {
                    eprintln!(
                        "[export-state] skip insert={} track={} reason=not_instantiated",
                        snap_insert.id, snap_track.id
                    );
                }
                continue;
            };
            match Vst3PluginState::from_packed_bytes(bytes) {
                Some(state) => {
                    let ok = vst3.set_state(&state);
                    if debug {
                        eprintln!(
                            "[export-state] restore insert={} track={} bytes={} ok={}",
                            snap_insert.id,
                            snap_track.id,
                            bytes.len(),
                            ok
                        );
                    }
                }
                None if debug => {
                    eprintln!(
                        "[export-state] skip insert={} track={} reason=unparseable_blob bytes={}",
                        snap_insert.id,
                        snap_track.id,
                        bytes.len()
                    );
                }
                None => {}
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn silence_snapshot(sample_rate: u32) -> EngineProjectSnapshot {
    use crate::types::{EngineRoutingSnapshot, EngineTrackSnapshot};
    EngineProjectSnapshot {
        project_id: "test".to_string(),
        project_root: None,
        preferred_input_device: None,
        bpm: 120.0,
        tempo_points: Vec::new(),
        time_signature: [4, 4],
        sample_rate,
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
        clips: Vec::new(),
        midi_clips: Vec::new(),
        routing: EngineRoutingSnapshot {
            master_output_device: None,
            sample_rate,
            buffer_size: 512,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::render_request::{ExportNormalizeMode, ExportTailMode};

    fn request(start: u64, end: u64) -> OfflineRenderRequest {
        OfflineRenderRequest {
            sample_rate: 48_000,
            channels: 2,
            start_sample: start,
            end_sample: end,
            master_volume: 1.0,
            block_size: 256,
            tail: ExportTailMode::None,
            normalize: ExportNormalizeMode::None,
        }
    }

    #[test]
    fn empty_project_renders_exact_frame_count_of_silence() {
        let snapshot = silence_snapshot(48_000);
        let req = request(0, 1000);
        let cancel = ExportCancelToken::new();
        let mut total = 0u64;
        let summary = render_offline(
            &snapshot,
            &req,
            &cancel,
            1.0,
            |block| {
                total += (block.len() / 2) as u64;
                Ok(())
            },
            |_p| {},
        )
        .unwrap();
        assert_eq!(summary.frames_rendered, 1000);
        assert_eq!(total, 1000);
        assert_eq!(summary.peak, 0.0);
    }

    #[test]
    fn pre_cancelled_render_returns_cancelled() {
        let snapshot = silence_snapshot(48_000);
        let req = request(0, 100_000);
        let cancel = ExportCancelToken::new();
        cancel.cancel();
        let result = render_offline(&snapshot, &req, &cancel, 1.0, |_b| Ok(()), |_p| {});
        assert!(matches!(result, Err(ExportError::Cancelled)));
    }

    #[test]
    fn invalid_range_is_rejected() {
        let snapshot = silence_snapshot(48_000);
        let req = request(500, 500);
        let cancel = ExportCancelToken::new();
        let result = render_offline(&snapshot, &req, &cancel, 1.0, |_b| Ok(()), |_p| {});
        assert!(matches!(result, Err(ExportError::Settings(_))));
    }

    #[test]
    fn restore_offline_plugin_states_skips_uninstantiated_insert() {
        use crate::types::EngineInsertSnapshot;
        use std::collections::HashMap;

        let mut snapshot = silence_snapshot(48_000);
        // native-plugin insert with no module path → no in-process processor is
        // created (from_params returns None before any FFI). It still carries a
        // garbage state blob; restore must skip it without panicking.
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("format".to_string(), serde_json::json!("VST3"));
        snapshot.tracks[0].inserts.push(EngineInsertSnapshot {
            id: "insert-1".to_string(),
            kind: "native-plugin".to_string(),
            enabled: true,
            params,
            state: Some(vec![0xde, 0xad, 0xbe, 0xef]),
        });

        let mut cache = HashMap::new();
        let mut runtime =
            RuntimeProject::build(&snapshot, 48_000, &mut cache, None, false).expect("build");
        assert!(
            runtime.tracks[0].inserts[0].vst3.is_none(),
            "insert without a module path must not instantiate a processor"
        );
        // Must not panic on a missing processor / unparseable blob.
        restore_offline_plugin_states(&snapshot, &mut runtime);
    }

    #[test]
    fn mono_output_folds_to_single_channel() {
        let snapshot = silence_snapshot(48_000);
        let mut req = request(0, 480);
        req.channels = 1;
        let cancel = ExportCancelToken::new();
        let mut samples = 0u64;
        render_offline(
            &snapshot,
            &req,
            &cancel,
            1.0,
            |block| {
                samples += block.len() as u64;
                Ok(())
            },
            |_p| {},
        )
        .unwrap();
        assert_eq!(samples, 480);
    }
}
