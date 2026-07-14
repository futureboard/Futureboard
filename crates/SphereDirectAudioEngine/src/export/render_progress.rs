//! Progress + cancellation primitives shared by the offline renderer and the
//! arrangement exporter. Plain data only — safe to move across threads and to
//! hand to a UI without any engine borrow.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportStage {
    Preparing,
    Rendering,
    AnalyzingPeak,
    Encoding,
    Finalizing,
    Complete,
    Failed,
    Cancelled,
}

impl ExportStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preparing => "Preparing",
            Self::Rendering => "Rendering",
            Self::AnalyzingPeak => "Analyzing peak",
            Self::Encoding => "Encoding",
            Self::Finalizing => "Finalizing",
            Self::Complete => "Complete",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
        }
    }

    /// Whether the job has reached a terminal state.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Complete | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone)]
pub struct ExportProgress {
    pub rendered_frames: u64,
    pub total_frames: u64,
    pub percent: f32,
    pub stage: ExportStage,
}

impl ExportProgress {
    pub fn new(stage: ExportStage, rendered_frames: u64, total_frames: u64) -> Self {
        let percent = if total_frames == 0 {
            if stage.is_terminal() {
                100.0
            } else {
                0.0
            }
        } else {
            (rendered_frames as f64 / total_frames as f64 * 100.0).clamp(0.0, 100.0) as f32
        };
        Self {
            rendered_frames,
            total_frames,
            percent,
            stage,
        }
    }

    pub fn stage_only(stage: ExportStage, total_frames: u64) -> Self {
        let rendered = if stage.is_terminal() { total_frames } else { 0 };
        Self::new(stage, rendered, total_frames)
    }
}

/// Cheap, cloneable cancellation flag. Cloning shares the same flag, so the UI
/// can hold one handle while the worker thread polls another.
#[derive(Debug, Clone, Default)]
pub struct ExportCancelToken(Arc<AtomicBool>);

impl ExportCancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_clamps_0_to_100() {
        let p = ExportProgress::new(ExportStage::Rendering, 50, 100);
        assert!((p.percent - 50.0).abs() < 0.001);
        let over = ExportProgress::new(ExportStage::Rendering, 200, 100);
        assert_eq!(over.percent, 100.0);
        let zero = ExportProgress::new(ExportStage::Rendering, 0, 0);
        assert_eq!(zero.percent, 0.0);
        let done = ExportProgress::new(ExportStage::Complete, 0, 0);
        assert_eq!(done.percent, 100.0);
    }

    #[test]
    fn cancel_token_is_shared_across_clones() {
        let token = ExportCancelToken::new();
        let clone = token.clone();
        assert!(!clone.is_cancelled());
        token.cancel();
        assert!(clone.is_cancelled());
    }
}
