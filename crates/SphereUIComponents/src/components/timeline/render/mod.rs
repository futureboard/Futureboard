//! Hybrid timeline rendering: immutable snapshots + pluggable backends.
//!
//! - [`TimelineViewport`] — scroll/zoom bounds for the arrangement region
//! - [`TimelineRenderSnapshot`] — read-only frame description (built on UI thread)
//! - [`TimelineRenderer`] — GPUI paint or offscreen WGPU draw
//!
//! Normal UI (menus, dialogs, headers, lanes as interactive GPUI elements) stays
//! in GPUI; dense paint (grid, future clip/waveform batches) routes here.

pub mod gpui_paint;
pub mod renderer;
pub mod snapshot;
pub mod viewport;
#[cfg(feature = "gpu-renderer")]
pub mod wgpu_renderer;

pub use gpui_paint::GpuiPaintTimelineRenderer;
pub use renderer::{
    create_timeline_renderer, create_timeline_renderer_with_fallback, set_preferred_backend,
    TimelineRenderOutput, TimelineRenderer, TimelineRendererBackend,
};
pub use snapshot::{
    BarShadeSnapshot, GridLineSnapshot, PlayheadSnapshot, RenderClipKind, RenderClipSnapshot,
    RenderLaneSnapshot, SelectionSnapshot, SnapshotBuildOptions, TimelineRenderSnapshot,
    VisibleBeatRange, VisibleTrackRange, WaveformChunkHandle, WaveformReadyKind,
};
pub use viewport::TimelineViewport;
#[cfg(feature = "gpu-renderer")]
pub use wgpu_renderer::{
    list_available_gpu_devices, set_preferred_gpu_device_id, GpuDeviceInfo, TimelineGpuPreference,
    WgpuOffscreenFrame, WgpuTimelineRenderer,
};

#[cfg(not(feature = "gpu-renderer"))]
pub fn set_preferred_gpu_device_id(_id: &str) {}

/// Stub used when the `gpu-renderer` cargo feature isn't enabled so the
/// settings UI can still compile and show a flat "GPU unavailable" state
/// without conditional code on every call site.
#[cfg(not(feature = "gpu-renderer"))]
#[derive(Debug, Clone)]
pub struct GpuDeviceInfo {
    pub id: String,
    pub name: String,
    pub backend: Option<String>,
    pub device_type: Option<String>,
    pub vendor_id: Option<u32>,
    pub device_id: Option<u32>,
}

#[cfg(not(feature = "gpu-renderer"))]
pub fn list_available_gpu_devices() -> Vec<GpuDeviceInfo> {
    Vec::new()
}
