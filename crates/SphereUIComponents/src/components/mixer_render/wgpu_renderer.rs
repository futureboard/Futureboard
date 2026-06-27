//! Parked offscreen-WGPU mixer backend (feature `gpu-renderer`).
//!
//! Symmetry stub mirroring the timeline's offscreen path. It is **never selected**
//! while `MIXER_WGPU_COMPOSITE_READY = false` in [`super::renderer`], because GPUI
//! on Windows cannot composite an external wgpu texture into the window. It exists
//! so the backend enum, feature gating, and fallback wiring have a real seam for a
//! future texture-interop path. It performs no GPU work and reports unavailable.

use super::renderer::{MixerRenderOutput, MixerRenderer};
use super::snapshot::MixerRenderSnapshot;

/// Placeholder offscreen frame handle. Carries nothing until texture interop
/// lands; today the surface never reaches a present path.
#[derive(Debug, Clone)]
pub struct WgpuMixerOffscreenFrame {
    pub width: u32,
    pub height: u32,
}

pub struct WgpuMixerRenderer {
    available: bool,
}

impl WgpuMixerRenderer {
    pub fn new() -> Self {
        // No device is created: the offscreen texture cannot be composited yet,
        // so advertising availability would only add GPU work behind a fallback.
        Self { available: false }
    }

    pub fn is_available(&self) -> bool {
        self.available
    }
}

impl MixerRenderer for WgpuMixerRenderer {
    fn backend_name(&self) -> &'static str {
        "wgpu-offscreen"
    }

    fn render(&mut self, snapshot: &MixerRenderSnapshot) -> MixerRenderOutput {
        MixerRenderOutput::WgpuOffscreen(WgpuMixerOffscreenFrame {
            width: snapshot.viewport.channel_area_width.max(0.0) as u32,
            height: snapshot.viewport.height.max(0.0) as u32,
        })
    }
}
