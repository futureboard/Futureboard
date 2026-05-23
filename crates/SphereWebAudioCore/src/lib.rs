//! Futureboard Studio — WebAudioCore DSP Framework
//!
//! A Rust-based audio engine core for the Futureboard Studio DAW.
//! Compiles to both native and WebAssembly targets.
//!
//! # Architecture
//!
//! - **engine** — Top-level DspEngine with command/event API
//! - **transport** — Play/pause/stop, beat↔sample conversion, loop
//! - **graph** — Flat audio processing graph (tracks → master)
//! - **mixer** — Per-track gain/pan/mute/solo with insert chain
//! - **buffer** — Non-interleaved f32 audio buffers
//! - **devices** — Audio insert effect trait and built-in devices
//! - **commands** — Serializable engine commands
//! - **events** — Engine-to-UI event queue
//! - **params** — Typed parameter system
//! - **meters** — Peak metering with smoothed decay
//! - **ids** — Type-safe entity ID wrappers
//! - **error** — Typed engine errors
//! - **wasm_api** — wasm-bindgen exports for AudioWorklet

pub mod buffer;
pub mod commands;
pub mod devices;
pub mod dsp;
pub mod engine;
pub mod error;
pub mod events;
pub mod graph;
pub mod ids;
pub mod meters;
pub mod mixer;
pub mod params;
pub mod transport;
pub mod wasm_api;

// Re-export primary types for convenience
pub use engine::{DspEngine, EngineConfig, EngineStatus};
pub use commands::{EngineCommand, CommandResult};
pub use events::EngineEvent;
pub use error::{EngineError, EngineResult};
pub use ids::{TrackId, ClipId, DeviceId, AssetId, SendId};
pub use params::ParamValue;
pub use transport::{Transport, PlayState};
pub use buffer::AudioBuffer;
