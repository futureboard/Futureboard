//! Engine commands — serializable messages from UI to DSP engine.
//!
//! Matches the shared engine command protocol from spec section 5.

use serde::{Deserialize, Serialize};

use crate::ids::{DeviceId, TrackId};
use crate::params::ParamValue;

/// All commands the engine can receive.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EngineCommand {
    // ── Transport ────────────────────────────────────────────
    Init {
        sample_rate: f64,
        max_block_size: usize,
        channel_count: usize,
        bpm: f64,
    },
    Play {
        #[serde(default)]
        position_beat: Option<f64>,
    },
    Pause,
    Stop,
    SeekBeat {
        beat: f64,
    },
    SetBpm {
        bpm: f64,
    },
    SetLoop {
        enabled: bool,
        start_beat: f64,
        end_beat: f64,
    },
    SetTimeSignature {
        numerator: u32,
        denominator: u32,
    },

    // ── Tracks ───────────────────────────────────────────────
    CreateTrack {
        track_id: TrackId,
        #[serde(default = "default_volume")]
        volume: f32,
        #[serde(default)]
        pan: f32,
        #[serde(default)]
        muted: bool,
        #[serde(default)]
        solo: bool,
    },
    RemoveTrack {
        track_id: TrackId,
    },
    SetTrackVolume {
        track_id: TrackId,
        volume: f32,
    },
    SetTrackPan {
        track_id: TrackId,
        pan: f32,
    },
    SetTrackMute {
        track_id: TrackId,
        muted: bool,
    },
    SetTrackSolo {
        track_id: TrackId,
        solo: bool,
    },

    // ── Devices ──────────────────────────────────────────────
    AddInsertDevice {
        track_id: TrackId,
        device_id: DeviceId,
        device_type: String,
        #[serde(default)]
        index: Option<usize>,
    },
    RemoveInsertDevice {
        track_id: TrackId,
        device_id: DeviceId,
    },
    SetInsertEnabled {
        track_id: TrackId,
        device_id: DeviceId,
        enabled: bool,
    },
    SetInsertParam {
        track_id: TrackId,
        device_id: DeviceId,
        param: String,
        value: ParamValue,
    },

    // ── Master ───────────────────────────────────────────────
    SetMasterVolume {
        volume: f32,
    },

    // ── Status ───────────────────────────────────────────────
    GetStatus,
    Ping,
}

fn default_volume() -> f32 {
    1.0
}

/// Result of handling a command.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum CommandResult {
    Ok {
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<serde_json::Value>,
    },
    Error {
        code: String,
        message: String,
    },
}

impl CommandResult {
    pub fn ok() -> Self {
        Self::Ok { data: None }
    }

    pub fn ok_with(data: serde_json::Value) -> Self {
        Self::Ok { data: Some(data) }
    }

    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Error {
            code: code.into(),
            message: message.into(),
        }
    }
}
