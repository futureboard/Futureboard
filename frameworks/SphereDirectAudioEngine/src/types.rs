use napi_derive::napi;
use serde::{Deserialize, Serialize};

// ── N-API–visible types ────────────────────────────────────────────────────────
// These cross the Rust/JS boundary via napi-derive.  Field names use camelCase
// so they arrive at JS looking natural.

#[napi(object)]
#[derive(Debug, Default)]
pub struct JsSphereAudioStatus {
    pub available: bool,
    pub running: bool,
    pub stream_open: bool,
    pub transport_playing: bool,
    pub position_seconds: f64,
    pub version: String,
    pub backend_name: String,
    pub sample_rate: u32,
    pub buffer_size: u32,
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub last_error: Option<String>,
}

#[napi(object)]
#[derive(Debug, Clone)]
pub struct JsAudioDeviceInfo {
    pub id: String,
    pub name: String,
    pub kind: String, // "input" | "output"
    pub channels: u32,
    pub default_sample_rate: u32,
    pub is_default: bool,
    pub backend: String,
}

#[napi(object)]
#[derive(Debug, Default)]
pub struct JsDeviceOpenConfig {
    pub input_device_id: Option<String>,
    pub output_device_id: Option<String>,
    pub sample_rate: Option<u32>,
    pub buffer_size: Option<u32>,
}

#[napi(object)]
#[derive(Debug, Default, Clone)]
pub struct JsMeterSnapshot {
    pub master_peak_l: f64,
    pub master_peak_r: f64,
    pub master_rms_l: f64,
    pub master_rms_r: f64,
}

// ── Internal (non-napi) serializable types ────────────────────────────────────
// These live purely on the Rust side and are used for project snapshots
// passed as JSON strings from the JS side.

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineProjectSnapshot {
    pub project_id: String,
    #[serde(default)]
    pub project_root: Option<String>,
    pub bpm: f64,
    pub time_signature: [u32; 2],
    pub sample_rate: u32,
    pub tracks: Vec<EngineTrackSnapshot>,
    pub clips: Vec<EngineClipSnapshot>,
    pub routing: EngineRoutingSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineTrackSnapshot {
    pub id: String,
    #[serde(rename = "type")]
    pub track_type: String,
    pub volume: f32,
    pub pan: f32,
    pub muted: bool,
    pub solo: bool,
    pub armed: bool,
    pub output_track_id: Option<String>,
    pub inserts: Vec<EngineInsertSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineInsertSnapshot {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub enabled: bool,
    pub params: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineClipSnapshot {
    pub id: String,
    pub track_id: String,
    pub asset_id: String,
    pub media_path: Option<String>,
    pub start_beat: f64,
    pub duration_beats: f64,
    pub offset_seconds: f64,
    pub gain: f32,
    #[serde(default)]
    pub fades: Option<EngineFadeSnapshot>,
    #[serde(default)]
    pub audio_process: Option<EngineClipAudioProcess>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineFadeSnapshot {
    pub in_duration: f64,
    pub out_duration: f64,
    pub in_curve: String,
    pub out_curve: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineClipAudioProcess {
    pub speed_ratio: f64,
    pub pitch_semitones: f64,
    pub preserve_pitch: bool,
    pub mode: String,
    pub quality: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineRoutingSnapshot {
    pub master_output_device: Option<String>,
    pub sample_rate: u32,
    pub buffer_size: u32,
}

/// Mutable engine status stored inside the engine under a lock.
/// Not exposed to JS directly — converted to JsSphereAudioStatus on read.
#[derive(Debug, Default, Clone)]
pub struct EngineStatus {
    pub stream_open: bool,
    pub running: bool,
    pub sample_rate: u32,
    pub buffer_size: u32,
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub last_error: Option<String>,
    pub loaded_project_id: Option<String>,
}

/// Debug state snapshot returned by `getDebugInfo()`.
/// Exposes the internal runtime graph so JS can verify the engine is loaded.
#[napi(object)]
#[derive(Debug, Default)]
pub struct JsEngineDebugInfo {
    /// Project ID from the last loaded snapshot.
    pub project_id: Option<String>,
    /// Number of tracks in the current runtime graph.
    pub loaded_tracks: u32,
    /// Number of clips in the current runtime graph (only clips with resolved paths).
    pub loaded_clips: u32,
    /// Number of clips whose audio buffer has frames > 0 (successfully decoded).
    pub ready_clips: u32,
    /// Whether the transport is currently playing.
    pub is_playing: bool,
    /// Current transport position in seconds.
    pub position_seconds: f64,
    /// Whether any track has solo enabled.
    pub has_solo: bool,
    /// Human-readable summary of each loaded clip (id, trackId, startSec, durationSec, frames).
    pub clip_summaries: Vec<String>,
}
