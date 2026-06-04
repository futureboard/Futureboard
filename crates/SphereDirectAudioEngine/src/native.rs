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

use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::audio_file;
use crate::backend::BackendKind;
use crate::device;
use crate::engine::EngineInner;
use crate::error::SphereAudioError;
use crate::types::{EngineProjectSnapshot, JsDauxConfig, JsMeterSnapshot, JsStartRecordingConfig};

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
    pub position_seconds: f64,
    pub position_beats: f64,
    pub position_samples: u64,
    pub loop_enabled: bool,
    pub bpm: f64,
    pub time_signature_num: u32,
    pub time_signature_den: u32,
    pub sample_rate: u32,
    pub buffer_size: u32,
    pub backend_name: String,
    pub output_device: Option<String>,
    pub last_error: Option<String>,
    pub glitch_count: u64,
    pub estimated_latency_ms: f64,
    /// `true` when the device was lost mid-stream and recovery is pending.
    pub device_lost: bool,
    /// Lifecycle state: "Closed" | "Ready" | "Running" | "DeviceLost".
    pub device_state: String,
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
        // Resume an already-open stream — never tear down and reopen here.
        // Reopening runs `get_initial_runtime` on the caller thread and can
        // re-decode every clip (UI freeze on Play after a project sync).
        let st = self.inner.get_status();
        if st.stream_open && st.running {
            return Ok(());
        }
        if st.stream_open {
            return self.inner.start();
        }
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

    /// Ordered shutdown before UI teardown. Idempotent.
    pub fn shutdown(&mut self) {
        self.inner.shutdown();
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

    /// Seek the native transport to an absolute project time in seconds.
    pub fn seek(&self, position_seconds: f64) -> Result<(), SphereAudioError> {
        self.inner.seek(position_seconds.max(0.0))
    }

    pub fn set_metronome_enabled(&self, enabled: bool) -> Result<(), SphereAudioError> {
        self.inner.set_metronome_enabled(enabled)
    }

    pub fn set_bpm(&self, bpm: f64) -> Result<(), SphereAudioError> {
        self.inner.set_bpm(bpm)
    }

    pub fn set_time_signature(
        &self,
        numerator: u32,
        denominator: u32,
    ) -> Result<(), SphereAudioError> {
        self.inner.set_time_signature(numerator, denominator)
    }

    pub fn set_loop(
        &self,
        enabled: bool,
        start_seconds: f64,
        end_seconds: f64,
    ) -> Result<(), SphereAudioError> {
        self.inner.set_loop(enabled, start_seconds, end_seconds)
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
        let transport = self.inner.transport_snapshot();
        EngineStats {
            running: st.running,
            stream_open: st.stream_open,
            transport_playing: self.inner.shared_playing(),
            position_seconds: transport.position_seconds,
            position_beats: transport.position_beats,
            position_samples: transport.position_samples,
            loop_enabled: transport.loop_enabled,
            bpm: transport.bpm,
            time_signature_num: transport.time_signature[0],
            time_signature_den: transport.time_signature[1],
            sample_rate: st.sample_rate,
            buffer_size: st.buffer_size,
            backend_name: daux.backend_name,
            output_device: daux.output_device,
            last_error: st.last_error,
            glitch_count: daux.glitch_count as u64,
            estimated_latency_ms: daux.estimated_latency_ms,
            device_lost: daux.device_lost,
            device_state: daux.device_state,
        }
    }

    /// Attempt to recover the audio device after a device-loss event, reusing
    /// the last-known-good config. Returns `Ok(true)` if recovery ran,
    /// `Ok(false)` if the device was not lost.
    pub fn recover_device(&self) -> Result<bool, SphereAudioError> {
        self.inner.recover_daux()
    }

    /// Begin an input-level test on `device_id` (or the default input device).
    /// Poll [`AudioEngine::input_test_level`] for the meter and stop with
    /// [`AudioEngine::stop_input_test`].
    pub fn start_input_test(&self, device_id: Option<&str>) -> Result<(), SphereAudioError> {
        self.inner.start_input_test(device_id.map(str::to_string))
    }

    /// Peak input level since the last poll, `0.0..=1.0` (0 when inactive).
    pub fn input_test_level(&self) -> f32 {
        self.inner.get_input_test_level()
    }

    /// Stop and release the input-level test stream.
    pub fn stop_input_test(&self) {
        self.inner.stop_input_test();
    }

    /// Whether switching to `config` would require a controlled device restart
    /// versus the currently-open device.
    pub fn requires_restart(&self, config: &EngineConfig) -> bool {
        let daux = JsDauxConfig {
            backend_id: config.backend.backend_id().to_string(),
            output_device_id: None,
            sample_rate: (config.sample_rate > 0).then_some(config.sample_rate),
            buffer_size: (config.buffer_size > 0).then_some(config.buffer_size),
            mmcss_priority: false,
            safe_mode: false,
        };
        self.inner.daux_requires_restart(&daux)
    }

    /// Build/update the realtime runtime graph from a control-thread project snapshot.
    pub fn load_project(&self, snapshot: EngineProjectSnapshot) -> Result<(), SphereAudioError> {
        self.inner.load_project(snapshot)
    }

    pub fn start_recording(&self, config: JsStartRecordingConfig) -> Result<(), SphereAudioError> {
        self.inner.start_recording(config)
    }

    pub fn stop_recording(&self) -> Result<Vec<crate::types::JsRecordingResult>, SphereAudioError> {
        self.inner.stop_recording()
    }

    pub fn recording_status(&self) -> crate::types::JsRecordingStatus {
        self.inner.get_recording_status()
    }

    /// Update a track or master parameter without rebuilding the full runtime graph.
    pub fn update_track_param(
        &self,
        track_id: &str,
        param_id: &str,
        value: f64,
    ) -> Result<(), SphereAudioError> {
        self.inner.update_track_param(track_id, param_id, value)
    }

    /// Clone the live runtime VST3 processor handle for an insert, if it has a
    /// ready native plugin instance. The GPUI PluginView uses this to open the
    /// editor from the *existing* instance/controller — never a new one — so
    /// GUI parameter edits affect the actual audio processor. The handle is
    /// `Arc`-backed; holding it keeps the C++ instance alive while the editor
    /// is open.
    pub fn insert_processor(
        &self,
        track_id: &str,
        insert_id: &str,
    ) -> Option<crate::vst3_processor::Vst3RuntimeProcessor> {
        self.inner.insert_processor(track_id, insert_id)
    }

    /// Poll meter atomics and runtime track meters for UI display.
    pub fn meters(&self) -> JsMeterSnapshot {
        self.inner.get_meters()
    }

    /// Aggregate latency report: device buffer latency plus per-track and master
    /// plug-in latency. Reporting only (Phase V); full plug-in delay
    /// compensation is Phase W.
    pub fn latency_info(&self) -> crate::types::JsLatencyInfo {
        self.inner.get_latency_info()
    }

    pub fn pdc_enabled(&self) -> bool {
        self.inner.pdc_enabled()
    }

    pub fn set_pdc_enabled(&self, enabled: bool) {
        self.inner.set_pdc_enabled(enabled);
    }

    /// Multi-LOD peak summary for any audio format supported by the
    /// engine's decoder. The Native UI's waveform pipeline calls this
    /// instead of running its own decoder, so the LOD ladder and decode
    /// quality stay in sync with the realtime engine's view of the file.
    ///
    /// Runs the full decode on the caller's thread — invoke from a
    /// background executor, never from render / layout / audio callback.
    pub fn generate_peaks(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<audio_file::AudioPeakFile, SphereAudioError> {
        audio_file::generate_audio_peaks(path)
    }

    /// Lightweight engine snapshot for diagnostic logs (Rust-side, no NAPI types).
    /// Use this to verify that clips made it into the realtime runtime — silent
    /// playback almost always means `loaded_clips == 0` or `ready_clips == 0`.
    pub fn debug_snapshot(&self) -> EngineDebugSnapshot {
        let info = self.inner.get_debug_info();
        EngineDebugSnapshot {
            loaded_tracks: info.loaded_tracks,
            loaded_clips: info.loaded_clips,
            ready_clips: info.ready_clips,
            is_playing: info.is_playing,
            position_seconds: info.position_seconds,
        }
    }

    /// Structured per-insert instantiation status (Phase 2b). Cheap — locks
    /// the runtime briefly and clones a small descriptor list. Used by the
    /// native shell to flip `InsertLoadStatus::Failed` on plugin load failure.
    pub fn insert_statuses(&self) -> Vec<EngineInsertStatus> {
        self.inner.insert_statuses()
    }
}

/// Per-insert instantiation status for UI readback (Phase 2b). Plain Rust,
/// no NAPI — consumed by the native shell's audio poll to surface
/// `InsertLoadStatus::Failed` when a native plugin fails to instantiate.
#[derive(Debug, Clone)]
pub struct EngineInsertStatus {
    pub track_id: String,
    pub insert_id: String,
    /// `true` when this is a native-plugin insert (vs. a built-in DSP).
    pub native: bool,
    /// `true` when the insert's processor is live. A native insert with
    /// `ready == false` is a definitive instantiation failure.
    pub ready: bool,
}

/// Plain-Rust mirror of the realtime engine's debug snapshot, suitable for
/// status-bar logs in the native shell.
#[derive(Debug, Clone, Default)]
pub struct EngineDebugSnapshot {
    pub loaded_tracks: u32,
    pub loaded_clips: u32,
    pub ready_clips: u32,
    pub is_playing: bool,
    pub position_seconds: f64,
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
