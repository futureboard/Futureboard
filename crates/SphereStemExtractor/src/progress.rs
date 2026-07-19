use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use crate::stems::StemKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StemExtractStage {
    Preparing,
    LoadingModel,
    Separating,
    Writing,
    Complete,
}

impl StemExtractStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preparing => "Preparing",
            Self::LoadingModel => "Loading model",
            Self::Separating => "Separating stems",
            Self::Writing => "Writing stems",
            Self::Complete => "Complete",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StemExtractProgress {
    pub stage: StemExtractStage,
    /// 0.0 … 100.0
    pub percent: f32,
    pub current_stem: Option<StemKind>,
    pub detail: String,
}

impl StemExtractProgress {
    pub fn new(stage: StemExtractStage, percent: f32, detail: impl Into<String>) -> Self {
        Self {
            stage,
            percent: percent.clamp(0.0, 100.0),
            current_stem: None,
            detail: detail.into(),
        }
    }

    pub fn with_stem(mut self, stem: StemKind) -> Self {
        self.current_stem = Some(stem);
        self
    }
}

#[derive(Clone, Debug, Default)]
pub struct StemExtractCancelToken {
    cancelled: Arc<AtomicBool>,
}

impl StemExtractCancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}
