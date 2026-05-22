use std::fmt;

/// All errors that can originate from the SphereDirectAudioEngine.
#[derive(Debug)]
pub enum SphereAudioError {
    BackendUnavailable(String),
    DeviceNotFound(String),
    StreamOpenFailed(String),
    StreamStartFailed(String),
    EngineNotOpen,
    InvalidConfig(String),
    ProjectDeserialize(String),
    NativeError(String),
}

impl fmt::Display for SphereAudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BackendUnavailable(s) => write!(f, "Audio backend unavailable: {s}"),
            Self::DeviceNotFound(s) => write!(f, "Device not found: {s}"),
            Self::StreamOpenFailed(s) => write!(f, "Stream open failed: {s}"),
            Self::StreamStartFailed(s) => write!(f, "Stream start failed: {s}"),
            Self::EngineNotOpen => write!(f, "Engine stream is not open"),
            Self::InvalidConfig(s) => write!(f, "Invalid configuration: {s}"),
            Self::ProjectDeserialize(s) => write!(f, "Project deserialization failed: {s}"),
            Self::NativeError(s) => write!(f, "Native error: {s}"),
        }
    }
}

impl std::error::Error for SphereAudioError {}

/// Convert to a plain string suitable for crossing the N-API boundary.
impl From<SphereAudioError> for napi::Error {
    fn from(e: SphereAudioError) -> napi::Error {
        napi::Error::new(napi::Status::GenericFailure, e.to_string())
    }
}
