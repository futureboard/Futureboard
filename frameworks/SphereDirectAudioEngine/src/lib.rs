//! SphereDirectAudioEngine — N-API entry point.
//!
//! Exposes a single JS class `SphereDirectAudioEngine` that wraps the Rust
//! engine core behind a thread-safe `Arc<EngineInner>`.
//!
//! All public methods on the class are callable from the Electron main process
//! (or any Node.js environment) through the native `.node` addon.
//!
//! Thread safety contract:
//!   - The class instance may be accessed from the JS thread only; napi-rs
//!     enforces this.
//!   - The underlying `EngineInner` is `Send + Sync` — its hot-path state is
//!     accessed by the cpal audio thread via atomics and a lock-free channel.
//!   - Calls that touch the stream (open/close/start/stop) hold a
//!     `parking_lot::Mutex` for the duration — not realtime-safe, but they
//!     run only on the JS thread.

#![deny(clippy::all)]
#![allow(clippy::needless_pass_by_value)] // napi-rs requires owned String args

mod audio_file;
mod command;
pub mod device;
mod dsp;
pub mod engine;
pub mod error;
mod graph;
mod runtime;
pub mod types;

use std::sync::Arc;

use napi_derive::napi;

use engine::EngineInner;
use types::{
    EngineProjectSnapshot, JsAudioDeviceInfo, JsDeviceOpenConfig, JsEngineDebugInfo,
    JsMeterSnapshot, JsSphereAudioStatus,
};

// ── N-API class ───────────────────────────────────────────────────────────────

/// The main audio engine class exposed to Node.js.
///
/// Lifecycle:
/// ```js
/// const engine = new SphereDirectAudioEngine();
/// await engine.openDevice({ sampleRate: 44100, bufferSize: 256 });
/// engine.start();          // start audio stream (silent until play or test tone)
/// engine.setTestTone(true, 440);
/// engine.stop();           // pause stream
/// engine.closeDevice();
/// ```
#[napi]
pub struct SphereDirectAudioEngine {
    inner: Arc<EngineInner>,
}

