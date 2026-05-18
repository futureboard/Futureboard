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

use crate::types::{EngineClipSnapshot, EngineProjectSnapshot};

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
}

#[derive(Debug, Clone)]
pub struct InsertDspState {
    pub sample_rate: u32,
    pub eq_l: Vec<Biquad>,
    pub eq_r: Vec<Biquad>,
}

impl InsertDspState {
    fn new(kind: &str, params: &HashMap<String, Value>, sample_rate: u32) -> Self {
        let mut state = Self {
            sample_rate,
            eq_l: Vec::new(),
            eq_r: Vec::new(),
        };
        state.rebuild(kind, params, sample_rate);
        state
    }

    pub fn rebuild(&mut self, kind: &str, params: &HashMap<String, Value>, sample_rate: u32) {
        self.sample_rate = sample_rate.max(1);
        self.eq_l.clear();
        self.eq_r.clear();
        if !is_eq_kind(kind) {
            return;
        }

        for band in 1..=8 {
            let prefix = format!("band{band}");
            if !param_bool(params, &format!("{prefix}Active"), true) {
                continue;
            }
            let band_type = param_str(params, &format!("{prefix}Type"), "bell");
            let freq = param_f32(params, &format!("{prefix}Freq"), 1000.0).clamp(20.0, 20_000.0);
            let gain = param_f32(params, &format!("{prefix}Gain"), 0.0).clamp(-18.0, 18.0);
            let q = param_f32(params, &format!("{prefix}Q"), 1.0).clamp(0.1, 12.0);
            let Some(filter) = Biquad::from_eq_band(&band_type, freq, gain, q, self.sample_rate as f32) else {
                continue;
            };
            self.eq_l.push(filter.clone());
            self.eq_r.push(filter);
        }
    }
}

#[derive(Debug, Clone)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    fn from_eq_band(kind: &str, freq: f32, gain_db: f32, q: f32, sample_rate: f32) -> Option<Self> {
        let nyquist = sample_rate * 0.5;
        let f0 = freq.clamp(10.0, nyquist * 0.96);
        let q = q.clamp(0.1, 12.0);
        let w0 = std::f32::consts::TAU * f0 / sample_rate.max(1.0);
        let sin = w0.sin();
        let cos = w0.cos();
        let alpha = sin / (2.0 * q);
        let a = 10.0f32.powf(gain_db / 40.0);

        let (b0, b1, b2, a0, a1, a2) = match kind {
            "bell" | "peak" | "peaking" => (
                1.0 + alpha * a,
                -2.0 * cos,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cos,
                1.0 - alpha / a,
            ),
            "notch" => (
                1.0,
                -2.0 * cos,
                1.0,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
            "lowpass" | "lp" => (
                (1.0 - cos) * 0.5,
                1.0 - cos,
                (1.0 - cos) * 0.5,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
            "highpass" | "hp" => (
                (1.0 + cos) * 0.5,
                -(1.0 + cos),
                (1.0 + cos) * 0.5,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
            "lowshelf" | "ls" => make_shelf(true, cos, sin, a, q),
            "highshelf" | "hs" => make_shelf(false, cos, sin, a, q),
            _ => return None,
        };

        let inv_a0 = 1.0 / a0.max(1.0e-8);
        Some(Self {
            b0: b0 * inv_a0,
            b1: b1 * inv_a0,
            b2: b2 * inv_a0,
            a1: a1 * inv_a0,
            a2: a2 * inv_a0,
            z1: 0.0,
            z2: 0.0,
        })
    }

    #[inline]
    pub fn process(&mut self, input: f32) -> f32 {
        let output = self.b0 * input + self.z1;
        self.z1 = self.b1 * input - self.a1 * output + self.z2;
        self.z2 = self.b2 * input - self.a2 * output;
        if output.is_finite() { output } else { 0.0 }
    }
}

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
    pub fn build(
        snapshot: &EngineProjectSnapshot,
        output_sample_rate: u32,
        decoded_by_path: &mut HashMap<String, Arc<AudioFileBuffer>>,
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
                        eprintln!("[SphereAudio] clip '{}' — decode FAILED '{path}': {e}", clip.id);
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

        let tracks: Vec<RuntimeTrack> = snapshot
            .tracks
            .iter()
            .map(|t| RuntimeTrack {
                id: t.id.clone(),
                track_type: t.track_type.clone(),
                volume: t.volume.clamp(0.0, 2.0),
                pan: t.pan.clamp(-1.0, 1.0),
                muted: t.muted,
                solo: t.solo,
                preview_mode: RuntimePreviewMode::from_str(&t.preview_mode),
                output_track_id: t.output_track_id.clone(),
                inserts: t
                    .inserts
                    .iter()
                    .map(|insert| RuntimeInsert {
                        id: insert.id.clone(),
                        kind: insert.kind.clone(),
                        enabled: insert.enabled,
                        params: insert.params.clone(),
                        dsp: InsertDspState::new(&insert.kind, &insert.params, output_sample_rate),
                    })
                    .collect(),
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
            })
            .collect();
        let has_solo = tracks.iter().any(|t| t.solo);

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
            track.meter.store(track.meter_peak_l, track.meter_peak_r, rms_l, rms_r);
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
    pub fn update_insert_param(&mut self, track_id: &str, insert_id: &str, param_id: &str, value: f32) {
        let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) else {
            return;
        };
        let Some(insert) = track.inserts.iter_mut().find(|i| i.id == insert_id) else {
            return;
        };
        if param_id == "enabled" {
            insert.enabled = value >= 0.5;
            return;
        }
        insert
            .params
            .insert(param_id.to_string(), Value::from(value as f64));
        if is_eq_kind(&insert.kind) && (param_id == "power" || param_id.starts_with("band")) {
            insert.dsp.rebuild(&insert.kind, &insert.params, self.sample_rate);
        }
    }
}

