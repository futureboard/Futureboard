//! Runtime playback graph sent to the CPAL callback.
//!
//! The control thread builds this from an `EngineProjectSnapshot`, including
//! decoding supported media files.  The audio thread then owns a local clone of
//! the graph and can render without touching locks or parsing JSON.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use crate::audio_file::{load_audio_file, AudioFileBuffer};
use serde_json::Value;
use sphere_audio_plugins::{canonical_plugin_id, should_rebuild_state, AudioPluginDspState};

use crate::types::{EngineClipSnapshot, EngineProjectSnapshot};
use crate::vst3_processor::Vst3RuntimeProcessor;

#[derive(Debug, Clone)]
pub struct RuntimeTrack {
    pub id: String,
    pub track_type: String,
    pub volume: f32,
    pub pan: f32,
    pub muted: bool,
    pub solo: bool,
    pub preview_mode: RuntimePreviewMode,
    pub output_track_id: Option<String>,
    pub inserts: Vec<RuntimeInsert>,
    pub sends: Vec<RuntimeSend>,
    pub meter: Arc<RuntimeTrackMeter>,
    pub meter_peak_l: f32,
    pub meter_peak_r: f32,
    pub meter_sum_sq_l: f32,
    pub meter_sum_sq_r: f32,
    pub callback_insert_log_done: bool,
    pub callback_clip_route_log_done: bool,
    pub block_l: Vec<f32>,
    pub block_r: Vec<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimePreviewMode {
    Stereo,
    Mono,
    Mid,
    Side,
}

impl RuntimePreviewMode {
    #[inline]
    pub fn from_str(value: &str) -> Self {
        match value {
            "mono" => Self::Mono,
            "mid" => Self::Mid,
            "side" => Self::Side,
            _ => Self::Stereo,
        }
    }

    #[inline]
    pub fn from_code(value: f32) -> Self {
        match value as i32 {
            1 => Self::Mono,
            2 => Self::Mid,
            3 => Self::Side,
            _ => Self::Stereo,
        }
    }
}

#[derive(Debug, Default)]
pub struct RuntimeTrackMeter {
    peak_l: AtomicU32,
    peak_r: AtomicU32,
    rms_l: AtomicU32,
    rms_r: AtomicU32,
}

#[derive(Debug, Clone)]
pub struct RuntimeTrackMeterSnapshot {
    pub track_id: String,
    pub peak_l: f32,
    pub peak_r: f32,
    pub rms_l: f32,
    pub rms_r: f32,
}

impl RuntimeTrackMeter {
    #[inline]
    fn store(&self, peak_l: f32, peak_r: f32, rms_l: f32, rms_r: f32) {
        self.peak_l.store(f32_store(peak_l), Ordering::Relaxed);
        self.peak_r.store(f32_store(peak_r), Ordering::Relaxed);
        self.rms_l.store(f32_store(rms_l), Ordering::Relaxed);
        self.rms_r.store(f32_store(rms_r), Ordering::Relaxed);
    }