#[napi]
impl SphereDirectAudioEngine {
    /// Create a new engine instance.  The audio stream is **not** started
    /// automatically — call `openDevice()` then `start()`.
    #[napi(constructor)]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(EngineInner::new()),
        }
    }

    // ── Version / Status ─────────────────────────────────────────────────────

    /// Return the engine version string (e.g. `"0.1.0"`).
    #[napi]
    pub fn get_version(&self) -> String {
        self.inner.get_version()
    }

    /// Return a status snapshot — device names, sample rate, running state, etc.
    #[napi]
    pub fn get_status(&self) -> JsSphereAudioStatus {
        self.inner.get_status()
    }

    // ── Device enumeration ───────────────────────────────────────────────────

    /// List all available audio input devices on the system.
    #[napi]
    pub fn list_input_devices(&self) -> Vec<JsAudioDeviceInfo> {
        self.inner.list_input_devices()
    }

    /// List all available audio output devices on the system.
    #[napi]
    pub fn list_output_devices(&self) -> Vec<JsAudioDeviceInfo> {
        self.inner.list_output_devices()
    }

    // ── Stream lifecycle ─────────────────────────────────────────────────────

    /// Open (or re-open) the audio output stream with the given configuration.
    ///
    /// Closes any previously open stream first.
    /// Call `start()` afterwards to begin audio output.
    ///
    /// Throws if the device is not found or the stream cannot be created.
    #[napi]
    pub fn open_device(&self, config: JsDeviceOpenConfig) -> napi::Result<()> {
        self.inner.open_device(config).map_err(Into::into)
    }

    /// Stop and close the audio stream, freeing the device.
    #[napi]
    pub fn close_device(&self) {
        self.inner.close_device();
    }

    /// Start the audio stream (calls cpal `play()`).
    ///
    /// The transport cursor starts **paused** — call `play()` to begin
    /// advancing the timeline.  The test tone will play immediately if enabled.
    ///
    /// Throws if no stream is open.
    #[napi]
    pub fn start(&self) -> napi::Result<()> {
        self.inner.start().map_err(Into::into)
    }

    /// Pause (silence) the audio stream without closing the device.
    #[napi]
    pub fn stop(&self) {
        self.inner.stop();
    }

    // ── Transport ────────────────────────────────────────────────────────────

    /// Advance the transport cursor (begin timeline playback).
    ///
    /// Throws if no stream is open.
    #[napi]
    pub fn play(&self) -> napi::Result<()> {
        self.inner.play().map_err(Into::into)
    }

    /// Pause the transport cursor (audio stream stays active for monitoring).
    ///
    /// Throws if no stream is open.
    #[napi]
    pub fn pause(&self) -> napi::Result<()> {
        self.inner.pause().map_err(Into::into)
    }

    /// Seek the transport cursor to `seconds` from the project start.
    ///
    /// Throws if no stream is open.
    #[napi]
    pub fn seek(&self, seconds: f64) -> napi::Result<()> {
        self.inner.seek(seconds).map_err(Into::into)
    }

    // ── Test tone ────────────────────────────────────────────────────────────

    /// Enable or disable the sine test tone.
    ///
    /// The tone sounds immediately when the stream is running, regardless of
    /// the transport play/pause state.  Useful for hardware verification.
    ///
    /// `frequency` — Hz (e.g. 440.0 for A4).  Defaults to 440 on `new()`.
    #[napi]
    pub fn set_test_tone(&self, enabled: bool, frequency: f64) {
        self.inner.set_test_tone(enabled, frequency as f32);
    }

    // ── Master volume ────────────────────────────────────────────────────────

    /// Set the master output volume.
    ///
    /// `value` — linear gain in `[0.0, 2.0]` (1.0 = unity, 2.0 = +6 dBFS).
    /// Values outside the range are clamped.
    ///
    /// This is applied inside the audio callback via an atomic — no locking.
    #[napi]
    pub fn set_master_volume(&self, value: f64) -> napi::Result<()> {
        self.inner
            .set_master_volume(value as f32)
            .map_err(Into::into)
    }

    // ── Project snapshot ─────────────────────────────────────────────────────

    /// Load a project snapshot from a JSON string.
    ///
    /// Expected format: `EngineProjectSnapshot` (see `types.rs` for the full
    /// schema).  The engine rebuilds its internal track graph from the snapshot.
    ///
    /// Throws on deserialization error.
    ///
    /// Example (TypeScript side):
    /// ```ts
    /// await engine.loadProject(JSON.stringify(projectSnapshot));
    /// ```
    #[napi]
    pub fn load_project(&self, snapshot_json: String) -> napi::Result<()> {
        let snapshot: EngineProjectSnapshot =
            serde_json::from_str(&snapshot_json).map_err(|e| {
                napi::Error::new(
                    napi::Status::InvalidArg,
                    format!("Invalid project snapshot JSON: {e}"),
                )
            })?;
        self.inner.load_project(snapshot).map_err(Into::into)
    }

    // ── Realtime param updates ───────────────────────────────────────────────

    /// Update a single parameter on a mixer track.
    ///
    /// `param_id` may be `"volume"` (0..2), `"pan"` (-1..1), or `"muted"` (0/1).
    ///
    /// The update is sent through the lock-free command queue and takes effect
    /// at the start of the next audio block.
    ///
    /// Throws if the stream is not open.
    #[napi]
    pub fn update_track_param(
        &self,
        track_id: String,
        param_id: String,
        value: f64,
    ) -> napi::Result<()> {
        self.inner
            .update_track_param(&track_id, &param_id, value)
            .map_err(Into::into)
    }

    /// Update a parameter on an insert effect on a specific track.
    ///
    /// Throws if the stream is not open.
    #[napi]
    pub fn update_insert_param(
        &self,
        track_id: String,
        insert_id: String,
        param_id: String,
        value: f64,
    ) -> napi::Result<()> {
        self.inner
            .update_insert_param(&track_id, &insert_id, &param_id, value)
            .map_err(Into::into)
    }

    /// Apply a JSON-encoded patch to a clip.
    ///
    /// **MVP note:** not yet processed by the audio callback; stored for future use.
    #[napi]
    pub fn update_clip(&self, clip_id: String, _patch_json: String) -> napi::Result<()> {
        eprintln!("[SphereAudio] updateClip '{clip_id}' — not yet implemented in MVP");
        Ok(())
    }

    // ── Debug info ───────────────────────────────────────────────────────────

    /// Return a debug snapshot of the engine's current runtime state.
    ///
    /// Useful for verifying that the project was loaded and clips are ready:
    /// ```ts
    /// const info = engine.getDebugInfo();
    /// console.log(info.loadedClips, info.readyClips, info.clipSummaries);
    /// ```
    #[napi]
    pub fn get_debug_info(&self) -> JsEngineDebugInfo {
        self.inner.get_debug_info()
    }

    // ── Meters ───────────────────────────────────────────────────────────────

    /// Read the current meter snapshot (peak + RMS for L and R master bus).
    ///
    /// Values are linear amplitudes in `[0.0, 1.0]`.  Poll at ~20 fps from the
    /// JS side for smooth VU meter display.
    ///
    /// Returns zeros when the stream is not running.
    #[napi]
    pub fn get_meters(&self) -> JsMeterSnapshot {
        self.inner.get_meters()
    }
}
