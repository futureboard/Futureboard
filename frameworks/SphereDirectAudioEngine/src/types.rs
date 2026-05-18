use napi_derive::napi;
use serde::{Deserialize, Serialize};

// ── DAUx backend selection types ──────────────────────────────────────────────

/// Information about one available DAUx backend.
#[napi(object)]
#[derive(Debug, Default, Clone)]
pub struct JsDauxBackendInfo {
    /// Machine-readable id: "auto" | "wasapi-shared" | "wasapi-exclusive" | "coreaudio" | "alsa" | "mme"
    pub id: String,
    /// Human-readable name: "DAUx WASAPI Shared", etc.
    pub name: String,
    /// Whether this backend is currently usable on this platform.
    pub available: bool,
    /// Whether this is the platform default.
    pub is_default: bool,
    /// Short description.
    pub description: String,
}

/// Configuration for selecting / opening a DAUx backend.
#[napi(object)]
#[derive(Debug, Default, Clone)]
pub struct JsDauxConfig {
    /// Backend id string (see `JsDauxBackendInfo.id`).
    pub backend_id: String,
    /// Target output device name / id.  Empty = system default.
    pub output_device_id: Option<String>,
    /// Target sample rate in Hz (0 = device default).
    pub sample_rate: Option<u32>,
    /// Target buffer size in frames (0 = driver default).
    pub buffer_size: Option<u32>,
    /// Enable MMCSS "Pro Audio" thread priority on Windows.
    pub mmcss_priority: bool,
    /// Safe mode: use larger buffer to reduce glitches.
    pub safe_mode: bool,
}

/// Runtime status of the active DAUx backend.
#[napi(object)]
#[derive(Debug, Default, Clone)]
pub struct JsDauxStatus {
    /// Active backend id.
    pub backend_id: String,
    /// Active backend human-readable name.
    pub backend_name: String,
    /// Active output device name.
    pub output_device: Option<String>,
    /// Active sample rate (Hz).
    pub sample_rate: u32,
    /// Active buffer size (frames).
    pub buffer_size: u32,
    /// Estimated output latency (ms) = buffer_frames / sample_rate * 1000.
    pub estimated_latency_ms: f64,
    /// Number of audio glitches / underruns since the stream was opened.
    pub glitch_count: f64,
    /// MMCSS priority active on audio thread (Windows only).
    pub mmcss_active: bool,
    /// Last backend error (e.g. WASAPI Exclusive failed reason). Cleared on success.
    pub last_error: Option<String>,
}

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
pub struct JsTrackMeterSnapshot {
    pub track_id: String,
    pub peak_l: f64,
    pub peak_r: f64,
    pub rms_l: f64,
    pub rms_r: f64,
}

#[napi(object)]
#[derive(Debug, Default, Clone)]
pub struct JsMeterSnapshot {
    pub tracks: Vec<JsTrackMeterSnapshot>,
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
    #[serde(default = "default_preview_mode")]
    pub preview_mode: String,
    pub output_track_id: Option<String>,
    pub inserts: Vec<EngineInsertSnapshot>,
    #[serde(default)]
    pub sends: Vec<EngineSendSnapshot>,
}

fn default_preview_mode() -> String {
    "stereo".to_string()
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
pub struct EngineSendSnapshot {
    pub id: String,
    pub return_track_id: String,
    pub level: f32,
    pub enabled: bool,
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
    /// Last WASAPI / backend error, displayed in Audio Settings UI.
    pub last_daux_error: Option<String>,
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
