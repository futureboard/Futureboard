use gpui::Context;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::components::timeline::timeline_state::{
    TrackAudioFormat, TrackInputRouting, TrackState, TrackType,
};
use crate::components::timeline::waveform_cache::{self, WaveformPeak};

use super::{RecordingPreviewUi, RecordingUiState, StudioLayout, RECORDING_PREVIEW_CLIP_ID};
use DAUx::types::{JsRecordingTrackConfig, JsStartRecordingConfig};

impl StudioLayout {
    pub(super) fn toggle_native_recording(&mut self, cx: &mut Context<Self>) {
        let recording = self.is_recording_active(cx);
        if recording {
            self.stop_native_recording(cx);
        } else {
            self.start_native_recording(cx);
        }
    }

    pub(super) fn start_native_recording(&mut self, cx: &mut Context<Self>) {
        self.recording_ui_state = RecordingUiState::Preparing;
        cx.notify();

        let project_root = match self.project_folder.as_ref() {
            Some(path) => path.clone(),
            None => {
                self.audio_last_error =
                    Some("save the project to a folder before recording".to_string());
                self.recording_ui_state = RecordingUiState::Failed {
                    reason: "save the project to a folder before recording".to_string(),
                };
                eprintln!("[recording] no project folder — save project first");
                return;
            }
        };

        let save_before_recording = self
            .settings
            .read(cx)
            .current
            .recording
            .audio
            .save_before_recording;
        if save_before_recording && self.project_session.is_dirty {
            let Some(project_path) = self.project_session.project_file_path.clone() else {
                self.fail_recording_start("save the project to a folder before recording", cx);
                return;
            };
            if !self.do_save_project(&project_path, cx) {
                self.fail_recording_start("could not save the project before recording", cx);
                return;
            }
        }

        let Some(engine) = self.audio_engine.as_ref() else {
            self.fail_recording_start("audio engine unavailable", cx);
            return;
        };

        let input_devices: Vec<RecordingInputDevice> = engine
            .list_input_devices()
            .into_iter()
            .map(|d| RecordingInputDevice {
                id: d.id,
                name: d.name,
                channels: d.channels,
                is_default: d.is_default,
            })
            .collect();

        let (bpm, start_beat, sample_rate, input_device_name) = {
            let timeline = self.timeline.read(cx);
            let settings = self.settings.read(cx);
            let armed_count = timeline
                .state
                .tracks
                .iter()
                .filter(|t| t.armed && t.track_type == TrackType::Audio)
                .count();
            if armed_count == 0 {
                self.fail_recording_start("no armed audio tracks", cx);
                return;
            }
            (
                timeline.state.bpm,
                timeline.state.transport.playhead_beats,
                self.current_audio_sample_rate(),
                settings.current.hardware.audio.device_in.clone(),
            )
        };

        let selected_input_device =
            select_recording_input_device(&input_devices, &input_device_name);

        let (tracks, explicit_device_id, monitor_channels): (
            Vec<JsRecordingTrackConfig>,
            Option<String>,
            Vec<u32>,
        ) = {
            let timeline = self.timeline.read(cx);
            let mut explicit_device_id: Option<String> = None;
            let mut monitor_channels = Vec::new();
            let mut configs = Vec::new();
            for track in timeline
                .state
                .tracks
                .iter()
                .filter(|t| t.armed && t.track_type == TrackType::Audio)
            {
                let route = match recording_input_channels_checked(
                    track,
                    &input_devices,
                    selected_input_device,
                ) {
                    Ok(route) => route,
                    Err(error) => {
                        self.fail_recording_start(&error, cx);
                        return;
                    }
                };
                if let Some(device_id) = route.device_id.clone() {
                    match explicit_device_id.as_ref() {
                        Some(existing) if existing != &device_id => {
                            self.fail_recording_start(
                                "armed tracks use different input devices; record one input device at a time",
                                cx,
                            );
                            return;
                        }
                        None => explicit_device_id = Some(device_id),
                        _ => {}
                    }
                }
                if monitor_channels.is_empty() && track.input_monitor.is_active(track.armed) {
                    monitor_channels = route.channels.clone();
                }
                configs.push(JsRecordingTrackConfig {
                    track_id: track.id.clone(),
                    input_channels: route.channels,
                    name: track.name.clone(),
                });
            }
            (configs, explicit_device_id, monitor_channels)
        };

        let input_device_id = explicit_device_id.or_else(|| {
            selected_input_device
                .as_ref()
                .map(|device| device.id.clone())
                .or_else(|| resolve_input_device_id(engine, &input_device_name))
        });
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
            .to_string();
        let session_id = format!("rec-{timestamp}");
        let project_name = self.project_session.name.clone();

        let config = JsStartRecordingConfig {
            project_root: project_root.to_string_lossy().into_owned(),
            project_name,
            session_id,
            timestamp,
            bpm: bpm.max(1.0) as f64,
            start_beat: start_beat.max(0.0) as f64,
            sample_rate: sample_rate.max(1),
            input_device_id,
            tracks,
            monitor_mix: !monitor_channels.is_empty(),
            monitor_channels,
        };

        if let Err(error) = engine.start_recording(config) {
            self.fail_recording_start(&error.to_string(), cx);
            return;
        }

        self.recording_start_beat = start_beat.max(0.0);
        self.audio_last_error = None;
        self.recording_ui_state = RecordingUiState::Recording;
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
        self.stop_recording_transport_ui(cx);

        let Some(engine) = self.audio_engine.clone() else {
            self.recording_ui_state = RecordingUiState::Failed {
                reason: "audio engine unavailable".to_string(),
            };
            cx.notify();
            return;
        };

        let engine_recording = engine.recording_status().active;
        if let Err(error) = engine.pause() {
            self.audio_last_error = Some(error.to_string());
            eprintln!("[audio] stop transport while recording failed: {error}");
        }
        self.audio_stats = Some(engine.stats());

        if !engine_recording {
            self.recording_ui_state = RecordingUiState::Idle;
            cx.notify();
            return;
        }

        self.recording_ui_state = RecordingUiState::Finalizing;
        cx.notify();

        let results = match engine.stop_recording() {
            Ok(results) => results,
            Err(error) => {
                self.audio_last_error = Some(error.to_string());
                self.recording_ui_state = RecordingUiState::Failed {
                    reason: error.to_string(),
                };
                eprintln!("[recording] stop failed: {error}");
                return;
            }
        };

        self.commit_recording_results(cx, results);
        if !matches!(self.recording_ui_state, RecordingUiState::Failed { .. }) {
            self.recording_ui_state = RecordingUiState::Idle;
            cx.notify();
        }
    }

