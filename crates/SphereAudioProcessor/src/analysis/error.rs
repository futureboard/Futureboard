//! Error type for the offline analysis path (model loading / inference).

/// Errors from analysis backends that can fail (e.g. loading or running a
/// learned classifier model). The pure-DSP path is infallible and simply
/// returns `None` when there is not enough signal.
#[derive(Debug, thiserror::Error)]
pub enum AnalysisError {
    #[error("model load failed: {0}")]
    ModelLoad(String),

    #[error("model inference failed: {0}")]
    Inference(String),

    #[error("classifier backend unavailable: {0}")]
    BackendUnavailable(&'static str),
}
