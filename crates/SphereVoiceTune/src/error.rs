use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq)]
pub enum VoiceTuneError {
    #[error("Invalid sample rate: {0}")]
    InvalidSampleRate(u32),
    #[error("Invalid buffer length or parameters: {0}")]
    InvalidParameters(String),
    #[error("Analysis failed: {0}")]
    AnalysisFailed(String),
}
