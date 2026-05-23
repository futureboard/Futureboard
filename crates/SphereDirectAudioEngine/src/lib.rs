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
pub mod backend;
mod command;
pub mod device;
mod dsp;
pub mod engine;
pub mod error;
mod graph;
pub mod recording;
mod runtime;
pub mod types;
mod vst3_processor;

use std::sync::Arc;

use napi_derive::napi;

use engine::EngineInner;
use types::{
    EngineProjectSnapshot, JsAudioDeviceInfo, JsDauxBackendInfo, JsDauxConfig, JsDauxStatus,
    JsDeviceOpenConfig, JsEngineDebugInfo, JsMeterSnapshot, JsRecordingResult, JsRecordingStatus,
    JsSphereAudioStatus, JsStartRecordingConfig, JsWavPeakResult,
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

    /// Return built-in SphereAudioPlugins descriptors as JSON.
    ///
    /// The renderer can use this to build extension-style plugin browsers while
    /// DAUx uses the same IDs in the realtime insert chain.
    #[napi]
    pub fn get_builtin_audio_plugins_json(&self) -> napi::Result<String> {
        serde_json::to_string(&sphere_audio_plugins::builtin_descriptors()).map_err(|error| {
            napi::Error::new(
                napi::Status::GenericFailure,
                format!("Failed to serialize built-in audio plugin descriptors: {error}"),
            )
        })
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

    /// Open the native VST3 editor bound to the existing insert processor.
    ///
    /// This must be called from the UI/control side only. It does not route
    /// audio through JS; editor parameter changes are queued natively and
    /// consumed by the audio callback on the next process block.
    #[napi]
    pub fn open_insert_editor(
        &self,
        track_id: String,
        insert_id: String,
        window_id: String,
        title: String,
        width: i32,
        height: i32,
    ) -> napi::Result<f64> {
        self.inner
            .open_insert_editor(&track_id, &insert_id, &window_id, &title, width, height)
            .map(|handle| handle as f64)
            .map_err(Into::into)
    }

    #[napi]
    pub fn close_insert_editor(&self, track_id: String, insert_id: String) -> napi::Result<()> {
        self.inner
            .close_insert_editor(&track_id, &insert_id)
            .map_err(Into::into)
    }

    #[napi]
    pub fn focus_insert_editor(&self, track_id: String, insert_id: String) -> napi::Result<bool> {
        self.inner
            .focus_insert_editor(&track_id, &insert_id)
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

    // ── DAUx backend API ─────────────────────────────────────────────────────

    /// Return all DAUx backends available on the current platform.
    ///
    /// Use the returned `id` values with `openDaux()` to select a backend.
    #[napi]
    pub fn list_daux_backends(&self) -> Vec<JsDauxBackendInfo> {
        self.inner.list_daux_backends()
    }

    /// Open (or re-open) a DAUx stream with a specific backend, device, and
    /// buffer configuration.
    ///
    /// This is the preferred way to open the audio device in Electron.
    /// After a successful call, use `start()` to begin audio output.
    ///
    /// Example (TypeScript):
    /// ```ts
    /// engine.openDaux({ backendId: "wasapi-exclusive", bufferSize: 128, mmcssPriority: true });
    /// engine.start();
    /// ```
    #[napi]
    pub fn open_daux(&self, config: JsDauxConfig) -> napi::Result<()> {
        // Catch any Rust panic so it does not cross the NAPI boundary and
        // terminate the Electron process.
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.inner.open_daux(config).map_err(Into::into)
        }))
        .unwrap_or_else(|_| {
            Err(napi::Error::new(
                napi::Status::GenericFailure,
                "WASAPI: internal panic during audio backend open".to_string(),
            ))
        })
    }

    /// Safe variant of `openDaux` that restores the previous working backend if
    /// the requested config fails.  The stream is always left in an open (or
    /// previously-open) state.  On failure the returned error describes what
    /// happened and which backend was restored.
    #[napi]
    pub fn open_daux_safe(&self, config: JsDauxConfig) -> napi::Result<()> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.inner.open_daux_safe(config).map_err(Into::into)
        }))
        .unwrap_or_else(|_| {
            Err(napi::Error::new(
                napi::Status::GenericFailure,
                "WASAPI: internal panic during safe audio backend switch".to_string(),
            ))
        })
    }

    /// Return the current DAUx runtime status: backend, device, latency, glitches.
    ///
    /// Poll this at ~1Hz to update the Settings / status bar UI.
    #[napi]
    pub fn get_daux_status(&self) -> JsDauxStatus {
        self.inner.get_daux_status()
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

    /// Generate Int16 min/max waveform peaks for a PCM WAV file by streaming
    /// the source from disk. This is used by Electron import/background jobs so
    /// renderer drag/drop never decodes or scans long files.
    #[napi]
    pub fn generate_wav_peaks(
        &self,
        file_path: String,
        file_id: String,
        samples_per_peak: u32,
    ) -> napi::Result<JsWavPeakResult> {
        let result = audio_file::generate_wav_peaks_from_path(&file_path, samples_per_peak)
            .map_err(|e| {
                napi::Error::from_reason(format!("Waveform peak generation failed: {e}"))
            })?;
        Ok(JsWavPeakResult {
            file_id,
            sample_rate: result.sample_rate,
            channel_count: result.channel_count,
            duration: result.duration,
            samples_per_peak: result.samples_per_peak,
            peak_count: result.peak_count,
            peaks: result.peaks,
        })
    }

    // ── Recording ────────────────────────────────────────────────────────────

    /// Begin recording armed tracks to WAV files in `<projectRoot>/Media/Audio`.
    ///
    /// Opens a separate cpal input stream on the selected input device.
    /// Audio data is routed through a lock-free channel to a disk writer thread —
    /// the output audio callback is not affected.
    ///
    /// Throws if a session is already active or if the device cannot be opened.
    #[napi]
    pub fn start_recording(&self, config: JsStartRecordingConfig) -> napi::Result<()> {
        self.inner
            .start_recording(config)
            .map_err(|e| napi::Error::from_reason(e.to_string()))
    }

    /// Stop the active recording session, finalize WAV files, and return per-track results.
    ///
    /// Drops the input stream (causing the disk writer to flush and close its
    /// files), then waits up to 60 s for finalization before returning.
    ///
    /// Throws if no recording is active.
    #[napi]
    pub fn stop_recording(&self) -> napi::Result<Vec<JsRecordingResult>> {
        self.inner
            .stop_recording()
            .map_err(|e| napi::Error::from_reason(e.to_string()))
    }

    /// Return a lightweight recording status snapshot for UI polling.
    #[napi]
    pub fn get_recording_status(&self) -> JsRecordingStatus {
        self.inner.get_recording_status()
    }
}
