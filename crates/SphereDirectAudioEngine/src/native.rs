//! Native Rust facade over [`crate::engine::EngineInner`].
//!
//! This module exists so the Rust-Native Futureboard Studio shell
//! (`apps/experimental/native`) can drive the audio engine without
//! touching any NAPI types or Node.js runtime. It is a thin wrapper —
//! the underlying state, audio thread, and DSP code stay the same.
//!
//! The existing NAPI surface in [`crate::SphereDirectAudioEngine`] is
//! untouched. Both entry points share the same `EngineInner` so a
//! command issued through either surface sees the same realtime state.
//!
//! Realtime safety contract is identical to the NAPI path:
//!   * `start` / `stop` / `open` calls run on the control thread and
//!     take a parking-lot lock — not realtime safe, but they only run
//!     from UI events.
//!   * The native facade adds no allocations on the audio thread.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::backend::BackendKind;
use crate::device;
use crate::engine::EngineInner;
use crate::error::SphereAudioError;
use crate::types::JsDauxConfig;

/// Default sample rate used when the caller does not specify one. Mirrors
/// the system's "Auto" path — most backends ignore this and pick their
/// own default anyway.
pub const DEFAULT_SAMPLE_RATE: u32 = 48_000;
/// Default buffer size used when the caller does not specify one.
pub const DEFAULT_BUFFER_SIZE: u32 = 256;

/// Which DAUx audio backend to drive. Mirrors [`BackendKind`] but is
/// re-exposed under a more Rust-Native-friendly name and limited to the
/// values the native shell currently cares about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AudioBackend {
    /// Platform default — WASAPI Shared / CoreAudio / ALSA.
    #[default]
    Auto,
    /// cpal-backed best-effort (same as `Auto` for now; explicit selector).
    Cpal,
    /// Windows: WASAPI Exclusive event-driven (lowest latency).
    WasapiExclusive,
}

impl AudioBackend {
    fn to_backend_kind(self) -> BackendKind {
        match self {
            AudioBackend::Auto => BackendKind::Auto,
            AudioBackend::Cpal => BackendKind::Auto,
            AudioBackend::WasapiExclusive => BackendKind::WasapiExclusive,
        }
    }

    fn backend_id(self) -> &'static str {
        self.to_backend_kind().id()
    }
}

/// Configuration for opening the engine's audio stream.
///
/// `sample_rate == 0` or `buffer_size == 0` means "use the device default".
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub sample_rate: u32,
    pub buffer_size: u32,
    pub channels: u16,
    pub backend: AudioBackend,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            sample_rate: DEFAULT_SAMPLE_RATE,
            buffer_size: DEFAULT_BUFFER_SIZE,
            channels: 2,
            backend: AudioBackend::Auto,
        }
    }
}

/// Plain-Rust audio device descriptor returned by
/// [`AudioEngine::list_output_devices`] / [`AudioEngine::list_input_devices`].
///
/// Mirrors the NAPI `JsAudioDeviceInfo` shape but lives entirely in Rust so
/// the native shell does not pull NAPI types into its public surface.
#[derive(Debug, Clone)]
pub struct EngineDeviceInfo {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub channels: u32,
    pub default_sample_rate: u32,
    pub is_default: bool,
    pub backend: String,
}

impl From<crate::types::JsAudioDeviceInfo> for EngineDeviceInfo {
    fn from(d: crate::types::JsAudioDeviceInfo) -> Self {
        Self {
            id: d.id,
            name: d.name,
            kind: d.kind,
            channels: d.channels,
            default_sample_rate: d.default_sample_rate,
            is_default: d.is_default,
            backend: d.backend,
        }
    }
}

/// Lightweight status snapshot suitable for status-bar polling.
#[derive(Debug, Clone, Default)]
pub struct EngineStats {
    pub running: bool,
    pub stream_open: bool,
    pub transport_playing: bool,
    pub sample_rate: u32,
    pub buffer_size: u32,
    pub backend_name: String,
    pub output_device: Option<String>,
    pub last_error: Option<String>,
    pub glitch_count: u64,
    pub estimated_latency_ms: f64,
}

/// Native Rust-facing handle to the engine.
///
/// Wraps [`EngineInner`] in an `Arc`. Cloning the handle is cheap and
/// shares the same underlying engine — the audio thread and any other
/// control surfaces all see the same state.
#[derive(Clone)]
pub struct AudioEngine {
    inner: Arc<EngineInner>,
    config: EngineConfig,
}

