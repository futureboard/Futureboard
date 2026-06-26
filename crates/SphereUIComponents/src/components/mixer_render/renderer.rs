//! Mixer renderer backend trait and selection.
//!
//! Mirrors [`crate::components::timeline::render::renderer`]. The mixer's dense,
//! repeated decoration (strip backgrounds, top accent bars, separators, selection
//! highlight) is drawn by a backend from a draw-only [`MixerRenderSnapshot`]; the
//! GPUI-paint backend batches it into a single `canvas` of `paint_quad`s instead
//! of one `div().bg()` per primitive per strip.

use gpui::AnyElement;

use super::snapshot::MixerRenderSnapshot;

/// Output of a mixer primitive render pass.
pub enum MixerRenderOutput {
    /// GPUI element tree (a batched `canvas`) composited by GPUI.
    Gpui(AnyElement),
    #[cfg(feature = "gpu-renderer")]
    /// Offscreen GPU frame — compositing into GPUI requires texture interop and
    /// is parked (see [`MIXER_WGPU_COMPOSITE_READY`]).
    WgpuOffscreen(super::wgpu_renderer::WgpuMixerOffscreenFrame),
}

/// Dense mixer-primitive renderer. Implementations must only **draw** from the
/// snapshot: no audio, no project, no routing, no layout.
pub trait MixerRenderer: Send {
    fn backend_name(&self) -> &'static str;

    /// Build the primitive layer that paints behind the channel strips.
    fn render(&mut self, snapshot: &MixerRenderSnapshot) -> MixerRenderOutput;
}

/// Active backend for mixer primitive rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MixerRendererBackend {
    GpuiPaint,
    #[cfg(feature = "gpu-renderer")]
    Wgpu,
}

/// Process-wide preference set once from saved Settings at startup, read by the
/// thread-local renderer construction in [`super::super::mixer_surface`].
static PREFERRED_BACKEND: std::sync::OnceLock<MixerRendererBackend> = std::sync::OnceLock::new();

/// The WGPU mixer path would render into an offscreen texture, but GPUI does not
/// composite that texture into the window on Windows (same limitation the
/// timeline documents). Keep WGPU disabled for user-visible mixer paint until
/// texture interop lands; the GPUI-paint batched-`canvas` path is the real win.
#[cfg(feature = "gpu-renderer")]
const MIXER_WGPU_COMPOSITE_READY: bool = false;

/// Called once at app startup with the user's saved UI-render choice.
/// Subsequent calls are no-ops (Settings is restart-gated).
pub fn set_preferred_mixer_backend(backend: MixerRendererBackend) {
    let _ = PREFERRED_BACKEND.set(backend);
}

impl MixerRendererBackend {
    pub fn from_env() -> Self {
        #[cfg(feature = "gpu-renderer")]
        {
            if std::env::var_os("FUTUREBOARD_MIXER_WGPU").is_some() {
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

pub fn create_mixer_renderer(backend: MixerRendererBackend) -> Box<dyn MixerRenderer> {
    match backend {
        MixerRendererBackend::GpuiPaint => {
            Box::new(super::gpui_paint::GpuiPaintMixerRenderer::new())
        }
        #[cfg(feature = "gpu-renderer")]
        MixerRendererBackend::Wgpu => Box::new(super::wgpu_renderer::WgpuMixerRenderer::new()),
    }
}

/// Preferred renderer with automatic fallback to GPUI paint when WGPU cannot
/// composite (always, today). Keeps the app usable regardless of the request.
pub fn create_mixer_renderer_with_fallback(
    preferred: MixerRendererBackend,
) -> (Box<dyn MixerRenderer>, MixerRendererBackend) {
    #[cfg(feature = "gpu-renderer")]
    {
        if preferred == MixerRendererBackend::Wgpu {
            if !MIXER_WGPU_COMPOSITE_READY {
                if std::env::var_os("FUTUREBOARD_GPU_RENDERER_DEBUG").is_some() {
                    eprintln!(
                        "[gpu-renderer] WGPU mixer requested, but texture compositing is not ready; using GPUI paint"
                    );
                }
                return (
                    Box::new(super::gpui_paint::GpuiPaintMixerRenderer::new()),
                    MixerRendererBackend::GpuiPaint,
                );
            }
            let wgpu = super::wgpu_renderer::WgpuMixerRenderer::new();
            if wgpu.is_available() {
                return (Box::new(wgpu), MixerRendererBackend::Wgpu);
            }
            eprintln!("[gpu-renderer] WGPU mixer renderer unavailable; falling back to GPUI paint");
        }
    }
    let _ = preferred;
    (
        Box::new(super::gpui_paint::GpuiPaintMixerRenderer::new()),
        MixerRendererBackend::GpuiPaint,
    )
}