    fn stop_recording_transport_ui(&mut self, cx: &mut Context<Self>) {
        let _ = self.timeline.update(cx, |timeline, cx| {
            let mut changed = false;
            if timeline.state.transport.recording {
                timeline.state.transport.recording = false;
                changed = true;
            }
            if timeline.state.transport.playing {
                timeline.state.transport.playing = false;
                changed = true;
            }
            if changed {
                cx.notify();
            }
        });
        self.finish_recording_preview(cx);
    }

    fn commit_recording_results(
        &mut self,
        cx: &mut Context<Self>,
        results: Vec<DAUx::types::JsRecordingResult>,
    ) {
        let bpm = self.timeline.read(cx).state.bpm;
        let owner = cx.entity().clone();
        let timeline = self.timeline.clone();
        let (generate_waveforms, recording_offset_ms) = {
            let settings = self.settings.read(cx);
            (
                settings
                    .current
                    .recording
                    .audio
                    .generate_waveform_after_record,
                settings.current.recording.audio.recording_offset_ms,
            )
        };
        let recording_offset_beats = recording_offset_ms as f32 / 1000.0 * bpm / 60.0;
        let mut import_paths: Vec<(PathBuf, String)> = Vec::new();
        let mut failed_tracks: Vec<String> = Vec::new();

        let _ = self.timeline.update(cx, |timeline, cx| {
            for result in &results {
                if !result.success {
                    failed_tracks.push(format!(
                        "{}: {}",
                        result.track_id,
                        result.error.as_deref().unwrap_or("unknown error")
                    ));
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
                    (result.start_beat as f32 + recording_offset_beats).max(0.0),
                    result.duration_seconds,
                    bpm,
                );
                if generate_waveforms {
                    import_paths.push((PathBuf::from(&result.file_path), result.file_path.clone()));
                }
                eprintln!(
                    "[recording] clip created id={clip_id} track={} path={}",
                    result.track_id, result.relative_path
                );
            }
            cx.notify();
        });

        if !failed_tracks.is_empty() {
            let reason = format!("recording finalize failed ({})", failed_tracks.join("; "));
            self.audio_last_error = Some(reason.clone());
            self.recording_ui_state = RecordingUiState::Failed { reason };
            cx.notify();
            return;
        }

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

    /// Poll the engine's recording preview ring and keep the temporary preview
    /// clip + its streamed waveform in sync (Part 1). Called every audio tick.
    pub(super) fn update_recording_preview(&mut self, cx: &mut Context<Self>) {
        if !self.timeline.read(cx).state.transport.recording {
            if self.recording_preview.is_some()
                && std::env::var_os("FUTUREBOARD_RECORDING_DEBUG").is_some()
            {
                eprintln!("[RecordingPreviewUI] ignored_late_peaks transport_recording=false");
            }
            self.finish_recording_preview(cx);
            return;
        }
        let Some(engine) = self.audio_engine.clone() else {
            return;
        };
        let info = engine.recording_preview_info();
        if !info.active {
            self.finish_recording_preview(cx);
            return;
        }
        let rec_id = info.recording_id as u64;

        // (Re)initialize the preview clip for a new take.
        let need_init = self
            .recording_preview
            .as_ref()
            .map(|p| p.recording_id != rec_id)
            .unwrap_or(true);
        if need_init {
            self.finish_recording_preview(cx);
            let track_id = {
                let timeline = self.timeline.read(cx);
                timeline
                    .state
                    .tracks
                    .iter()
                    .find(|t| t.armed && t.track_type == TrackType::Audio)
                    .map(|t| t.id.clone())
            };
            let Some(track_id) = track_id else {
                return;
            };
            let start_beat = self.recording_start_beat.max(0.0);
            let clip_id = RECORDING_PREVIEW_CLIP_ID.to_string();
            waveform_cache::clear_recording_preview(&clip_id);
            if std::env::var_os("FUTUREBOARD_RECORDING_DEBUG").is_some() {
                eprintln!("[RecordingPreviewUI] started id={rec_id} clip_id={clip_id}");
            }
            let (cid, tid) = (clip_id.clone(), track_id.clone());
            self.timeline.update(cx, |timeline, cx| {
                timeline
                    .state
                    .begin_recording_preview_clip(&cid, &tid, start_beat);
                cx.notify();
            });
            self.recording_preview = Some(RecordingPreviewUi {
                clip_id,
                recording_id: rec_id,
                track_id,
                start_beat,
                sample_rate: info.sample_rate,
                peaks_per_second: info.peaks_per_second.max(1),
                drained: 0,
                peaks: Vec::new(),
            });
        }

        let bpm = {
            let timeline = self.timeline.read(cx);
            timeline.state.bpm
        };

        // Drain newly produced peaks and republish the preview waveform.
        let mut length_update: Option<(String, f32)> = None;
        if let Some(p) = self.recording_preview.as_mut() {
            let new = engine.drain_recording_preview_peaks(p.drained as f64);
            if !new.is_empty() {
                p.drained += new.len() as u64;
                if std::env::var_os("FUTUREBOARD_RECORDING_DEBUG").is_some() {
                    eprintln!(
                        "[RecordingPreviewUI] peaks id={} count={}",
                        p.recording_id,
                        new.len()
                    );
                }
                p.peaks.extend(new.into_iter().map(|q| WaveformPeak {
                    min: q.min as f32,
                    max: q.max as f32,
                }));
                let wf = waveform_cache::preview_from_recording_peaks(
                    &p.peaks,
                    p.sample_rate,
                    p.peaks_per_second,
                );
                waveform_cache::set_recording_preview(&p.clip_id, std::sync::Arc::new(wf));
            }
            let length_seconds = p.peaks.len() as f64 / p.peaks_per_second.max(1) as f64;
            let length_beats = (length_seconds * bpm.max(1.0) as f64 / 60.0) as f32;
            length_update = Some((p.clip_id.clone(), length_beats));
        }
        if let Some((clip_id, length_beats)) = length_update {
            self.timeline.update(cx, |timeline, cx| {
                if timeline
                    .state
                    .set_recording_preview_clip_length(&clip_id, length_beats)
                {
                    cx.notify();
                }
            });
        }
    }

    /// Tear down the live recording preview clip + registry entry.
    pub(super) fn finish_recording_preview(&mut self, cx: &mut Context<Self>) {
        let Some(preview) = self.recording_preview.take() else {
            return;
        };
        if std::env::var_os("FUTUREBOARD_RECORDING_DEBUG").is_some() {
            eprintln!(
                "[RecordingPreviewUI] finished id={} active_recording_id=None",
                preview.recording_id
            );
        }
        waveform_cache::clear_recording_preview(&preview.clip_id);
        let clip_id = preview.clip_id;
        self.timeline.update(cx, |timeline, cx| {
            if timeline.state.remove_recording_preview_clip(&clip_id) {
                cx.notify();
            }
        });
    }

    fn fail_recording_start(&mut self, message: &str, cx: &mut Context<Self>) {
        self.audio_last_error = Some(message.to_string());
        self.recording_ui_state = RecordingUiState::Failed {
            reason: message.to_string(),
        };
        eprintln!("[recording] start blocked: {message}");
        cx.notify();
    }
}

impl StudioLayout {
    /// `(device id, input channel count)` for the currently selected global
    /// input device — used to populate the Inspector input-channel selector
    /// (roadmap Phase E). Falls back to the default input device, then the first
    /// enumerated input. `None` when the engine is unavailable or no inputs exist.
    pub(super) fn selected_input_device_channels(
        &self,
        cx: &Context<Self>,
    ) -> Option<(String, u32)> {
        let engine = self.audio_engine.as_ref()?;
        let wanted = self
            .settings
            .read(cx)
            .current
            .hardware
            .audio
            .device_in
            .clone();
        let devices = engine.list_input_devices();
        if !wanted.trim().is_empty() {
            if let Some(d) = devices.iter().find(|d| d.name == wanted || d.id == wanted) {
                return Some((d.id.clone(), d.channels));
            }
        }
        devices
            .iter()
            .find(|d| d.is_default)
            .or_else(|| devices.first())
            .map(|d| (d.id.clone(), d.channels))
    }