    #[inline]
    fn load(&self, track_id: &str) -> RuntimeTrackMeterSnapshot {
        RuntimeTrackMeterSnapshot {
            track_id: track_id.to_string(),
            peak_l: f32_load(self.peak_l.load(Ordering::Relaxed)),
            peak_r: f32_load(self.peak_r.load(Ordering::Relaxed)),
            rms_l: f32_load(self.rms_l.load(Ordering::Relaxed)),
            rms_r: f32_load(self.rms_r.load(Ordering::Relaxed)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeInsert {
    pub id: String,
    pub kind: String,
    pub enabled: bool,
    pub params: HashMap<String, Value>,
    pub dsp: InsertDspState,
    pub vst3: Option<Vst3RuntimeProcessor>,
    pub callback_process_log_done: bool,
    pub silent_process_blocks: u32,
    pub scratch_l: Vec<f32>,
    pub scratch_r: Vec<f32>,
}

pub type InsertDspState = AudioPluginDspState;

const DEFAULT_AUDIO_BLOCK_CAPACITY: usize = 8192;

#[derive(Debug, Clone)]
pub struct RuntimeSend {
    pub id: String,
    pub return_track_id: String,
    pub level: f32,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct RuntimeClip {
    pub id: String,
    pub track_id: String,
    pub start_sample: u64,
    pub duration_samples: u64,
    pub offset_seconds: f64,
    pub gain: f32,
    pub speed_ratio: f32,
    pub source: Arc<AudioFileBuffer>,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeProject {
    pub sample_rate: u32,
    pub tracks: Vec<RuntimeTrack>,
    pub clips: Vec<RuntimeClip>,
    pub has_solo: bool,
}

impl RuntimeProject {
    /// Build a RuntimeProject from a snapshot.
    ///
    /// `existing_vst3` — if provided, VST3 processors from a previous runtime
    /// whose insert ID + plugin path + class_id + sample_rate still match are
    /// REUSED (taken out of the map) rather than recreated.  This keeps the
    /// same C++ processor alive across project reloads so editor windows stay
    /// valid.  Any entries left in the map after build were not matched and will
    /// be dropped by the caller (triggering `sphere_daux_vst3_destroy`).
    pub fn build(
        snapshot: &EngineProjectSnapshot,
        output_sample_rate: u32,
        decoded_by_path: &mut HashMap<String, Arc<AudioFileBuffer>>,
        mut existing_vst3: Option<&mut HashMap<String, Vst3RuntimeProcessor>>,
    ) -> Self {
        let output_sample_rate = output_sample_rate.max(1);
        let beats_per_second = snapshot.bpm.max(1.0) / 60.0;
        let mut clips = Vec::new();
        let mut skipped_no_path = 0u32;
        let mut skipped_decode_err = 0u32;
        let mut loaded_from_cache = 0u32;
        let mut loaded_fresh = 0u32;

        for clip in &snapshot.clips {
            let Some(path) = clip.media_path.as_deref().filter(|p| !p.trim().is_empty()) else {
                eprintln!(
                    "[SphereAudio] clip '{}' (track={}) — no mediaPath, skipping",
                    clip.id, clip.track_id
                );
                skipped_no_path += 1;
                continue;
            };

            let source = match decoded_by_path.get(path) {
                Some(existing) => {
                    eprintln!(
                        "[SphereAudio] clip '{}' — cache hit: '{path}' ({} frames)",
                        clip.id, existing.frames
                    );
                    loaded_from_cache += 1;
                    Arc::clone(existing)
                }
                None => match load_audio_file(path) {
                    Ok(buffer) => {
                        eprintln!(
                            "[SphereAudio] clip '{}' — decoded: '{path}' {} frames @ {}Hz {} ch",
                            clip.id, buffer.frames, buffer.sample_rate, buffer.channels
                        );
                        loaded_fresh += 1;
                        let buffer = Arc::new(buffer);
                        decoded_by_path.insert(path.to_string(), Arc::clone(&buffer));
                        buffer
                    }
                    Err(e) => {
                        skipped_decode_err += 1;
                        eprintln!(
                            "[SphereAudio] clip '{}' — decode FAILED '{path}': {e}",
                            clip.id
                        );
                        continue;
                    }
                },
            };

            let Some(runtime_clip) = build_clip_runtime(
                clip,
                Arc::clone(&source),
                beats_per_second,
                output_sample_rate,
            ) else {
                skipped_decode_err += 1;
                continue;
            };
            clips.push(runtime_clip);
        }

        if skipped_no_path > 0 || skipped_decode_err > 0 || loaded_fresh > 0 {
            eprintln!(
                "[SphereAudio] RuntimeProject built: {} clips ready ({} cached, {} decoded), \
                 {} skipped (no path), {} decode errors",
                clips.len(),
                loaded_from_cache,
                loaded_fresh,
                skipped_no_path,
                skipped_decode_err,
            );
        }

        // Use an explicit loop so we can mutably borrow existing_vst3 on each insert.
        let mut tracks: Vec<RuntimeTrack> = Vec::with_capacity(snapshot.tracks.len());
        for t in &snapshot.tracks {
            let mut inserts: Vec<RuntimeInsert> = Vec::with_capacity(t.inserts.len());
            for insert in &t.inserts {
                let is_native_vst3 = insert.kind.eq_ignore_ascii_case("native-plugin")
                    && insert
                        .params
                        .get("format")
                        .and_then(Value::as_str)
                        .map(|f| f.eq_ignore_ascii_case("VST3"))
                        .unwrap_or(false);

                let vst3 = if is_native_vst3 {
                    let new_path = insert
                        .params
                        .get("modulePath")
                        .or_else(|| insert.params.get("path"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    let new_class_id = insert
                        .params
                        .get("classId")
                        .or_else(|| insert.params.get("class_id"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim()
                        .to_string();

                    // Try to reuse an existing processor matching insert ID +
                    // plugin path + class_id + sample_rate.
                    let reused: Option<Vst3RuntimeProcessor> =
                        if let Some(ref mut map) = existing_vst3 {
                            let can_reuse = map.get(&insert.id).map(|e| {
                                e.plugin_path().map(|p| p == new_path.as_str()).unwrap_or(false)
                                    && e.class_id()
                                        .map(|c| c == new_class_id.as_str())
                                        .unwrap_or(false)
                                    && e.sample_rate() == output_sample_rate
                                    && e.is_ready()
                            }).unwrap_or(false);
                            if can_reuse { map.remove(&insert.id) } else { None }
                        } else {
                            None
                        };

                    let reused_flag = reused.is_some();
                    let processor = reused.or_else(|| {
                        Vst3RuntimeProcessor::from_params(&insert.params, output_sample_rate)
                    });
                    let processor_handle = processor.as_ref().map(|p| p.handle_value()).unwrap_or(0);
                    eprintln!(
                        "[SphereAudio] native VST3 insert track='{}' insert='{}' pluginInstanceId='{}' reused={} ready={} processorHandle=0x{:x} path='{}'",
                        t.id,
                        insert.id,
                        insert.params.get("pluginInstanceId").and_then(Value::as_str).unwrap_or(&insert.id),
                        reused_flag,
                        processor.as_ref().map(|p| p.is_ready()).unwrap_or(false),
                        processor_handle,
                        insert.params.get("path").and_then(Value::as_str).unwrap_or(""),
                    );
                    processor
                } else {
                    None
                };

                inserts.push(RuntimeInsert {
                    id: insert.id.clone(),
                    kind: insert.kind.clone(),
                    enabled: insert.enabled,
                    params: insert.params.clone(),
                    dsp: InsertDspState::new(
                        canonical_plugin_id(&insert.kind),
                        &insert.params,
                        output_sample_rate,
                    ),
                    vst3,
                    callback_process_log_done: false,
                    silent_process_blocks: 0,
                    scratch_l: vec![0.0; DEFAULT_AUDIO_BLOCK_CAPACITY],
                    scratch_r: vec![0.0; DEFAULT_AUDIO_BLOCK_CAPACITY],
                });
            }

            tracks.push(RuntimeTrack {
                id: t.id.clone(),
                track_type: t.track_type.clone(),
                volume: t.volume.clamp(0.0, 2.0),
                pan: t.pan.clamp(-1.0, 1.0),
                muted: t.muted,
                solo: t.solo,
                preview_mode: RuntimePreviewMode::from_str(&t.preview_mode),
                output_track_id: t.output_track_id.clone(),
                inserts,
                sends: t
                    .sends
                    .iter()
                    .map(|send| RuntimeSend {
                        id: send.id.clone(),
                        return_track_id: send.return_track_id.clone(),
                        level: send.level.clamp(0.0, 2.0),
                        enabled: send.enabled,
                    })
                    .collect(),
                meter: Arc::new(RuntimeTrackMeter::default()),
                meter_peak_l: 0.0,
                meter_peak_r: 0.0,
                meter_sum_sq_l: 0.0,
                meter_sum_sq_r: 0.0,
                callback_insert_log_done: false,
                callback_clip_route_log_done: false,
                block_l: vec![0.0; DEFAULT_AUDIO_BLOCK_CAPACITY],
                block_r: vec![0.0; DEFAULT_AUDIO_BLOCK_CAPACITY],
            });
        }
        let has_solo = tracks.iter().any(|t| t.solo);
        let master_insert_count = tracks
            .iter()
            .find(|track| track.track_type == "master")
            .map(|track| track.inserts.len())
            .unwrap_or(0);
        eprintln!("[SphereAudio] RuntimeMaster inserts={master_insert_count}");
        for track in &tracks {
            let track_clips = clips.iter().filter(|clip| clip.track_id == track.id).count();
            eprintln!(
                "[SphereAudio] RuntimeTrack track={} clips={} inserts={}",
                track.id,
                track_clips,
                track.inserts.len()
            );
            if !track.inserts.is_empty() {
                for insert in &track.inserts {
                    let format = insert
                        .params
                        .get("format")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let path = insert
                        .params
                        .get("modulePath")
                        .or_else(|| insert.params.get("path"))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let class_id = insert
                        .params
                        .get("classId")
                        .or_else(|| insert.params.get("class_id"))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    eprintln!(
                        "[SphereAudio] RuntimeInsert id={} format={} path={} classId={} bypass={}",
                        insert.id,
                        format,
                        path,
                        class_id,
                        !insert.enabled
                    );
                }
            }
        }

        Self {
            sample_rate: output_sample_rate,
            tracks,
            clips,
            has_solo,
        }
    }

    #[inline]
    pub fn active_clip_count_at_sample(&self, project_sample: u64) -> usize {
        self.clips
            .iter()
            .filter(|clip| {
                project_sample >= clip.start_sample
                    && project_sample < clip.start_sample.saturating_add(clip.duration_samples)
            })
            .count()
    }

    #[inline]
    pub fn begin_meter_block(&mut self) {
        for track in &mut self.tracks {
            track.meter_peak_l = 0.0;
            track.meter_peak_r = 0.0;
            track.meter_sum_sq_l = 0.0;
            track.meter_sum_sq_r = 0.0;
        }
    }

    #[inline]
    pub fn accumulate_track_meter(&mut self, track_index: usize, l: f32, r: f32) {
        let Some(track) = self.tracks.get_mut(track_index) else {
            return;
        };
        let abs_l = l.abs();
        let abs_r = r.abs();
        track.meter_peak_l = track.meter_peak_l.max(abs_l);
        track.meter_peak_r = track.meter_peak_r.max(abs_r);
        track.meter_sum_sq_l += l * l;
        track.meter_sum_sq_r += r * r;
    }

    #[inline]
    pub fn end_meter_block(&mut self, frames: u64) {
        let frame_count = frames.max(1) as f32;
        for track in &mut self.tracks {
            let rms_l = (track.meter_sum_sq_l / frame_count).sqrt();
            let rms_r = (track.meter_sum_sq_r / frame_count).sqrt();
            track
                .meter
                .store(track.meter_peak_l, track.meter_peak_r, rms_l, rms_r);
        }
    }

    pub fn meter_snapshots(&self) -> Vec<RuntimeTrackMeterSnapshot> {
        self.tracks
            .iter()
            .map(|track| track.meter.load(&track.id))
            .collect()
    }

    #[inline]
    pub fn update_track_volume(&mut self, track_id: &str, volume: f32) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            track.volume = volume.clamp(0.0, 2.0);
        }
    }

    #[inline]
    pub fn update_track_pan(&mut self, track_id: &str, pan: f32) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            track.pan = pan.clamp(-1.0, 1.0);
        }
    }

    #[inline]
    pub fn update_track_mute(&mut self, track_id: &str, muted: bool) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            track.muted = muted;
        }
    }

    #[inline]
    pub fn update_track_solo(&mut self, track_id: &str, solo: bool) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            track.solo = solo;
            self.has_solo = self.tracks.iter().any(|t| t.solo);
        }
    }

    #[inline]
    pub fn update_track_preview_mode(&mut self, track_id: &str, mode: RuntimePreviewMode) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            track.preview_mode = mode;
        }
    }