impl AudioEngine {
    /// The default native configuration. Equivalent to
    /// `EngineConfig::default()`; provided as a method so call sites read
    /// closer to the spec (`AudioEngine::default_config()`).
    pub fn default_config() -> EngineConfig {
        EngineConfig::default()
    }

    /// Build a new engine handle. Does **not** open or start the audio
    /// stream — call [`AudioEngine::start`] when ready.
    pub fn new(config: EngineConfig) -> Result<Self, SphereAudioError> {
        Ok(Self {
            inner: Arc::new(EngineInner::new()),
            config,
        })
    }

    /// Borrow the configuration the engine was created with. The active
    /// runtime sample rate / buffer size may differ once a stream is
    /// open — see [`AudioEngine::stats`].
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Engine semver string, e.g. `"0.1.0"`.
    pub fn version(&self) -> String {
        self.inner.get_version()
    }

    /// Open the audio stream and start it. The stream stays paused at the
    /// transport level — use [`AudioEngine::play`] to advance the
    /// timeline once playback work is wired up.
    pub fn start(&mut self) -> Result<(), SphereAudioError> {
        let daux = JsDauxConfig {
            backend_id: self.config.backend.backend_id().to_string(),
            output_device_id: None,
            sample_rate: if self.config.sample_rate > 0 {
                Some(self.config.sample_rate)
            } else {
                None
            },
            buffer_size: if self.config.buffer_size > 0 {
                Some(self.config.buffer_size)
            } else {
                None
            },
            mmcss_priority: false,
            safe_mode: false,
        };
        self.inner.open_daux(daux)?;
        self.inner.start()
    }

    /// Stop the audio stream (closes the device, frees realtime resources).
    pub fn stop(&mut self) -> Result<(), SphereAudioError> {
        self.inner.stop();
        self.inner.close_device();
        Ok(())
    }

    /// Whether the stream is currently active.
    pub fn is_running(&self) -> bool {
        self.inner.get_status().running
    }

    /// Begin advancing the transport cursor. The audio stream must already
    /// be open via [`AudioEngine::start`].
    pub fn play(&self) -> Result<(), SphereAudioError> {
        self.inner.play()
    }

    /// Pause the transport cursor. The audio stream remains active.
    pub fn pause(&self) -> Result<(), SphereAudioError> {
        self.inner.pause()
    }

    /// Toggle the transport between play and pause. Returns the new playing
    /// state. No-ops cleanly if the stream is not open yet.
    pub fn toggle_transport(&self) -> Result<bool, SphereAudioError> {
        if self.inner.shared_playing() {
            self.inner.pause()?;
            Ok(false)
        } else {
            // `play` requires an open stream — surface the same error the
            // engine would have produced.
            self.inner.play()?;
            Ok(true)
        }
    }

    /// Enumerate output devices on the default host. Returns an empty list
    /// on any backend error rather than panicking.
    pub fn list_output_devices(&self) -> Vec<EngineDeviceInfo> {
        device::list_output_devices()
            .into_iter()
            .map(EngineDeviceInfo::from)
            .collect()
    }

    /// Enumerate input devices on the default host.
    pub fn list_input_devices(&self) -> Vec<EngineDeviceInfo> {
        device::list_input_devices()
            .into_iter()
            .map(EngineDeviceInfo::from)
            .collect()
    }

    /// Return the default output device descriptor, if the platform has one.
    pub fn default_output_device(&self) -> Option<EngineDeviceInfo> {
        self.list_output_devices()
            .into_iter()
            .find(|d| d.is_default)
    }

    /// Polling snapshot for status bar / diagnostics.
    pub fn stats(&self) -> EngineStats {
        let st = self.inner.get_status();
        let daux = self.inner.get_daux_status();
        EngineStats {
            running: st.running,
            stream_open: st.stream_open,
            transport_playing: self.inner.shared_playing(),
            sample_rate: st.sample_rate,
            buffer_size: st.buffer_size,
            backend_name: daux.backend_name,
            output_device: daux.output_device,
            last_error: st.last_error,
            glitch_count: daux.glitch_count as u64,
            estimated_latency_ms: daux.estimated_latency_ms,
        }
    }
}

// ── EngineInner helper accessor ──────────────────────────────────────────
//
// We need read access to `SharedState::playing` for the stats snapshot
// without exposing the entire shared state. A small accessor on
// `EngineInner` keeps the rest of the engine untouched.

impl EngineInner {
    /// Whether the transport is advancing. Used by [`AudioEngine::stats`].
    pub fn shared_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Relaxed)
    }
}
