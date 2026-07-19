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
    create_mdx_net_backend, default_stem_extract_params, extract_stems, gpu_available,
    mdx_net_gpu_params, resolve_device, InferBackendKind, InferDevice, StemExtractCancelToken,
    StemExtractError, StemExtractInput, StemExtractOutput, StemExtractParams, StemExtractProgress,
    StemExtractQuality, StemExtractResult, StemExtractStage, StemInferBackend, StemKind, StemModel,
    StemModelInfo, StemSet, STEM_MODELS,
};
pub use stretching::{
    StretchAlgorithm, StretchBackend, StretchError, StretchMode, StretchParams, StretchProcessor,
    create_stretch_processor, effective_pitch_ratio, effective_time_ratio,
    pitch_ratio_to_semitone_cents, resolve_backend, semitone_to_pitch_ratio,
    signalsmith_stretch_available, source_read_rate_for_repitch, stretched_duration_samples,
};