    #[inline]
    pub fn update_insert_param(
        &mut self,
        track_id: &str,
        insert_id: &str,
        param_id: &str,
        value: f32,
    ) {
        let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) else {
            return;
        };
        let Some(insert) = track.inserts.iter_mut().find(|i| i.id == insert_id) else {
            return;
        };

        // "enabled" toggles bypass for all insert types.
        if param_id == "enabled" {
            insert.enabled = value >= 0.5;
            return;
        }

        // For native VST3 inserts: forward numeric param IDs to the C++ processor.
        // The web UI sends VST3 ParamIDs as decimal strings ("12345"), and values
        // are normalized (0..1) as required by IParameterChanges.
        if let Some(vst3) = insert.vst3.as_mut() {
            if let Ok(vst3_param_id) = param_id.parse::<u32>() {
                vst3.set_param(vst3_param_id, value as f64);
                insert.callback_process_log_done = false;
                insert.silent_process_blocks = 0;
                // Also persist in params map for snapshot/recall, then return —
                // built-in DSP state rebuild is not applicable to VST3 inserts.
                insert.params.insert(param_id.to_string(), Value::from(value as f64));
                return;
            }
        }

        // Built-in plugin insert: update params map and rebuild DSP state if needed.
        insert
            .params
            .insert(param_id.to_string(), Value::from(value as f64));
        let plugin_id = canonical_plugin_id(&insert.kind);
        if should_rebuild_state(plugin_id, param_id) {
            insert
                .dsp
                .rebuild(plugin_id, &insert.params, self.sample_rate);
        }
    }
}

