//! Arrangement background surface (grid + bar shades) via renderer abstraction.

use std::cell::RefCell;

use gpui::IntoElement;

use crate::components::timeline::render::{
    gpui_paint, renderer::TimelineRenderOutput, snapshot::TimelineRenderSnapshot,
    snapshot::SnapshotBuildOptions, create_timeline_renderer_with_fallback, TimelineRenderer,
    TimelineRendererBackend,
};
#[cfg(feature = "gpu-renderer")]
use crate::components::timeline::render::GpuiPaintTimelineRenderer;
use crate::components::timeline::timeline_state::TimelineState;

thread_local! {
    static TIMELINE_RENDERER: RefCell<Option<(TimelineRendererBackend, Box<dyn TimelineRenderer>)>> =
        RefCell::new(None);
}

fn render_arrangement_with(
    snapshot: &TimelineRenderSnapshot,
    renderer: &mut dyn TimelineRenderer,
    backend: TimelineRendererBackend,
) -> gpui::AnyElement {
    gpui_paint::log_snapshot_stats(snapshot, backend.label());
    match renderer.render_arrangement(snapshot) {
        TimelineRenderOutput::Gpui(element) => element,
        #[cfg(feature = "gpu-renderer")]
        TimelineRenderOutput::WgpuOffscreen(frame) => {
            let _ = frame;
            match GpuiPaintTimelineRenderer::new().render_arrangement(snapshot) {
                TimelineRenderOutput::Gpui(element) => element,
                TimelineRenderOutput::WgpuOffscreen(_) => unreachable!(),
            }
        }
    }
}

fn with_renderer(snapshot: &TimelineRenderSnapshot) -> gpui::AnyElement {
    if TIMELINE_RENDERER.try_with(|_| ()).is_err() {
        let preferred = TimelineRendererBackend::from_env();
        let (mut renderer, backend) = create_timeline_renderer_with_fallback(preferred);
        return render_arrangement_with(snapshot, renderer.as_mut(), backend);
    }
    TIMELINE_RENDERER.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            let preferred = TimelineRendererBackend::from_env();
            let (renderer, backend) = create_timeline_renderer_with_fallback(preferred);
            *slot = Some((backend, renderer));
        }
        let (backend, renderer) = slot.as_mut().expect("renderer slot");
        render_arrangement_with(snapshot, renderer.as_mut(), *backend)
    })
}

/// Scrollable arrangement grid layer (behind track lanes / clips).
pub fn timeline_surface(
    state: &TimelineState,
    grid_width: f32,
    grid_height: f32,
) -> impl IntoElement {
    let _s = crate::perf::PerfScope::enter("TimelineSurface");

    let mut options = SnapshotBuildOptions::default();
    options.scale_factor = 1.0;

    let mut snapshot = TimelineRenderSnapshot::from_state(state, options);
    snapshot.viewport.width = grid_width.max(1.0);
    snapshot.viewport.height = grid_height.max(1.0);

    with_renderer(&snapshot)
}
