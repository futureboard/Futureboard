//! Thread-local mixer primitive renderer + the process-wide GPU-mode switch.
//!
//! Mirrors [`crate::components::timeline::timeline_surface`]. The mixer panel
//! builds a [`MixerRenderSnapshot`] and calls [`render_mixer_primitives`] to get
//! the batched `canvas` element painted behind the channel strips. Whether the
//! panel uses that path at all is gated by [`mixer_gpu_primitives_active`], set
//! from the saved UI-render Setting at startup (and overridable for testing via
//! `FUTUREBOARD_MIXER_GPU=1`).

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use gpui::{div, AnyElement, IntoElement, Styled};

use super::mixer_render::{
    create_mixer_renderer_with_fallback, MixerRenderOutput, MixerRenderSnapshot, MixerRenderer,
    MixerRendererBackend,
};

thread_local! {
    static MIXER_RENDERER: RefCell<Option<(MixerRendererBackend, Box<dyn MixerRenderer>)>> =
        const { RefCell::new(None) };
}

/// Whether the mixer paints its dense decoration via the GPU primitive canvas.
/// Default off — the legacy `div` path stays the fallback until the GPU path is
/// visually verified. Set from the `RenderMode` Setting at startup.
static GPU_PRIMITIVES: AtomicBool = AtomicBool::new(false);

/// Called once at startup from the saved Setting mapping.
pub fn set_mixer_gpu_primitives_enabled(on: bool) {
    GPU_PRIMITIVES.store(on, Ordering::Relaxed);
}

/// `FUTUREBOARD_MIXER_GPU=1`/`0` forces the GPU primitive path on/off, winning
/// over the Setting (developer / QA override).
fn env_override() -> Option<bool> {
    static FLAG: OnceLock<Option<bool>> = OnceLock::new();
    *FLAG.get_or_init(
        || match std::env::var("FUTUREBOARD_MIXER_GPU").ok().as_deref() {
            Some("1") | Some("true") | Some("on") => Some(true),
            Some("0") | Some("false") | Some("off") => Some(false),
            _ => None,
        },
    )
}

/// Whether the mixer should render its dense decoration on the GPU primitive
/// layer this frame. Cheap (one atomic load + cached env check).
pub fn mixer_gpu_primitives_active() -> bool {
    if let Some(forced) = env_override() {
        return forced;
    }
    GPU_PRIMITIVES.load(Ordering::Relaxed)
}

fn render_with(snapshot: &MixerRenderSnapshot, renderer: &mut dyn MixerRenderer) -> AnyElement {
    match renderer.render(snapshot) {
        MixerRenderOutput::Gpui(element) => element,
        #[cfg(feature = "gpu-renderer")]
        MixerRenderOutput::WgpuOffscreen(frame) => {
            // Retained for the future texture-composite path; today WGPU is never
            // selected (see `MIXER_WGPU_COMPOSITE_READY`), so this paints nothing.
            let _ = frame;
            div().absolute().inset_0().into_any_element()
        }
    }
}

/// Render the mixer primitive layer for `snapshot` using the thread-local
/// renderer (constructed with automatic fallback on first use).
pub fn render_mixer_primitives(snapshot: &MixerRenderSnapshot) -> AnyElement {
    let preferred = MixerRendererBackend::from_env();
    MIXER_RENDERER.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            let (renderer, backend) = create_mixer_renderer_with_fallback(preferred);
            *slot = Some((backend, renderer));
        }
        let (_backend, renderer) = slot.as_mut().expect("renderer slot");
        render_with(snapshot, renderer.as_mut())
    })
}

/// Active backend label for diagnostics / HUD.
pub fn active_mixer_renderer_backend() -> &'static str {
    MIXER_RENDERER
        .try_with(|cell| cell.borrow().as_ref().map(|(backend, _)| backend.label()))
        .ok()
        .flatten()
        .unwrap_or("uninit")
}
