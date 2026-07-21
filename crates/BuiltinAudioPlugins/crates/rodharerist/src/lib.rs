//! Rodhareist — flagship guitar multi-effect (DSP core).
//!
//! Engine-agnostic like the other `BuiltinAudioPlugins` cores: it exposes a
//! realtime-safe [`StereoEffect`](builtin_dsp_core::StereoEffect) chain
//! (`Gate → Drive → Amp → Chorus → Delay → Reverb → Cabinet`) plus the metadata
//! and parameter model the React editor drives. Host/bridge wiring (C entry
//! points, embedded-UI table) is layered on separately.

mod dsp;

pub use dsp::{
    AmpModel, CabModel, Dsp, DriveModel, NamCaptureInfo, NamLoadError, PLUGIN_ID, Params,
    StageKind, ToneEngineKind, default_params, descriptor,
};

#[cfg(test)]
mod tests {
    use super::*;
    use builtin_dsp_core::StereoEffect;

    #[test]
    fn descriptor_is_effect_and_ids_are_unique() {
        let d = descriptor();
        assert_eq!(d.id, PLUGIN_ID);
        assert_eq!(d.category, builtin_dsp_core::PluginCategory::Effect);
        let mut ids: Vec<_> = d.params.iter().map(|p| p.id).collect();
        ids.sort_unstable();
        let unique = ids.len();
        ids.dedup();
        assert_eq!(unique, ids.len(), "duplicate parameter id in descriptor");
    }

    #[test]
    fn processes_finite_at_multiple_rates() {
        for &sr in &[44_100.0f32, 48_000.0, 96_000.0] {
            let mut dsp = Dsp::new(sr);
            for n in 0..1_000 {
                let x = (n as f32 * 0.02).sin() * 0.4;
                let (l, r) = dsp.process_stereo(x, x);
                assert!(l.is_finite() && r.is_finite());
            }
        }
    }

    #[test]
    fn reset_clears_tails() {
        let mut dsp = Dsp::new(48_000.0);
        for _ in 0..1_000 {
            let _ = dsp.process_stereo(0.5, -0.5);
        }
        dsp.reset();
        let (l, r) = dsp.process_stereo(0.0, 0.0);
        assert!(l.abs() < 1.0e-3 && r.abs() < 1.0e-3);
    }
}
