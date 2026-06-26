//! Draw-only mixer render snapshot.
//!
//! Built on the UI thread from already-cloned UI state. This type is the *only*
//! input a [`super::renderer::MixerRenderer`] is allowed to read — it must never
//! touch the audio engine, project state, routing, or perform layout. It mirrors
//! the timeline's [`crate::components::timeline::render::snapshot`] contract.
//!
//! Geometry is split into **static** fields (strip set / order / size / colors /
//! selection — change rarely) and **dynamic** fields (`meter_l`/`meter_r`,
//! `hovered` — change every frame). [`MixerRenderSnapshot::static_key`] hashes
//! only the static fields so a backend can keep its static primitive batch cached
//! and rebuild it solely when the key changes.

use std::hash::{Hash, Hasher};

use gpui::Rgba;

/// Scroll / size bounds for the mixer body (the row beneath the sub-header).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MixerRenderViewport {
    /// Width of the scrollable channel area (excludes the pinned master).
    pub channel_area_width: f32,
    /// Full body height (strip height).
    pub height: f32,
    /// Horizontal scroll offset applied to channel strips (not the master).
    pub scroll_x: f32,
    /// Panel-local left x of the pinned master region, if the master is shown.
    pub master_x: Option<f32>,
}

/// A single strip's draw geometry. `x` is in scroll-content space (strip index ×
/// strip width) for channel strips, or panel-local space for the master.
#[derive(Clone, Copy, Debug)]
pub struct MixerStripGeom {
    pub x: f32,
    pub width: f32,
    pub height: f32,
    /// Strip background (already resolved for selected / alternating row).
    pub bg: Rgba,
    /// Top accent bar colour (track colour, or master accent).
    pub accent: Rgba,
    /// Right separator line colour (already resolved stronger when selected, so
    /// selection is reproduced faithfully via `bg` + `separator` — no extra bar).
    pub separator: Rgba,
    pub selected: bool,
    pub is_master: bool,
    // ── Dynamic (excluded from `static_key`) ─────────────────────────────────
    pub meter_l: f32,
    pub meter_r: f32,
    pub hovered: bool,
}

impl MixerStripGeom {
    fn hash_static(&self, hasher: &mut impl Hasher) {
        // Quantise floats so sub-pixel jitter does not thrash the static batch.
        let q = |v: f32| (v * 4.0).round() as i64;
        q(self.x).hash(hasher);
        q(self.width).hash(hasher);
        q(self.height).hash(hasher);
        hash_rgba(self.bg, hasher);
        hash_rgba(self.accent, hasher);
        hash_rgba(self.separator, hasher);
        self.selected.hash(hasher);
        self.is_master.hash(hasher);
        // NOTE: meter_l/meter_r/hovered intentionally excluded — they are the
        // per-frame dynamic batch and must not invalidate the static geometry.
    }
}

fn hash_rgba(c: Rgba, hasher: &mut impl Hasher) {
    c.r.to_bits().hash(hasher);
    c.g.to_bits().hash(hasher);
    c.b.to_bits().hash(hasher);
    c.a.to_bits().hash(hasher);
}

/// Immutable per-frame description of the mixer's dense primitives.
#[derive(Clone, Debug)]
pub struct MixerRenderSnapshot {
    pub viewport: MixerRenderViewport,
    pub strips: Vec<MixerStripGeom>,
    pub master: Option<MixerStripGeom>,
    /// Height of the top accent bar.
    pub accent_bar_h: f32,
    /// Width of the right separator line.
    pub separator_w: f32,
    /// Hash of every static field (strip set/size/colour/selection + viewport
    /// extents). Stable across meter/hover-only changes.
    pub static_key: u64,
}

impl MixerRenderSnapshot {
    pub fn new(
        viewport: MixerRenderViewport,
        strips: Vec<MixerStripGeom>,
        master: Option<MixerStripGeom>,
        accent_bar_h: f32,
        separator_w: f32,
    ) -> Self {
        let static_key =
            Self::compute_static_key(&viewport, &strips, master.as_ref(), accent_bar_h, separator_w);
        Self {
            viewport,
            strips,
            master,
            accent_bar_h,
            separator_w,
            static_key,
        }
    }