fn build_clip_runtime(
    clip: &EngineClipSnapshot,
    source: Arc<AudioFileBuffer>,
    beats_per_second: f64,
    output_sample_rate: u32,
) -> Option<RuntimeClip> {
    if beats_per_second <= 0.0 || output_sample_rate == 0 {
        return None;
    }

    let start_seconds = clip.start_beat / beats_per_second;
    let duration_seconds = clip.duration_beats / beats_per_second;
    if duration_seconds <= 0.0 {
        return None;
    }

    let speed_ratio = clip
        .audio_process
        .as_ref()
        .map(|p| p.speed_ratio as f32)
        .unwrap_or(1.0)
        .clamp(0.01, 16.0);

    Some(RuntimeClip {
        id: clip.id.clone(),
        track_id: clip.track_id.clone(),
        start_sample: seconds_to_samples(start_seconds.max(0.0), output_sample_rate),
        duration_samples: seconds_to_samples(duration_seconds, output_sample_rate).max(1),
        offset_seconds: clip.offset_seconds.max(0.0),
        gain: clip.gain.clamp(0.0, 4.0),
        speed_ratio,
        source,
    })
}

#[inline]
fn seconds_to_samples(seconds: f64, sample_rate: u32) -> u64 {
    (seconds * sample_rate as f64).round().max(0.0) as u64
}

#[inline]
fn f32_store(v: f32) -> u32 {
    v.to_bits()
}

#[inline]
fn f32_load(v: u32) -> f32 {
    f32::from_bits(v)
}