#[inline]
fn is_eq_kind(kind: &str) -> bool {
    let kind = kind.to_ascii_lowercase();
    kind == "eq" || kind == "equz8" || kind.contains("eq")
}

fn make_shelf(low: bool, cos: f32, sin: f32, a: f32, q: f32) -> (f32, f32, f32, f32, f32, f32) {
    let slope = q.clamp(0.1, 1.0);
    let alpha = (sin * 0.5) * ((a + 1.0 / a) * (1.0 / slope - 1.0) + 2.0).max(0.0001).sqrt();
    let beta = 2.0 * a.sqrt() * alpha;
    if low {
        (
            a * ((a + 1.0) - (a - 1.0) * cos + beta),
            2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
            a * ((a + 1.0) - (a - 1.0) * cos - beta),
            (a + 1.0) + (a - 1.0) * cos + beta,
            -2.0 * ((a - 1.0) + (a + 1.0) * cos),
            (a + 1.0) + (a - 1.0) * cos - beta,
        )
    } else {
        (
            a * ((a + 1.0) + (a - 1.0) * cos + beta),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
            a * ((a + 1.0) + (a - 1.0) * cos - beta),
            (a + 1.0) - (a - 1.0) * cos + beta,
            2.0 * ((a - 1.0) - (a + 1.0) * cos),
            (a + 1.0) - (a - 1.0) * cos - beta,
        )
    }
}

fn param_f32(params: &HashMap<String, Value>, key: &str, fallback: f32) -> f32 {
    params
        .get(key)
        .and_then(|v| v.as_f64())
        .map(|v| v as f32)
        .unwrap_or(fallback)
}

fn param_bool(params: &HashMap<String, Value>, key: &str, fallback: bool) -> bool {
    params
        .get(key)
        .and_then(|v| v.as_bool().or_else(|| v.as_f64().map(|n| n >= 0.5)))
        .unwrap_or(fallback)
}

fn param_str(params: &HashMap<String, Value>, key: &str, fallback: &str) -> String {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or(fallback)
        .to_ascii_lowercase()
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
