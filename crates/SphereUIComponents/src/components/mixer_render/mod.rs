//! Hybrid mixer rendering: draw-only snapshots + pluggable backends.
//!
//! - [`MixerRenderSnapshot`] — read-only frame description (built on UI thread)
//! - [`MixerRenderer`] — GPUI batched-`canvas` paint, or parked offscreen WGPU
//!
//! Native GPUI stays responsible for layout, input, focus, text, buttons,
//! faders, menus, IME. Only the dense, repeated strip decoration (backgrounds,
//! accent bars, separators, selection) routes here. Mirrors
//! [`crate::components::timeline::render`].

pub mod gpui_paint;
pub mod renderer;
pub mod snapshot;
#[cfg(feature = "gpu-renderer")]
pub mod wgpu_renderer;

pub use gpui_paint::GpuiPaintMixerRenderer;
pub use renderer::{
    create_mixer_renderer, create_mixer_renderer_with_fallback, set_preferred_mixer_backend,
    MixerRenderOutput, MixerRenderer, MixerRendererBackend,
};
pub use snapshot::{
    MixerDynamicBatch, MixerMasterSnapshot, MixerRenderSnapshot, MixerRenderViewport,
    MixerStaticBatch, MixerStripGeom, MixerStripSnapshot, MixerTreeSnapshot,
};