    fn compute_static_key(
        viewport: &MixerRenderViewport,
        strips: &[MixerStripGeom],
        master: Option<&MixerStripGeom>,
        accent_bar_h: f32,
        separator_w: f32,
    ) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let q = |v: f32| (v * 4.0).round() as i64;
        q(viewport.channel_area_width).hash(&mut hasher);
        q(viewport.height).hash(&mut hasher);
        // scroll_x is deliberately part of the static key NOT being included:
        // scrolling only shifts the paint transform, it never rebuilds geometry.
        viewport.master_x.map(q).hash(&mut hasher);
        q(accent_bar_h).hash(&mut hasher);
        q(separator_w).hash(&mut hasher);
        strips.len().hash(&mut hasher);
        for strip in strips {
            strip.hash_static(&mut hasher);
        }
        master.is_some().hash(&mut hasher);
        if let Some(master) = master {
            master.hash_static(&mut hasher);
        }
        hasher.finish()
    }

    /// Quantised fingerprint of all dynamic meter values — used to count
    /// meter-buffer updates without redrawing when nothing moved.
    pub fn meter_signature(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0) as u8;
        for strip in &self.strips {
            q(strip.meter_l).hash(&mut hasher);
            q(strip.meter_r).hash(&mut hasher);
        }
        if let Some(master) = &self.master {
            q(master.meter_l).hash(&mut hasher);
            q(master.meter_r).hash(&mut hasher);
        }
        hasher.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgba(r: f32) -> Rgba {
        Rgba { r, g: 0.2, b: 0.3, a: 1.0 }
    }

    fn strip(x: f32, selected: bool, meter: f32) -> MixerStripGeom {
        MixerStripGeom {
            x,
            width: 88.0,
            height: 320.0,
            bg: rgba(0.1),
            accent: rgba(0.5),
            separator: rgba(0.0),
            selected,
            is_master: false,
            meter_l: meter,
            meter_r: meter,
            hovered: false,
        }
    }

    fn viewport(scroll_x: f32) -> MixerRenderViewport {
        MixerRenderViewport {
            channel_area_width: 800.0,
            height: 320.0,
            scroll_x,
            master_x: Some(801.0),
        }
    }

    #[test]
    fn static_key_stable_across_meter_only_changes() {
        let a = MixerRenderSnapshot::new(viewport(0.0), vec![strip(0.0, false, 0.1)], None, 2.0, 1.0);
        let b = MixerRenderSnapshot::new(viewport(0.0), vec![strip(0.0, false, 0.9)], None, 2.0, 1.0);
        assert_eq!(a.static_key, b.static_key, "meter changes must not rebuild static batch");
        assert_ne!(a.meter_signature(), b.meter_signature(), "meter change must move the meter signature");
    }

    #[test]
    fn static_key_stable_across_scroll_only_changes() {
        let a = MixerRenderSnapshot::new(viewport(0.0), vec![strip(0.0, false, 0.1)], None, 2.0, 1.0);
        let b = MixerRenderSnapshot::new(viewport(240.0), vec![strip(0.0, false, 0.1)], None, 2.0, 1.0);
        assert_eq!(a.static_key, b.static_key, "scroll only shifts the transform, not geometry");
    }

    #[test]
    fn static_key_changes_on_selection_strip_set_and_size() {
        let base = MixerRenderSnapshot::new(viewport(0.0), vec![strip(0.0, false, 0.1)], None, 2.0, 1.0);
        let selected =
            MixerRenderSnapshot::new(viewport(0.0), vec![strip(0.0, true, 0.1)], None, 2.0, 1.0);
        assert_ne!(base.static_key, selected.static_key, "selection must rebuild static batch");

        let added = MixerRenderSnapshot::new(
            viewport(0.0),
            vec![strip(0.0, false, 0.1), strip(88.0, false, 0.1)],
            None,
            2.0,
            1.0,
        );
        assert_ne!(base.static_key, added.static_key, "adding a strip must rebuild");

        let mut tall = strip(0.0, false, 0.1);
        tall.height = 480.0;
        let resized = MixerRenderSnapshot::new(viewport(0.0), vec![tall], None, 2.0, 1.0);
        assert_ne!(base.static_key, resized.static_key, "size change must rebuild");
    }
}
