//! Engine error types following spec section 24.

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EngineError {
    NotInitialized,
    InvalidCommand(String),
    InvalidTrackId(String),
    InvalidClipId(String),
    InvalidDeviceId(String),
    InvalidParam { device: String, param: String },
    AssetNotLoaded(String),
    AudioDeviceError(String),
    WasmLoadFailed(String),
    InternalError(String),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInitialized => write!(f, "ENGINE_NOT_INITIALIZED"),
            Self::InvalidCommand(msg) => write!(f, "INVALID_COMMAND: {msg}"),
            Self::InvalidTrackId(id) => write!(f, "INVALID_TRACK_ID: {id}"),
            Self::InvalidClipId(id) => write!(f, "INVALID_CLIP_ID: {id}"),
            Self::InvalidDeviceId(id) => write!(f, "INVALID_DEVICE_ID: {id}"),
            Self::InvalidParam { device, param } => {
                write!(f, "INVALID_PARAM: {device}/{param}")
            }
            Self::AssetNotLoaded(id) => write!(f, "ASSET_NOT_LOADED: {id}"),
            Self::AudioDeviceError(msg) => write!(f, "AUDIO_DEVICE_ERROR: {msg}"),
            Self::WasmLoadFailed(msg) => write!(f, "WASM_LOAD_FAILED: {msg}"),
            Self::InternalError(msg) => write!(f, "INTERNAL_ERROR: {msg}"),
        }
    }
}

impl std::error::Error for EngineError {}

pub type EngineResult<T> = Result<T, EngineError>;
