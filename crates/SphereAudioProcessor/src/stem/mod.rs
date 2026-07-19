//! Stem extraction surface for SphereAudioProcessor.
//!
//! Re-exports the MDX-NET offline stem API from `SphereStemExtractor` so UI,
//! export, and future engine tooling share one params/model/device contract.
//!
//! Classification: offline / control path only — never call from the realtime
//! audio callback.

pub use sphere_stem_extractor::{
    create_mdx_net_backend, default_models_dir, download_model, ensure_models_dir, extract_stems,
    gpu_available, model_installed, resolve_device, resolve_installed_model_files,
    InferBackendKind, InferDevice, StemExtractCancelToken, StemExtractError, StemExtractInput,
    StemExtractOutput, StemExtractParams, StemExtractProgress, StemExtractQuality,
    StemExtractResult, StemExtractStage, StemInferBackend, StemKind, StemModel,
    StemModelDownloadProgress, StemModelFile, StemModelInfo, StemModelPackage, StemSet,
    STEM_MODELS, UVR_MODEL_RELEASE_BASE,
};

/// Convenience constructor matching the Stem Extractor dialog defaults:
/// MDX-NET · CPU · all 4 stems · balanced quality · CPU fallback enabled.
pub fn default_stem_extract_params() -> StemExtractParams {
    StemExtractParams::mdx_net_cpu()
}

/// Convenience constructor for MDX-NET on GPU (falls back to CPU when allowed).
pub fn mdx_net_gpu_params() -> StemExtractParams {
    StemExtractParams::mdx_net_gpu()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_use_mdx_net_cpu() {
        let params = default_stem_extract_params();
        assert_eq!(params.model, StemModel::MdxNet);
        assert_eq!(params.device, InferDevice::Cpu);
        assert_eq!(params.stems.len(), 4);
        params.validate().unwrap();
    }

    #[test]
    fn extract_via_audio_processor_surface() {
        let input = StemExtractInput::new(44_100, 1, vec![0.25; 1024]);
        let result = extract_stems(
            &input,
            &default_stem_extract_params(),
            &StemExtractCancelToken::new(),
            |_| {},
        )
        .unwrap();
        assert_eq!(result.stems.len(), 4);
        assert_eq!(result.model, StemModel::MdxNet);
    }
}
