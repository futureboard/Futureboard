use super::params::StretchBackend;

#[derive(Debug, thiserror::Error)]
pub enum StretchError {
    #[error("backend unavailable: {0:?}")]
    BackendUnavailable(StretchBackend),

    #[error("invalid stretch params: {0}")]
    InvalidParams(String),

    #[error("backend processing failed: {0}")]
    BackendFailed(String),

    #[error("buffer length mismatch")]
    BufferLengthMismatch,
}
