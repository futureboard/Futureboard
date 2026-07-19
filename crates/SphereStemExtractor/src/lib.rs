//! Offline stem extraction for Futureboard Studio.
//!
//! This crate owns the MDX-NET model catalog, CPU/GPU device selection, stem
//! slot selection, and a buffer-in / stems-out extraction API. It does **not**
//! perform filesystem I/O or touch the realtime audio callback.
//!
//! Classification: scanner/offline path (worker thread only).
//!
//! The current default backend is a deterministic spectral stub used to wire
//! the UI/job pipeline until ONNX MDX-NET weights are installed. Real MDX-NET
//! inference will replace [`backend::SpectralStubBackend`] without changing the
//! public params surface.

pub mod backend;
pub mod device;
pub mod error;
pub mod extractor;
pub mod model;
pub mod params;
pub mod progress;
pub mod stems;

pub use backend::{create_mdx_net_backend, InferBackendKind, StemInferBackend};
pub use device::{gpu_available, resolve_device, InferDevice};
pub use error::StemExtractError;
pub use extractor::{extract_stems, StemExtractInput, StemExtractOutput, StemExtractResult};
pub use model::{StemModel, StemModelInfo, STEM_MODELS};
pub use params::{StemExtractParams, StemExtractQuality};
pub use progress::{StemExtractCancelToken, StemExtractProgress, StemExtractStage};
pub use stems::{StemKind, StemSet};
