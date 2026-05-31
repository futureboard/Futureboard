//! Renderer backend trait and selection.

use gpui::AnyElement;

use super::snapshot::TimelineRenderSnapshot;

/// Output of an arrangement-surface render pass.
pub enum TimelineRenderOutput {
    /// GPUI element tree (canvas / div quads) composited by GPUI.
    Gpui(AnyElement),
    #[cfg(feature = "gpu-renderer")]
    /// Offscreen GPU frame — compositing into GPUI requires Blade/texture interop.
    WgpuOffscreen(super::wgpu_renderer::WgpuOffscreenFrame),
}

/// Dense timeline arrangement renderer (grid, lanes, clips, waveforms, playhead).
///
/// Implementations must only **draw** from [`TimelineRenderSnapshot`]:
/// no decode, no peak generation, no layout.
pub trait TimelineRenderer: Send {
    fn backend_name(&self) -> &'static str;

    /// Render the scrollable arrangement body (grid region width × height).
    fn render_arrangement(&mut self, snapshot: &TimelineRenderSnapshot) -> TimelineRenderOutput;
}

/// Active backend for arrangement rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineRendererBackend {
    GpuiPaint,
    #[cfg(feature = "gpu-renderer")]
    Wgpu,
}

/// Process-wide renderer preference set from saved Settings at startup.
/// The thread-local renderer construction in `timeline_surface.rs` reads
/// this on first use. `FUTUREBOARD_WGPU_TIMELINE=1` continues to win as
/// a developer override.
static PREFERRED_BACKEND: std::sync::OnceLock<TimelineRendererBackend> = std::sync::OnceLock::new();

/// Called once at app startup with the user's saved Renderer choice.
/// Settings UI is gated on a restart marker, so we never mutate this
/// after the first call. Subsequent calls are no-ops.
pub fn set_preferred_backend(backend: TimelineRendererBackend) {
    let _ = PREFERRED_BACKEND.set(backend);
}

impl TimelineRendererBackend {
    pub fn from_env() -> Self {
        #[cfg(feature = "gpu-renderer")]
        {
            if std::env::var_os("FUTUREBOARD_WGPU_TIMELINE").is_some() {
                return Self::Wgpu;
            }
        }
        if let Some(saved) = PREFERRED_BACKEND.get() {
            return *saved;
        }
        Self::GpuiPaint
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::GpuiPaint => "gpui-paint",
            #[cfg(feature = "gpu-renderer")]
            Self::Wgpu => "wgpu-offscreen",
        }
    }
}

pub fn create_timeline_renderer(backend: TimelineRendererBackend) -> Box<dyn TimelineRenderer> {
    match backend {
        TimelineRendererBackend::GpuiPaint => {
            Box::new(super::gpui_paint::GpuiPaintTimelineRenderer::new())
        }
        #[cfg(feature = "gpu-renderer")]
        TimelineRendererBackend::Wgpu => {
            Box::new(super::wgpu_renderer::WgpuTimelineRenderer::new())
        }
    }
}

/// Preferred renderer with automatic fallback when WGPU cannot composite.
pub fn create_timeline_renderer_with_fallback(
    preferred: TimelineRendererBackend,
) -> (Box<dyn TimelineRenderer>, TimelineRendererBackend) {
    #[cfg(feature = "gpu-renderer")]
    {
        if preferred == TimelineRendererBackend::Wgpu {
            let mut wgpu = super::wgpu_renderer::WgpuTimelineRenderer::new();
            if wgpu.is_available() {
                return (Box::new(wgpu), TimelineRendererBackend::Wgpu);
            }
            eprintln!(
                "[gpu-renderer] WGPU timeline renderer unavailable; falling back to GPUI paint"
            );
        }
    }
    let _ = preferred;
    (
        Box::new(super::gpui_paint::GpuiPaintTimelineRenderer::new()),
        TimelineRendererBackend::GpuiPaint,
    )
}
