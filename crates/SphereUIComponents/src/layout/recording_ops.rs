use gpui::Context;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::components::timeline::timeline_state::{TrackAudioFormat, TrackType};

use super::StudioLayout;
use DAUx::types::{JsRecordingTrackConfig, JsStartRecordingConfig};

impl StudioLayout {
    pub(super) fn toggle_native_recording(&mut self, cx: &mut Context<Self>) {
        let recording = self.timeline.read(cx).state.transport.recording;
        if recording {
            self.stop_native_recording(cx);
        } else {
            self.start_native_recording(cx);
        }
    }

    pub(super) fn start_native_recording(&mut self, cx: &mut Context<Self>) {
        let Some(engine) = self.audio_engine.as_ref() else {
            self.audio_last_error = Some("audio engine unavailable".to_string());
            return;
        };

        let project_root = match self.project_folder.as_ref() {
            Some(path) => path.clone(),
            None => {
                self.audio_last_error =
                    Some("save the project to a folder before recording".to_string());
                eprintln!("[recording] no project folder — save project first");
                return;
            }
        };

        let (bpm, start_beat, sample_rate, input_device_name, monitor_mix) = {
            let timeline = self.timeline.read(cx);
            let settings = self.settings.read(cx);
            let armed_count = timeline
                .state
                .tracks
                .iter()
                .filter(|t| t.armed && t.track_type == TrackType::Audio)
                .count();
            if armed_count == 0 {
                eprintln!("[recording] no armed audio tracks");
                return;
            }
            let monitor_mix = timeline
                .state
                .tracks
                .iter()
                .filter(|t| t.armed && t.track_type == TrackType::Audio)
                .any(|t| t.input_monitor.is_active(t.armed));
            (
                timeline.state.bpm,
                timeline.state.transport.playhead_beats,
                self.current_audio_sample_rate(),
                settings.current.hardware.audio.device_in.clone(),
                monitor_mix,
            )
        };

        let tracks: Vec<JsRecordingTrackConfig> = {
            let timeline = self.timeline.read(cx);
            timeline
                .state
                .tracks
                .iter()
                .filter(|t| t.armed && t.track_type == TrackType::Audio)
                .map(|t| JsRecordingTrackConfig {
                    track_id: t.id.clone(),
                    input_channels: recording_input_channels(t),
                    name: t.name.clone(),
                })
                .collect()
        };

        let input_device_id = resolve_input_device_id(engine, &input_device_name);
        let session_id = format!(
            "rec-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        );

        let config = JsStartRecordingConfig {
            project_root: project_root.to_string_lossy().into_owned(),
            session_id,
            bpm: bpm.max(1.0) as f64,
            start_beat: start_beat.max(0.0) as f64,
            sample_rate: sample_rate.max(1),
            input_device_id,
            tracks,
            monitor_mix,
        };

        if let Err(error) = engine.start_recording(config) {
            self.audio_last_error = Some(error.to_string());
            eprintln!("[recording] start failed: {error}");
            return;
        }

        self.recording_start_beat = start_beat.max(0.0);
        self.audio_last_error = None;
        let _ = self.timeline.update(cx, |timeline, cx| {
            timeline.state.transport.recording = true;
            cx.notify();
        });

        let playing = self
            .audio_stats
            .as_ref()
            .map(|s| s.transport_playing)
            .unwrap_or(false);
        if !playing {
            self.start_native_playback(cx);
        }
    }

    pub(super) fn stop_native_recording(&mut self, cx: &mut Context<Self>) {
        let Some(engine) = self.audio_engine.as_ref() else {
            return;
        };

        let results = match engine.stop_recording() {
            Ok(results) => results,
            Err(error) => {
                self.audio_last_error = Some(error.to_string());
                eprintln!("[recording] stop failed: {error}");
                let _ = self.timeline.update(cx, |timeline, cx| {
                    timeline.state.transport.recording = false;
                    cx.notify();
                });
                return;
            }
        };

        let _ = self.timeline.update(cx, |timeline, cx| {
            timeline.state.transport.recording = false;
            cx.notify();
        });

        self.commit_recording_results(cx, results);
    }

    fn commit_recording_results(
        &mut self,
        cx: &mut Context<Self>,
        results: Vec<DAUx::types::JsRecordingResult>,
    ) {
        let bpm = self.timeline.read(cx).state.bpm;
        let owner = cx.entity().clone();
        let timeline = self.timeline.clone();
        let mut import_paths: Vec<(PathBuf, String)> = Vec::new();

        let _ = self.timeline.update(cx, |timeline, cx| {
            for result in &results {
                if !result.success {
                    eprintln!(
                        "[recording] track {} failed: {}",
                        result.track_id,
                        result.error.as_deref().unwrap_or("unknown error")
                    );
                    continue;
                }
                let clip_name = Path::new(&result.relative_path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Recording")
                    .to_string();
                let clip_id = timeline.state.insert_recorded_clip(
                    &result.track_id,
                    result.file_path.clone(),
                    clip_name,
                    result.start_beat as f32,
                    result.duration_seconds,
                    bpm,
                );
                import_paths.push((PathBuf::from(&result.file_path), result.file_path.clone()));
                eprintln!(
                    "[recording] clip created id={clip_id} track={} path={}",
                    result.track_id, result.relative_path
                );
            }
            cx.notify();
        });

        self.engine_project_dirty = true;
        self.engine_media_dirty = true;
        self.schedule_audio_project_sync(cx, true, "recording_commit");

        for (path, path_key) in import_paths {
            Self::spawn_timeline_audio_import_jobs(
                cx,
                owner.clone(),
                timeline.clone(),
                path,
                path_key,
            );
        }
    }
}

fn recording_input_channels(
    track: &crate::components::timeline::timeline_state::TrackState,
) -> Vec<u32> {
    use crate::components::timeline::timeline_state::TrackInputRouting;
    match &track.routing.input {
        TrackInputRouting::AudioDeviceChannel { channel, .. } => vec![*channel],
        _ => match track.routing.audio_format {
            TrackAudioFormat::Mono => vec![0],
            TrackAudioFormat::Stereo => vec![0, 1],
        },
    }
}

fn resolve_input_device_id(engine: &DAUx::AudioEngine, device_name: &str) -> Option<String> {
    if device_name.trim().is_empty() {
        return None;
    }
    engine
        .list_input_devices()
        .into_iter()
        .find(|d| d.name == device_name || d.id == device_name)
        .map(|d| d.id)
        .or_else(|| Some(device_name.to_string()))
}
