//! Central audio-processing API surface for Futureboard Studio.
//!
//! This crate owns shared, serializable stretch parameters and pure ratio /
//! backend-selection math so UI, playback, export, waveform cache, and timeline
//! length can converge on one source of truth.

pub mod ffi;
pub mod stretching;

pub use stretching::{
    StretchAlgorithm, StretchBackend, StretchError, StretchMode, StretchParams, StretchProcessor,
    create_stretch_processor, effective_pitch_ratio, effective_time_ratio, resolve_backend,
    source_read_rate_for_repitch, stretched_duration_samples,
};