    /// `(device name, output channel count)` for the currently selected global
    /// output device, used to populate hardware output routes in the Inspector.
    pub(super) fn selected_output_device_channels(
        &self,
        cx: &Context<Self>,
    ) -> Option<(String, u32)> {
        let engine = self.audio_engine.as_ref()?;
        let wanted = self
            .settings
            .read(cx)
            .current
            .hardware
            .audio
            .device_out
            .clone();
        let devices = engine.list_output_devices();
        if !wanted.trim().is_empty() {
            if let Some(d) = devices.iter().find(|d| d.name == wanted || d.id == wanted) {
                return Some((d.name.clone(), d.channels));
            }
        }
        devices
            .iter()
            .find(|d| d.is_default)
            .or_else(|| devices.first())
            .map(|d| (d.name.clone(), d.channels))
    }
}

#[derive(Clone, Debug)]
struct RecordingInputDevice {
    id: String,
    name: String,
    channels: u32,
    is_default: bool,
}

#[derive(Clone, Debug)]
struct RecordingInputRoute {
    device_id: Option<String>,
    channels: Vec<u32>,
}

fn recording_input_channels_checked(
    track: &TrackState,
    devices: &[RecordingInputDevice],
    selected_device: Option<&RecordingInputDevice>,
) -> Result<RecordingInputRoute, String> {
    match &track.routing.input {
        TrackInputRouting::None => Err(format!(
            "{} has no input selected. Choose an input channel before recording.",
            track.name
        )),
        TrackInputRouting::AllInputs => {
            let Some(device) = selected_device else {
                return Err("no input device selected or available".to_string());
            };
            let channels = match track.routing.audio_format {
                TrackAudioFormat::Mono => vec![0],
                TrackAudioFormat::Stereo => vec![0, 1],
            };
            validate_channels(&track.name, device, &channels)?;
            Ok(RecordingInputRoute {
                device_id: Some(device.id.clone()),
                channels,
            })
        }
        TrackInputRouting::AudioDeviceChannel { device_id, channel } => {
            if track.routing.audio_format != TrackAudioFormat::Mono {
                return Err(format!(
                    "{} is stereo but has a mono input route. Choose a stereo input pair.",
                    track.name
                ));
            }
            let device = find_recording_input_device(devices, device_id).ok_or_else(|| {
                format!("{} input device is unavailable: {}", track.name, device_id)
            })?;
            let channels = vec![*channel];
            validate_channels(&track.name, device, &channels)?;
            Ok(RecordingInputRoute {
                device_id: Some(device.id.clone()),
                channels,
            })
        }
        TrackInputRouting::AudioDeviceChannels {
            device_id,
            channels,
        } => {
            if channels.is_empty() {
                return Err(format!("{} has no input channels selected.", track.name));
            }
            match track.routing.audio_format {
                TrackAudioFormat::Mono if channels.len() != 1 => {
                    return Err(format!(
                        "{} is mono but has a stereo/multi input route. Choose one input channel.",
                        track.name
                    ));
                }
                TrackAudioFormat::Stereo if channels.len() != 2 => {
                    return Err(format!(
                        "{} is stereo but has an incompatible input route. Choose a stereo input pair.",
                        track.name
                    ));
                }
                _ => {}
            }
            let device = find_recording_input_device(devices, device_id).ok_or_else(|| {
                format!("{} input device is unavailable: {}", track.name, device_id)
            })?;
            validate_channels(&track.name, device, channels)?;
            Ok(RecordingInputRoute {
                device_id: Some(device.id.clone()),
                channels: channels.clone(),
            })
        }
        TrackInputRouting::MidiDevice { .. } => Err(format!(
            "{} has a MIDI input route; choose an audio input channel before recording.",
            track.name
        )),
    }
}

fn validate_channels(
    track_name: &str,
    device: &RecordingInputDevice,
    channels: &[u32],
) -> Result<(), String> {
    for channel in channels {
        if *channel >= device.channels {
            return Err(format!(
                "{} input channel {} is unavailable on {}.",
                track_name,
                channel + 1,
                device.name
            ));
        }
    }
    Ok(())
}

fn select_recording_input_device<'a>(
    devices: &'a [RecordingInputDevice],
    wanted: &str,
) -> Option<&'a RecordingInputDevice> {
    if !wanted.trim().is_empty() {
        if let Some(device) = find_recording_input_device(devices, wanted) {
            return Some(device);
        }
    }
    devices
        .iter()
        .find(|device| device.is_default)
        .or_else(|| devices.first())
}

fn find_recording_input_device<'a>(
    devices: &'a [RecordingInputDevice],
    id_or_name: &str,
) -> Option<&'a RecordingInputDevice> {
    devices
        .iter()
        .find(|device| device.id == id_or_name || device.name == id_or_name)
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
