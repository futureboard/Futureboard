//! Central audio-processing API surface for Futureboard Studio.
//!
//! This crate owns shared, serializable stretch parameters and pure ratio /
//! backend-selection math so UI, playback, export, waveform cache, and timeline
//! length can converge on one source of truth.
//!
//! It also re-exports the offline MDX-NET stem-extraction params/model/device
//! surface from `SphereStemExtractor` for the Stem Extractor dialog and jobs.

pub mod ffi;
pub mod stem;
pub mod stretching;

pub use stem::{
    InferBackendKind, InferDevice, STEM_MODELS, StemExtractCancelToken, StemExtractError,
    StemExtractInput, StemExtractOutput, StemExtractParams, StemExtractProgress,
    StemExtractQuality, StemExtractResult, StemExtractStage, StemInferBackend, StemKind, StemModel,
    StemModelDownloadProgress, StemModelFile, StemModelInfo, StemModelPackage, StemSet,
    UVR_MODEL_RELEASE_BASE, create_mdx_net_backend, default_models_dir,
    default_stem_extract_params, download_model, ensure_models_dir, extract_stems, gpu_available,
    mdx_net_gpu_params, model_installed, resolve_device, resolve_installed_model_files,
};
pub use stretching::{
    StretchAlgorithm, StretchBackend, StretchError, StretchMode, StretchParams, StretchProcessor,
    create_stretch_processor, effective_pitch_ratio, effective_time_ratio,
    pitch_ratio_to_semitone_cents, resolve_backend, semitone_to_pitch_ratio,
    signalsmith_stretch_available, source_read_rate_for_repitch, stretched_duration_samples,
};
