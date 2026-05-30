use serde::{Deserialize, Serialize};

// ── DAUx backend selection types ──────────────────────────────────────────────

#[cfg(feature = "napi")]
use napi_derive::napi;

/// Information about one available DAUx backend.
#[cfg_attr(feature = "napi", napi(object))]
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
#[cfg_attr(feature = "napi", napi(object))]
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
#[cfg_attr(feature = "napi", napi(object))]
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

#[cfg_attr(feature = "napi", napi(object))]
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

#[cfg_attr(feature = "napi", napi(object))]
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

#[cfg_attr(feature = "napi", napi(object))]
#[derive(Debug, Default)]
pub struct JsDeviceOpenConfig {
    pub input_device_id: Option<String>,
    pub output_device_id: Option<String>,
    pub sample_rate: Option<u32>,
    pub buffer_size: Option<u32>,
}

#[cfg_attr(feature = "napi", napi(object))]
#[derive(Debug, Default, Clone)]
pub struct JsTrackMeterSnapshot {
    pub track_id: String,
    pub peak_l: f64,
    pub peak_r: f64,
    pub rms_l: f64,
    pub rms_r: f64,
}

#[cfg_attr(feature = "napi", napi(object))]
#[derive(Debug, Default, Clone)]
pub struct JsMeterSnapshot {
    pub tracks: Vec<JsTrackMeterSnapshot>,
    pub master_peak_l: f64,
    pub master_peak_r: f64,
    pub master_rms_l: f64,
    pub master_rms_r: f64,
}

#[cfg_attr(feature = "napi", napi(object))]
#[derive(Debug, Default, Clone)]
pub struct JsWavPeakResult {
    pub file_id: String,
    pub sample_rate: u32,
    pub channel_count: u32,
    pub duration: f64,
    pub samples_per_peak: u32,
    pub peak_count: u32,
    /// Interleaved Int16 min/max pairs per peak/channel, widened for N-API.
    pub peaks: Vec<i32>,
}

#[cfg_attr(feature = "napi", napi(object))]
#[derive(Debug, Default, Clone)]
pub struct JsAudioFileInfo {
    pub path: String,
    pub sample_rate: u32,
    pub channel_count: u32,
    pub total_frames: f64,
    pub duration_seconds: f64,
    pub format: String,
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
    /// MIDI clips (Phase 2). Defaulted so older snapshots without the field
    /// still deserialize. Notes are stored relative to the clip start; the
    /// runtime converts them to absolute project beats/samples at build time.
    #[serde(default)]
    pub midi_clips: Vec<EngineMidiClipSnapshot>,
    pub routing: EngineRoutingSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineMidiClipSnapshot {
    pub id: String,
    pub track_id: String,
    pub start_beat: f64,
    pub length_beats: f64,
    pub notes: Vec<EngineMidiNoteSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineMidiNoteSnapshot {
    pub id: u64,
    pub pitch: u8,
    /// Start beat relative to the clip start.
    pub start_beat: f64,
    pub length_beats: f64,
    pub velocity: u8,
    #[serde(default)]
    pub channel: u8,
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
    /// `true` taps the signal before the source track fader; `false`
    /// (default) taps post-fader. Phase 3.
    #[serde(default)]
    pub pre_fader: bool,
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

// ── Recording types ───────────────────────────────────────────────────────────

/// Config for one armed track being recorded.
#[cfg_attr(feature = "napi", napi(object))]
#[derive(Debug, Default, Clone)]
pub struct JsRecordingTrackConfig {
    pub track_id: String,
    /// 0-based input channel indices (e.g., [0, 1] for the first stereo pair).
    pub input_channels: Vec<u32>,
    /// Human-readable track name — used to derive the output filename.
    pub name: String,
}

/// Full config passed to `startRecording()`.
#[cfg_attr(feature = "napi", napi(object))]
#[derive(Debug, Default, Clone)]
pub struct JsStartRecordingConfig {
    /// Absolute path to the project folder root.
    pub project_root: String,
    /// Unique ID for this recording session (used to name temp files).
    pub session_id: String,
    pub bpm: f64,
    pub start_beat: f64,
    pub sample_rate: u32,
    /// Input device name/id (None = system default).
    pub input_device_id: Option<String>,
    /// Armed tracks to record.
    pub tracks: Vec<JsRecordingTrackConfig>,
}

/// Per-track result returned by `stopRecording()`.
#[cfg_attr(feature = "napi", napi(object))]
#[derive(Debug, Default, Clone)]
pub struct JsRecordingResult {
    pub track_id: String,
    /// Absolute path to the finalized WAV file.
    pub file_path: String,
    /// Path relative to project root (e.g., "Media/Audio/Kick Rec 0001.wav").
    pub relative_path: String,
    /// Transport beat at which recording started.
    pub start_beat: f64,
    pub duration_seconds: f64,
    pub sample_rate: u32,
    pub channels: u32,
    pub success: bool,
    pub error: Option<String>,
}

/// Snapshot of recording state for UI polling.
#[cfg_attr(feature = "napi", napi(object))]
#[derive(Debug, Default, Clone)]
pub struct JsRecordingStatus {
    pub active: bool,
    pub duration_seconds: f64,
    pub track_count: u32,
}

/// Debug state snapshot returned by `getDebugInfo()`.
/// Exposes the internal runtime graph so JS can verify the engine is loaded.
#[cfg_attr(feature = "napi", napi(object))]
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
    /// Human-readable summary of inserts, including whether native VST3 processors are active.
    pub insert_summaries: Vec<String>,
}
