//! Core engine logic.
//!
//! `EngineInner` is the real audio engine.  It is wrapped in `Arc` and shared
//! between the N-API class (on the JS/main thread) and the audio callback
//! (on cpal's realtime thread).
//!
//! Thread safety:
//!   - All control parameters are sent through a `crossbeam_channel` and
//!     consumed at the top of each audio block — no locking in the hot path.
//!   - Meter output uses `AtomicU32` (f32 bit-cast) — both sides access with
//!     `Relaxed` ordering, which is sufficient for meter display purposes.
//!   - Stream lifecycle (open/close) is guarded by `parking_lot::Mutex`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{BufferSize, FromSample, Sample, SampleFormat, SizedSample};
use crossbeam_channel::{bounded, Receiver, Sender};
use parking_lot::Mutex;
use sphere_audio_plugins::{canonical_plugin_id, process_stereo_sample};

use crate::audio_graph::is_routing_track_type;
use crate::audio_source::{sample_source_stereo, ClipAudioSource};
#[cfg(target_os = "windows")]
use crate::backend::wasapi_exclusive::{self, WasapiExclusiveHandle};
use crate::backend::{
    cpal_backend::{self, CpalStreamHandle},
    list_available_backends, BackendKind, DauxDeviceConfig,
};
use crate::command::EngineCommand;
use crate::device;
use crate::dsp::{meter::smooth_peak, oscillator::SineOscillator};
use crate::error::SphereAudioError;
use crate::graph::{MasterState, TrackState};
use crate::latency_graph::apply_pdc_delay_block;
use crate::recording::{self, RecordingSession};
use crate::runtime::{RuntimeInsert, RuntimePreviewMode, RuntimeProject, RuntimeTrack};
use crate::tempo_map::{TempoMap, TempoPoint};
use crate::transport::{self, RuntimeTransportSnapshot};
use crate::vst3_processor::RuntimeTransportContext;
use crate::types::{
    EngineProjectSnapshot, EngineStatus, JsAudioDeviceInfo, JsDauxBackendInfo, JsDauxConfig,
    JsDauxStatus, JsDeviceOpenConfig, JsEngineDebugInfo, JsMeterSnapshot, JsRecordingResult,
    JsRecordingStatus, JsSphereAudioStatus, JsStartRecordingConfig, JsTrackMeterSnapshot,
};

// ── Version ───────────────────────────────────────────────────────────────────

pub const ENGINE_VERSION: &str = "0.1.0";

fn command_debug_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("FUTUREBOARD_AUDIO_COMMAND_DEBUG").is_some())
}

/// `FUTUREBOARD_INPUT_DEBUG=1` enables throttled raw-input-peak traces from the
/// control thread (see `log_input_debug_throttled`). Off by default.
fn input_debug_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("FUTUREBOARD_INPUT_DEBUG").is_some())
}

/// `FUTUREBOARD_AUDIO_CALLBACK_DEBUG=1` enables the realtime callback's
/// occasional eprintln traces (graph swap, mute, render-path). Off by default
/// so the audio thread never formats strings or writes to stdio — see
/// `tasks/native/audio-system-spec.md` §1 and Phase A finding A.2.2.
fn callback_debug_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("FUTUREBOARD_AUDIO_CALLBACK_DEBUG").is_some())
}

fn plugin_restore_debug_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("FUTUREBOARD_PLUGIN_DEBUG").is_some())
}

// ── Realtime constants shared with render.rs ──────────────────────────────────

pub const TEST_TONE_AMPLITUDE: f32 = 0.125; // −18 dBFS  (safe default test level)
pub const PEAK_DECAY: f32 = 0.94; // per audio block, responsive UI peak decay

// ── Atomic helpers (pub for render.rs) ────────────────────────────────────────

#[inline]
pub fn f32_store(v: f32) -> u32 {
    v.to_bits()
}
#[inline]
pub fn f32_load(v: u32) -> f32 {
    f32::from_bits(v)
}

#[inline]
fn atomic_max_f32_bits(target: &AtomicU32, value: f32) {
    let value = value.max(0.0);
    let mut current = target.load(Ordering::Relaxed);
    loop {
        if value <= f32::from_bits(current) {
            break;
        }
        match target.compare_exchange_weak(
            current,
            value.to_bits(),
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(next) => current = next,
        }
    }
}

// ── Audio engine lifecycle (control → audio, Relaxed reads in callback) ───────

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioEngineState {
    Running = 0,
    Paused = 1,
    LoadingProject = 2,
    ClosingProject = 3,
    DeviceSwitching = 4,
    Suspended = 5,
}

impl AudioEngineState {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Paused,
            2 => Self::LoadingProject,
            3 => Self::ClosingProject,
            4 => Self::DeviceSwitching,
            5 => Self::Suspended,
            _ => Self::Running,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Paused => "Paused",
            Self::LoadingProject => "LoadingProject",
            Self::ClosingProject => "ClosingProject",
            Self::DeviceSwitching => "DeviceSwitching",
            Self::Suspended => "Suspended",
        }
    }

    pub fn outputs_silence(self) -> bool {
        !matches!(self, Self::Running)
    }
}

// ── Shared state (accessed by both control and audio threads) ─────────────────

pub struct SharedState {
    // Meters (audio → control)
    pub peak_l: AtomicU32,
    pub peak_r: AtomicU32,
    pub rms_l: AtomicU32,
    pub rms_r: AtomicU32,

    // Control flags (control → audio, relaxed reads in callback)
    pub test_tone_enabled: AtomicBool,
    pub test_tone_freq: AtomicU32, // Hz as f32 bits
    pub master_volume: AtomicU32,  // linear f32 bits
    pub playing: AtomicBool,
    /// Lifecycle gate for realtime output (see [`AudioEngineState`]).
    pub engine_state: AtomicU8,
    pub position_samples: AtomicU64, // samples elapsed from start
    pub sample_rate: AtomicU32,

    // Transport clock (control ↔ audio via commands + Relaxed reads)
    pub bpm_bits: AtomicU64,
    pub time_sig_num: AtomicU32,
    pub time_sig_den: AtomicU32,
    pub loop_enabled: AtomicBool,
    pub loop_start_samples: AtomicU64,
    pub loop_end_samples: AtomicU64,
    pub metronome_enabled: AtomicBool,

    // Recording (Phase U): input monitor tap + session flag for realtime mix.
    pub recording_active: AtomicBool,
    pub recording_monitor_mix: AtomicBool,
    pub recording_monitor_l: AtomicU32,
    pub recording_monitor_r: AtomicU32,
    pub live_input_active: AtomicBool,
    pub live_input_l: AtomicU32,
    pub live_input_r: AtomicU32,
    pub live_input_peak_l: AtomicU32,
    pub live_input_peak_r: AtomicU32,

    // ── Live monitoring / input bus (Layers 4, 6, 7) ──────────────────────
    /// Lock-free stereo bridge from the input callback to the output render
    /// callback. Carries the actual monitored samples (not just a peak).
    pub input_ring: crate::input_ring::InputRing,
    /// `true` when any track has monitoring enabled — gates the output-side
    /// monitor mix so the render callback only taps the ring when needed.
    pub monitor_enabled_any: AtomicBool,
    /// Linear monitor gain applied to the input bus before it reaches master.
    pub monitor_gain: AtomicU32,
    /// Peak of the input bus *after* the render callback reads it from the ring
    /// (Layer 4 verification — distinct from the raw `live_input_peak_*`).
    pub input_bus_peak_l: AtomicU32,
    pub input_bus_peak_r: AtomicU32,

    // ── Realtime diagnostics counters (Part 4) ────────────────────────────
    pub input_cb_count: AtomicU64,
    pub output_cb_count: AtomicU64,
    pub input_frames_received: AtomicU64,
    pub monitor_frames_consumed: AtomicU64,
    pub monitor_ring_underruns: AtomicU64,
    pub monitor_ring_overruns: AtomicU64,
    pub record_ring_overruns: AtomicU64,
    pub output_xruns: AtomicU64,
    /// Peak of the record capture (pre-file) for diagnostics.
    pub record_peak: AtomicU32,
    /// Peak actually mixed to the monitor output for diagnostics.
    pub monitor_output_peak: AtomicU32,

    // ── Realtime recording waveform preview (Part 1) ──────────────────────
    /// Finalized preview bins pushed by the recording input callback.
    pub preview_ring: crate::input_ring::PreviewPeakRing,
    /// `true` while a take is feeding the preview ring.
    pub recording_preview_active: AtomicBool,
    /// Monotonic id for the current take (lets the UI discard stale chunks).
    pub recording_preview_id: AtomicU64,
    /// Transport sample at which the take started (preview clip origin).
    pub recording_preview_start_sample: AtomicU64,
    pub recording_preview_sample_rate: AtomicU32,
    pub recording_preview_channels: AtomicU32,
    pub recording_preview_peaks_per_sec: AtomicU32,

    // ── Callback watchdog (audio-hang spec §12; audio → control) ──────────
    /// Duration of the most recent output callback, microseconds.
    pub last_callback_us: AtomicU32,
    /// Worst output-callback duration seen since stream open, microseconds.
    pub max_callback_us: AtomicU32,
    /// Blocks that exceeded the 2 ms debug threshold.
    pub slow_callback_count: AtomicU64,

    // DAUx diagnostics (incremented by audio thread, read by control thread)
    pub glitch_count: AtomicU64,
    pub mmcss_active: AtomicBool,
    /// Set by a backend when the audio device disappears mid-stream (USB
    /// unplugged, default device changed, exclusive-mode timeout). Read by the
    /// control thread to surface a DeviceLost state and trigger recovery.
    pub device_lost: AtomicBool,
    /// Playback plugin delay compensation (Phase W). Settings → Playback.
    pub pdc_enabled: AtomicBool,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            peak_l: AtomicU32::new(f32_store(0.0)),
            peak_r: AtomicU32::new(f32_store(0.0)),
            rms_l: AtomicU32::new(f32_store(0.0)),
            rms_r: AtomicU32::new(f32_store(0.0)),
            test_tone_enabled: AtomicBool::new(false),
            test_tone_freq: AtomicU32::new(f32_store(440.0)),
            master_volume: AtomicU32::new(f32_store(1.0)),
            playing: AtomicBool::new(false),
            engine_state: AtomicU8::new(AudioEngineState::Paused as u8),
            position_samples: AtomicU64::new(0),
            sample_rate: AtomicU32::new(44100),
            bpm_bits: AtomicU64::new(120.0_f64.to_bits()),
            time_sig_num: AtomicU32::new(4),
            time_sig_den: AtomicU32::new(4),
            loop_enabled: AtomicBool::new(false),
            loop_start_samples: AtomicU64::new(0),
            loop_end_samples: AtomicU64::new(0),
            metronome_enabled: AtomicBool::new(false),
            recording_active: AtomicBool::new(false),
            recording_monitor_mix: AtomicBool::new(false),
            recording_monitor_l: AtomicU32::new(f32_store(0.0)),
            recording_monitor_r: AtomicU32::new(f32_store(0.0)),
            live_input_active: AtomicBool::new(false),
            live_input_l: AtomicU32::new(f32_store(0.0)),
            live_input_r: AtomicU32::new(f32_store(0.0)),
            live_input_peak_l: AtomicU32::new(f32_store(0.0)),
            live_input_peak_r: AtomicU32::new(f32_store(0.0)),
            input_ring: crate::input_ring::InputRing::default(),
            monitor_enabled_any: AtomicBool::new(false),
            monitor_gain: AtomicU32::new(f32_store(1.0)),
            input_bus_peak_l: AtomicU32::new(f32_store(0.0)),
            input_bus_peak_r: AtomicU32::new(f32_store(0.0)),
            input_cb_count: AtomicU64::new(0),
            output_cb_count: AtomicU64::new(0),
            input_frames_received: AtomicU64::new(0),
            monitor_frames_consumed: AtomicU64::new(0),
            monitor_ring_underruns: AtomicU64::new(0),
            monitor_ring_overruns: AtomicU64::new(0),
            record_ring_overruns: AtomicU64::new(0),
            output_xruns: AtomicU64::new(0),
            record_peak: AtomicU32::new(f32_store(0.0)),
            monitor_output_peak: AtomicU32::new(f32_store(0.0)),
            preview_ring: crate::input_ring::PreviewPeakRing::default(),
            recording_preview_active: AtomicBool::new(false),
            recording_preview_id: AtomicU64::new(0),
            recording_preview_start_sample: AtomicU64::new(0),
            recording_preview_sample_rate: AtomicU32::new(0),
            recording_preview_channels: AtomicU32::new(0),
            recording_preview_peaks_per_sec: AtomicU32::new(0),
            last_callback_us: AtomicU32::new(0),
            max_callback_us: AtomicU32::new(0),
            slow_callback_count: AtomicU64::new(0),
            glitch_count: AtomicU64::new(0),
            mmcss_active: AtomicBool::new(false),
            device_lost: AtomicBool::new(false),
            pdc_enabled: AtomicBool::new(true),
        }
    }
}

// ── Engine inner ───────────────────────────────────────────────────────────────

/// Active stream variant — either a cpal-backed stream or a WASAPI exclusive thread.
enum ActiveStream {
    Cpal(CpalStreamHandle),
    #[cfg(target_os = "windows")]
    WasapiExclusive(WasapiExclusiveHandle),
}

impl ActiveStream {
    fn cmd_tx(&self) -> Option<&crossbeam_channel::Sender<EngineCommand>> {
        match self {
            ActiveStream::Cpal(h) => Some(&h.cmd_tx),
            #[cfg(target_os = "windows")]
            ActiveStream::WasapiExclusive(h) => Some(&h.cmd_tx),
        }
    }
    fn play(&self) -> Result<(), String> {
        match self {
            ActiveStream::Cpal(h) => h.play(),
            #[cfg(target_os = "windows")]
            ActiveStream::WasapiExclusive(_) => Ok(()), // already playing from stream start
        }
    }
    fn pause(&self) -> Result<(), String> {
        match self {
            ActiveStream::Cpal(h) => h.pause(),
            #[cfg(target_os = "windows")]
            ActiveStream::WasapiExclusive(_) => Ok(()), // no pause in exclusive — caller mutes output
        }
    }
    #[allow(dead_code)]
    fn sample_rate(&self) -> u32 {
        match self {
            ActiveStream::Cpal(h) => h.sample_rate,
            #[cfg(target_os = "windows")]
            ActiveStream::WasapiExclusive(h) => h.sample_rate,
        }
    }
    #[allow(dead_code)]
    fn buffer_size(&self) -> u32 {
        match self {
            ActiveStream::Cpal(h) => h.buffer_size,
            #[cfg(target_os = "windows")]
            ActiveStream::WasapiExclusive(h) => h.buffer_size,
        }
    }
    #[allow(dead_code)]
    fn device_name(&self) -> &str {
        match self {
            ActiveStream::Cpal(h) => &h.device_name,
            #[cfg(target_os = "windows")]
            ActiveStream::WasapiExclusive(h) => &h.device_name,
        }
    }
    fn backend_name(&self) -> &str {
        match self {
            ActiveStream::Cpal(h) => &h.backend_name,
            #[cfg(target_os = "windows")]
            ActiveStream::WasapiExclusive(_) => "DAUx WASAPI Exclusive",
        }
    }
}

// Safety: same rationale as before — all access is on the JS/main thread under Mutex.
unsafe impl Send for ActiveStream {}
unsafe impl Sync for ActiveStream {}

/// Live input-level test stream (Settings "Test Input" button). Holds the
/// cpal input stream and a shared peak atomic the callback writes and the UI
/// polls. Stored only inside `EngineInner` and touched only on the control
/// thread, so it inherits `EngineInner`'s `Send`/`Sync` assertion.
struct InputTestHandle {
    _stream: cpal::Stream,
    peak: Arc<AtomicU32>,
}

struct LiveInputHandle {
    _stream: cpal::Stream,
    device_id: Option<String>,
}

pub struct EngineInner {
    // Shared atomic state
    pub shared: Arc<SharedState>,

    // Active stream (cpal or WASAPI exclusive).
    // Dropping this stops the stream.
    active_stream: Mutex<Option<ActiveStream>>,

    // Legacy cpal stream path kept for compatibility with open_device().
    stream: Mutex<Option<cpal::Stream>>,
    cmd_tx: Mutex<Option<Sender<EngineCommand>>>,

    // Non-realtime mutable status (device names, error strings, etc.)
    status: Mutex<EngineStatus>,

    // In-memory track graph — updated from project snapshots / param commands.
    tracks: Mutex<Vec<TrackState>>,
    #[allow(dead_code)]
    master: Mutex<MasterState>,

    // Last loaded project snapshot (optional, for reference/debugging).
    project: Mutex<Option<EngineProjectSnapshot>>,

    // Prepared render graph shared with new streams and pushed to callbacks.
    runtime: Mutex<RuntimeProject>,
    audio_cache: Mutex<HashMap<String, Arc<ClipAudioSource>>>,

    // DAUx config & glitch counter (shared with audio thread for diagnostics).
    glitch_counter: Arc<AtomicU64>,
    daux_config: Mutex<DauxDeviceConfig>,

    // Active recording session (None when not recording).
    recording: Mutex<Option<RecordingSession>>,

    // Live input-level test stream (None when not testing input).
    input_test: Mutex<Option<InputTestHandle>>,

    // Engine-owned live input stream for armed/monitored track meters.
    live_input: Mutex<Option<LiveInputHandle>>,
}

#[derive(Clone)]
struct OutputStreamCandidate {
    config: cpal::StreamConfig,
    sample_format: SampleFormat,
    label: &'static str,
}

// ── Thread-safety declarations ────────────────────────────────────────────────
//
// On Windows, `cpal::Stream` (WASAPI backend) carries a
// `NotSendSyncAcrossAllPlatforms(PhantomData<*mut ()>)` marker that makes it
// `!Send`.  This is conservative — the WASAPI COM interfaces *can* be used from
// another thread as long as the stream is not concurrently accessed without
// synchronisation.
//
// Safety contract we uphold:
//   1. `EngineInner::stream` is accessed ONLY from the JS / main thread (via
//      `parking_lot::Mutex`).  No code path sends the `Mutex` guard across a
//      thread boundary.
//   2. The cpal audio callback has its OWN state captures (oscillator, local
//      vars) and accesses shared data only through `Arc<SharedState>` (which
//      is genuinely `Send + Sync` — plain atomics).  It never touches the
//      `Stream` handle.
//   3. The `Sender<EngineCommand>` inside `cmd_tx` is `Send`; it is only
//      written from the JS thread and read inside the audio callback's
//      `try_recv()`.
//
// Given these invariants, treating `EngineInner` as `Send + Sync` does not
// introduce data races.
unsafe impl Send for EngineInner {}
unsafe impl Sync for EngineInner {}

impl Default for EngineInner {
    fn default() -> Self {
        Self::new()
    }
}

impl EngineInner {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(SharedState::default()),
            active_stream: Mutex::new(None),
            stream: Mutex::new(None),
            cmd_tx: Mutex::new(None),
            status: Mutex::new(EngineStatus::default()),
            tracks: Mutex::new(Vec::new()),
            master: Mutex::new(MasterState::default()),
            project: Mutex::new(None),
            runtime: Mutex::new(RuntimeProject::default()),
            audio_cache: Mutex::new(HashMap::new()),
            glitch_counter: Arc::new(AtomicU64::new(0)),
            daux_config: Mutex::new(DauxDeviceConfig::default()),
            recording: Mutex::new(None),
            input_test: Mutex::new(None),
            live_input: Mutex::new(None),
        }
    }

    #[inline]
    pub fn pdc_enabled(&self) -> bool {
        if std::env::var_os("FUTUREBOARD_PDC").is_some_and(|v| v == "0" || v == "false") {
            return false;
        }
        self.shared.pdc_enabled.load(Ordering::Relaxed)
    }

    pub fn set_pdc_enabled(&self, enabled: bool) {
        self.shared.pdc_enabled.store(enabled, Ordering::Relaxed);
    }

    /// Current transport play flag (set/cleared only by Start/StopTransport).
    pub fn transport_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Relaxed)
    }

    /// Current lifecycle gate of the realtime callback.
    pub fn engine_state(&self) -> AudioEngineState {
        AudioEngineState::from_u8(self.shared.engine_state.load(Ordering::Relaxed))
    }

    // ── Lifecycle ──────────────────────────────────────────────────────────

    pub fn get_version(&self) -> String {
        ENGINE_VERSION.to_string()
    }

    pub fn get_status(&self) -> JsSphereAudioStatus {
        let st = self.status.lock().clone();
        let sample_rate = self.shared.sample_rate.load(Ordering::Relaxed).max(1);
        let position_samples = self.shared.position_samples.load(Ordering::Relaxed);
        JsSphereAudioStatus {
            available: true,
            running: st.running,
            stream_open: st.stream_open,
            transport_playing: self.shared.playing.load(Ordering::Relaxed),
            position_seconds: position_samples as f64 / sample_rate as f64,
            version: ENGINE_VERSION.to_string(),
            backend_name: cpal::default_host().id().name().to_string(),
            sample_rate: st.sample_rate,
            buffer_size: st.buffer_size,
            input_device: st.input_device,
            output_device: st.output_device,
            last_error: st.last_error,
        }
    }

    pub fn list_input_devices(&self) -> Vec<JsAudioDeviceInfo> {
        device::list_input_devices()
    }

    pub fn list_output_devices(&self) -> Vec<JsAudioDeviceInfo> {
        device::list_output_devices()
    }

    /// Open (or re-open) an audio output stream.
    /// Closes any existing stream first.
    pub fn open_device(&self, config: JsDeviceOpenConfig) -> Result<(), SphereAudioError> {
        self.close_device_inner();

        let (dev, dev_name) = device::resolve_output_device(config.output_device_id.as_deref())
            .map_err(SphereAudioError::DeviceNotFound)?;

        let candidates =
            output_stream_candidates(&dev, &config).map_err(SphereAudioError::StreamOpenFailed)?;

        let mut last_error = None;
        let mut selected = None;

        for candidate in candidates {
            let (tx, rx) = bounded::<EngineCommand>(512);
            let shared_cb = Arc::clone(&self.shared);
            shared_cb
                .sample_rate
                .store(candidate.config.sample_rate.0, Ordering::Relaxed);
            let initial_runtime = self
                .project
                .lock()
                .as_ref()
                .map(|snapshot| {
                    let mut audio_cache = self.audio_cache.lock();
                    match RuntimeProject::build(
                        snapshot,
                        candidate.config.sample_rate.0,
                        &mut audio_cache,
                        None,
                        self.pdc_enabled(),
                    ) {
                        Ok(runtime) => runtime,
                        Err(e) => {
                            eprintln!(
                                "[SphereAudio] open_device: invalid routing graph ({e}), keeping previous runtime"
                            );
                            let mut runtime = self.runtime.lock().clone();
                            runtime.sample_rate = candidate.config.sample_rate.0;
                            runtime
                        }
                    }
                })
                .unwrap_or_else(|| {
                    let mut runtime = self.runtime.lock().clone();
                    runtime.sample_rate = candidate.config.sample_rate.0;
                    runtime
                });
            *self.runtime.lock() = initial_runtime.clone();

            match build_output_stream(
                &dev,
                &candidate.config,
                candidate.sample_format,
                rx,
                shared_cb,
                initial_runtime,
            ) {
                Ok(stream) => {
                    selected = Some((candidate, tx, stream));
                    break;
                }
                Err(e) => {
                    last_error = Some(format!("{} config failed: {e}", candidate.label));
                }
            }
        }

        let (selected_config, tx, stream) = selected.ok_or_else(|| {
            SphereAudioError::StreamOpenFailed(
                last_error.unwrap_or_else(|| "no stream config candidates available".to_string()),
            )
        })?;

        let sample_rate = selected_config.config.sample_rate.0;
        let buffer_size = reported_buffer_size(&selected_config.config);

        // Store the stream and sender.
        *self.stream.lock() = Some(stream);
        *self.cmd_tx.lock() = Some(tx);

        // Update status.
        let mut st = self.status.lock();
        st.stream_open = true;
        st.running = false;
        st.sample_rate = sample_rate;
        st.buffer_size = buffer_size;
        st.output_device = Some(dev_name);
        st.last_error = None;

        Ok(())
    }

    /// Stop and close the audio stream.
    pub fn close_device(&self) {
        self.close_device_inner();
    }

    fn close_device_inner(&self) {
        // Drop active DAUx stream (stops WASAPI exclusive thread or cpal stream).
        *self.active_stream.lock() = None;
        // Drop legacy cpal stream path.
        *self.stream.lock() = None;
        *self.cmd_tx.lock() = None;

        let mut st = self.status.lock();
        st.stream_open = false;
        st.running = false;
    }

    /// Start audio playback (calls `stream.play()`).
    pub fn start(&self) -> Result<(), SphereAudioError> {
        {
            let st = self.status.lock();
            if st.stream_open && st.running {
                return Ok(());
            }
        }

        // Try DAUx active stream first — drop `active_stream` lock before `play()`.
        let daux_play = {
            let guard = self.active_stream.lock();
            guard.as_ref().map(|stream| stream.play())
        };
        if let Some(result) = daux_play {
            result.map_err(SphereAudioError::StreamStartFailed)?;
            self.shared.playing.store(false, Ordering::Relaxed); // transport starts paused
            self.status.lock().running = true;
            return Ok(());
        }
        // Legacy cpal path.
        let play_result = {
            let guard = self.stream.lock();
            guard
                .as_ref()
                .ok_or(SphereAudioError::EngineNotOpen)?
                .play()
                .map_err(|e| SphereAudioError::StreamStartFailed(e.to_string()))
        };
        play_result?;
        self.shared.playing.store(false, Ordering::Relaxed); // transport starts paused
        self.status.lock().running = true;
        Ok(())
    }

    /// Stop audio playback (calls `stream.pause()`).
    pub fn stop(&self) {
        if let Some(stream) = self.active_stream.lock().as_ref() {
            let _ = stream.pause();
        } else if let Some(stream) = self.stream.lock().as_ref() {
            let _ = stream.pause();
        }
        self.shared.playing.store(false, Ordering::Relaxed);
        self.status.lock().running = false;
    }

    /// Explicit shutdown for application exit — do not rely on `Drop` alone.
    /// Stops transport, pauses the device stream, and releases realtime resources.
    pub fn shutdown(&self) {
        self.shared
            .engine_state
            .store(AudioEngineState::ClosingProject as u8, Ordering::Relaxed);
        eprintln!("[AudioEngine] state -> ClosingProject");
        self.shared.playing.store(false, Ordering::Relaxed);
        let _ = self.send_command(EngineCommand::StopTransport);
        self.stop();
        self.close_device_inner();
    }

    // ── Transport ──────────────────────────────────────────────────────────

    pub fn play(&self) -> Result<(), SphereAudioError> {
        if self.shared.playing.load(Ordering::Relaxed) {
            if transport_freeze_debug_enabled() {
                eprintln!("[play-debug engine] play() skipped — transport already playing");
            }
            return Ok(());
        }
        if transport_freeze_debug_enabled() {
            eprintln!("[play-debug engine] play() queuing StartTransport");
        }
        self.send_command(EngineCommand::StartTransport)
    }

    pub fn pause(&self) -> Result<(), SphereAudioError> {
        self.send_command(EngineCommand::StopTransport)
    }

    pub fn seek(&self, position_seconds: f64) -> Result<(), SphereAudioError> {
        self.send_command(EngineCommand::Seek { position_seconds })
    }

    pub fn set_metronome_enabled(&self, enabled: bool) -> Result<(), SphereAudioError> {
        self.shared
            .metronome_enabled
            .store(enabled, Ordering::Relaxed);
        self.send_command(EngineCommand::SetMetronomeEnabled(enabled))
    }

    pub fn set_bpm(&self, bpm: f64) -> Result<(), SphereAudioError> {
        self.set_tempo_map(bpm, Vec::new())
    }

    /// Replace the authoritative tempo map used for playback, metronome, and
    /// beat/time/sample conversion. `points` empty = static tempo at `default_bpm`.
    pub fn set_tempo_map(
        &self,
        default_bpm: f64,
        points: Vec<crate::types::EngineTempoPointSnapshot>,
    ) -> Result<(), SphereAudioError> {
        let snapshot = crate::runtime::build_tempo_map_from_points(default_bpm, &points);
        transport::store_f64_bits(&self.shared.bpm_bits, default_bpm);
        if let Some(project) = self.project.lock().as_mut() {
            project.bpm = default_bpm;
            project.tempo_points = points;
        }
        self.send_command(EngineCommand::SetTempoMap(snapshot))
    }

    /// Stage 3b: install (or clear, with `None`) the realtime sink for
    /// `track_id` — the audio callback mixes its external plugin-host DSP output
    /// into the master. Applied between blocks; no realtime allocation.
    pub fn set_plugin_bridge_sink(
        &self,
        insert_id: String,
        sink: Option<std::sync::Arc<dyn crate::plugin_bridge::PluginBridgeSink>>,
    ) -> Result<(), SphereAudioError> {
        self.send_command(EngineCommand::SetPluginBridgeSink { insert_id, sink })
    }

    pub fn set_bridge_editor_active(
        &self,
        track_id: String,
        active: bool,
    ) -> Result<(), SphereAudioError> {
        self.send_command(EngineCommand::SetBridgeEditorActive { track_id, active })
    }

    pub fn set_time_signature(
        &self,
        numerator: u32,
        denominator: u32,
    ) -> Result<(), SphereAudioError> {
        self.shared
            .time_sig_num
            .store(numerator.max(1), Ordering::Relaxed);
        self.shared
            .time_sig_den
            .store(denominator.max(1), Ordering::Relaxed);
        self.send_command(EngineCommand::SetTimeSignature(numerator, denominator))
    }

    pub fn set_time_signature_map(
        &self,
        points: Vec<crate::time_signature_map::RuntimeTimeSignaturePointSnapshot>,
    ) -> Result<(), SphereAudioError> {
        let snapshot =
            crate::time_signature_map::RuntimeTimeSignatureMapSnapshot::from_points(points);
        if let Some(pt) = snapshot.points().first() {
            self.shared
                .time_sig_num
                .store(pt.numerator.max(1) as u32, Ordering::Relaxed);
            self.shared
                .time_sig_den
                .store(pt.denominator.max(1) as u32, Ordering::Relaxed);
        }
        self.send_command(EngineCommand::SetTimeSignatureMap(snapshot))
    }

    pub fn set_loop(
        &self,
        enabled: bool,
        start_seconds: f64,
        end_seconds: f64,
    ) -> Result<(), SphereAudioError> {
        self.send_command(EngineCommand::SetLoop {
            enabled,
            start_seconds,
            end_seconds,
        })
    }

    pub fn midi_preview_note_on(
        &self,
        track_id: String,
        channel: u8,
        pitch: u8,
        velocity: u8,
    ) -> Result<(), SphereAudioError> {
        self.send_command(EngineCommand::MidiPreviewNoteOn {
            track_id,
            channel,
            pitch,
            velocity,
        })
    }

    pub fn midi_preview_note_off(
        &self,
        track_id: String,
        channel: u8,
        pitch: u8,
    ) -> Result<(), SphereAudioError> {
        self.send_command(EngineCommand::MidiPreviewNoteOff {
            track_id,
            channel,
            pitch,
        })
    }

    pub fn midi_preview_all_notes_off(&self, track_id: String) -> Result<(), SphereAudioError> {
        self.send_command(EngineCommand::MidiPreviewAllNotesOff { track_id })
    }

    pub fn plugin_preview_note_on(
        &self,
        track_id: String,
        plugin_instance_id: String,
        channel: u8,
        pitch: u8,
        velocity: u8,
    ) -> Result<(), SphereAudioError> {
        eprintln!(
            "[midi-preview-engine] queued note_on track={track_id} instance={plugin_instance_id} pitch={pitch}"
        );
        self.send_command(EngineCommand::PluginPreviewNoteOn {
            track_id,
            plugin_instance_id,
            channel,
            pitch,
            velocity,
        })
    }

    pub fn plugin_preview_note_off(
        &self,
        track_id: String,
        plugin_instance_id: String,
        channel: u8,
        pitch: u8,
    ) -> Result<(), SphereAudioError> {
        eprintln!(
            "[midi-preview-engine] queued note_off track={track_id} instance={plugin_instance_id} pitch={pitch}"
        );
        self.send_command(EngineCommand::PluginPreviewNoteOff {
            track_id,
            plugin_instance_id,
            channel,
            pitch,
        })
    }

    pub fn plugin_preview_all_notes_off(
        &self,
        track_id: String,
        plugin_instance_id: String,
    ) -> Result<(), SphereAudioError> {
        self.send_command(EngineCommand::PluginPreviewAllNotesOff {
            track_id,
            plugin_instance_id,
        })
    }

    /// Read the current transport/clock snapshot for UI polling.
    pub fn transport_snapshot(&self) -> RuntimeTransportSnapshot {
        RuntimeTransportSnapshot::from_shared(&self.shared, &self.tempo_map())
    }

    fn tempo_map(&self) -> TempoMap {
        self.project
            .lock()
            .as_ref()
            .map(tempo_map_from_project_snapshot)
            .unwrap_or_else(|| {
                TempoMap::static_tempo(transport::f64_from_bits(
                    self.shared.bpm_bits.load(Ordering::Relaxed),
                ))
            })
    }

    // ── Test tone ──────────────────────────────────────────────────────────

    pub fn set_test_tone(&self, enabled: bool, frequency: f32) {
        self.shared
            .test_tone_enabled
            .store(enabled, Ordering::Relaxed);
        self.shared
            .test_tone_freq
            .store(f32_store(frequency), Ordering::Relaxed);
        // Also send through command queue so the oscillator resets its freq.
        let _ = self.send_command(EngineCommand::SetTestTone { enabled, frequency });
    }

    // ── Meters ────────────────────────────────────────────────────────────

    pub fn get_meters(&self) -> JsMeterSnapshot {
        let tracks = self
            .runtime
            .lock()
            .meter_snapshots()
            .into_iter()
            .map(|meter| JsTrackMeterSnapshot {
                track_id: meter.track_id,
                peak_l: meter.peak_l as f64,
                peak_r: meter.peak_r as f64,
                rms_l: meter.rms_l as f64,
                rms_r: meter.rms_r as f64,
            })
            .collect();

        JsMeterSnapshot {
            tracks,
            master_peak_l: f32_load(self.shared.peak_l.load(Ordering::Relaxed)) as f64,
            master_peak_r: f32_load(self.shared.peak_r.load(Ordering::Relaxed)) as f64,
            master_rms_l: f32_load(self.shared.rms_l.load(Ordering::Relaxed)) as f64,
            master_rms_r: f32_load(self.shared.rms_r.load(Ordering::Relaxed)) as f64,
            input_peak_l: f32_load(self.shared.live_input_peak_l.swap(0, Ordering::Relaxed)) as f64,
            input_peak_r: f32_load(self.shared.live_input_peak_r.swap(0, Ordering::Relaxed)) as f64,
        }
    }

    /// Full-pipeline diagnostics snapshot (Layer 10). Non-destructive — reads
    /// peaks without resetting them so it can be polled alongside `get_meters`.
    pub fn get_audio_diagnostics(&self) -> crate::types::JsAudioDiagnostics {
        use crate::types::{JsAudioDiagnostics, JsTrackInputDiagnostics};
        let st = self.status.lock().clone();
        let input_device_name = self
            .live_input
            .lock()
            .as_ref()
            .and_then(|h| h.device_id.clone());
        let input_running = self.live_input.lock().is_some() && self.shared.input_ring.is_active();

        let raw_input_peak = f32_load(self.shared.live_input_peak_l.load(Ordering::Relaxed)).max(
            f32_load(self.shared.live_input_peak_r.load(Ordering::Relaxed)),
        );
        let input_bus_peak = f32_load(self.shared.input_bus_peak_l.load(Ordering::Relaxed)).max(
            f32_load(self.shared.input_bus_peak_r.load(Ordering::Relaxed)),
        );
        let output_peak = f32_load(self.shared.peak_l.load(Ordering::Relaxed))
            .max(f32_load(self.shared.peak_r.load(Ordering::Relaxed)));

        let runtime = self.runtime.lock();
        let tracks = runtime
            .tracks
            .iter()
            .filter(|t| t.track_type == "audio")
            .map(|t| {
                let meter = t.meter.load(&t.id);
                JsTrackInputDiagnostics {
                    track_id: t.id.clone(),
                    record_armed: t.record_armed,
                    monitor_enabled: t.monitor_enabled,
                    input_source: format!("{:?}", t.input_source),
                    track_input_peak: meter.peak_l.max(meter.peak_r) as f64,
                    track_output_peak: meter.peak_l.max(meter.peak_r) as f64,
                }
            })
            .collect();

        JsAudioDiagnostics {
            backend: cpal::default_host().id().name().to_string(),
            input_device_name,
            output_device_name: st.output_device,
            input_stream_running: input_running,
            output_stream_running: st.running,
            input_sample_rate: self.shared.input_ring.sample_rate(),
            input_channels: self.shared.input_ring.channels(),
            raw_input_peak: raw_input_peak as f64,
            input_bus_peak: input_bus_peak as f64,
            output_peak: output_peak as f64,
            tracks,
            input_callback_count: self.shared.input_cb_count.load(Ordering::Relaxed) as f64,
            output_callback_count: self.shared.output_cb_count.load(Ordering::Relaxed) as f64,
            input_frames_received: self.shared.input_frames_received.load(Ordering::Relaxed) as f64,
            monitor_frames_consumed: self.shared.monitor_frames_consumed.load(Ordering::Relaxed)
                as f64,
            monitor_ring_underruns: self.shared.monitor_ring_underruns.load(Ordering::Relaxed)
                as f64,
            monitor_ring_overruns: self.shared.monitor_ring_overruns.load(Ordering::Relaxed) as f64,
            record_ring_overruns: self.shared.record_ring_overruns.load(Ordering::Relaxed) as f64,
            output_xruns: self.shared.output_xruns.load(Ordering::Relaxed) as f64,
            monitor_output_peak: f32_load(self.shared.monitor_output_peak.load(Ordering::Relaxed))
                as f64,
            record_peak: f32_load(self.shared.record_peak.load(Ordering::Relaxed)) as f64,
        }
    }

    /// Metadata + current bin count for the in-progress recording preview.
    pub fn recording_preview_info(&self) -> crate::types::JsRecordingPreviewInfo {
        crate::types::JsRecordingPreviewInfo {
            active: self.shared.recording_preview_active.load(Ordering::Relaxed),
            recording_id: self.shared.recording_preview_id.load(Ordering::Relaxed) as f64,
            start_sample: self
                .shared
                .recording_preview_start_sample
                .load(Ordering::Relaxed) as f64,
            sample_rate: self
                .shared
                .recording_preview_sample_rate
                .load(Ordering::Relaxed),
            channels: self
                .shared
                .recording_preview_channels
                .load(Ordering::Relaxed),
            peaks_per_second: self
                .shared
                .recording_preview_peaks_per_sec
                .load(Ordering::Relaxed),
            peak_count: self.shared.preview_ring.head() as f64,
        }
    }

    /// Drain preview bins in `[from_index, head)`. Cheap clone on the control
    /// thread; never blocks the audio path. Returns an empty vec when there is
    /// nothing new.
    pub fn drain_recording_preview_peaks(
        &self,
        from_index: f64,
    ) -> Vec<crate::types::JsWaveformPeak> {
        let head = self.shared.preview_ring.head();
        let mut from = from_index.max(0.0) as u64;
        // If the consumer lagged past the ring window, clamp to what's retained.
        let cap = crate::input_ring::PreviewPeakRing::default_capacity();
        if head.saturating_sub(from) > cap {
            from = head.saturating_sub(cap);
        }
        let mut out = Vec::with_capacity((head.saturating_sub(from)) as usize);
        let mut i = from;
        while i < head {
            let p = self.shared.preview_ring.read(i);
            out.push(crate::types::JsWaveformPeak {
                min: p.min as f64,
                max: p.max as f64,
                rms: p.rms as f64,
            });
            i += 1;
        }
        out
    }

    /// Throttled (≈500 ms) raw-input-peak trace, gated by
    /// `FUTUREBOARD_INPUT_DEBUG`. Called from the control thread (never the
    /// audio callback) so the realtime path stays allocation/IO-free.
    pub fn log_input_debug_throttled(&self) {
        if !input_debug_enabled() {
            return;
        }
        static LAST: OnceLock<Mutex<std::time::Instant>> = OnceLock::new();
        let last = LAST.get_or_init(|| {
            Mutex::new(std::time::Instant::now() - std::time::Duration::from_secs(1))
        });
        {
            let mut guard = last.lock();
            if guard.elapsed() < std::time::Duration::from_millis(500) {
                return;
            }
            *guard = std::time::Instant::now();
        }
        let s = &self.shared;
        let raw = f32_load(s.live_input_peak_l.load(Ordering::Relaxed))
            .max(f32_load(s.live_input_peak_r.load(Ordering::Relaxed)));
        let bus = f32_load(s.input_bus_peak_l.load(Ordering::Relaxed))
            .max(f32_load(s.input_bus_peak_r.load(Ordering::Relaxed)));
        let head = s.preview_ring.head();
        eprintln!(
            "[AudioRealtime] input_cb={} output_cb={} input_frames={} monitor_consumed={} \
             monitor_underruns={} monitor_overruns={} record_overruns={} output_xruns={} \
             raw_input_peak={raw:.4} monitor_output_peak={:.4} record_peak={:.4} input_stream={} monitor_any={}",
            s.input_cb_count.load(Ordering::Relaxed),
            s.output_cb_count.load(Ordering::Relaxed),
            s.input_frames_received.load(Ordering::Relaxed),
            s.monitor_frames_consumed.load(Ordering::Relaxed),
            s.monitor_ring_underruns.load(Ordering::Relaxed),
            s.monitor_ring_overruns.load(Ordering::Relaxed),
            s.record_ring_overruns.load(Ordering::Relaxed),
            s.output_xruns.load(Ordering::Relaxed),
            f32_load(s.monitor_output_peak.load(Ordering::Relaxed)),
            f32_load(s.record_peak.load(Ordering::Relaxed)),
            s.input_ring.is_active(),
            s.monitor_enabled_any.load(Ordering::Relaxed),
        );
        let _ = bus;
        if s.recording_preview_active.load(Ordering::Relaxed) {
            let sr = s
                .recording_preview_sample_rate
                .load(Ordering::Relaxed)
                .max(1);
            let pps = s
                .recording_preview_peaks_per_sec
                .load(Ordering::Relaxed)
                .max(1);
            eprintln!(
                "[RecordingPreview] recording_id={} peaks={} preview_duration_sec={:.2} sr={sr} pps={pps}",
                s.recording_preview_id.load(Ordering::Relaxed),
                head,
                head as f64 / pps as f64,
            );
        }
    }

    // ── Param updates ──────────────────────────────────────────────────────

    pub fn set_master_volume(&self, value: f32) -> Result<(), SphereAudioError> {
        self.shared
            .master_volume
            .store(f32_store(value.clamp(0.0, 2.0)), Ordering::Relaxed);
        Ok(())
    }

    pub fn update_track_param(
        &self,
        track_id: &str,
        param_id: &str,
        value: f64,
    ) -> Result<(), SphereAudioError> {
        if track_id == "__master__" && param_id == "volume" {
            return self.set_master_volume(value as f32);
        }

        match param_id {
            "volume" => self.send_command(EngineCommand::SetTrackVolume {
                track_id: track_id.into(),
                value: value as f32,
            }),
            "pan" => self.send_command(EngineCommand::SetTrackPan {
                track_id: track_id.into(),
                value: value as f32,
            }),
            "muted" => self.send_command(EngineCommand::SetTrackMute {
                track_id: track_id.into(),
                muted: value != 0.0,
            }),
            "solo" => self.send_command(EngineCommand::SetTrackSolo {
                track_id: track_id.into(),
                solo: value != 0.0,
            }),
            "previewMode" => self.send_command(EngineCommand::SetTrackPreviewMode {
                track_id: track_id.into(),
                value: value as f32,
            }),
            other => {
                // Unknown param — log but don't error (UI might send future params)
                eprintln!("[SphereAudio] Unknown track param: '{other}' (track={track_id})");
                Ok(())
            }
        }
    }

    pub fn update_insert_param(
        &self,
        track_id: &str,
        insert_id: &str,
        param_id: &str,
        value: f64,
    ) -> Result<(), SphereAudioError> {
        eprintln!(
            "[SphereAudio] queue insert param track={} insert={} param={} value={:.6}",
            track_id, insert_id, param_id, value
        );
        self.send_command(EngineCommand::SetInsertParam {
            track_id: track_id.into(),
            insert_id: insert_id.into(),
            param_id: param_id.into(),
            value: value as f32,
        })
    }

    pub fn open_insert_editor(
        &self,
        track_id: &str,
        insert_id: &str,
        window_id: &str,
        title: &str,
        width: i32,
        height: i32,
    ) -> Result<u64, SphereAudioError> {
        let mut runtime = self.runtime.lock();
        let Some(track) = runtime.tracks.iter_mut().find(|track| track.id == track_id) else {
            return Err(SphereAudioError::NativeError(format!(
                "track not found for insert editor: {track_id}"
            )));
        };
        let Some(insert) = track
            .inserts
            .iter_mut()
            .find(|insert| insert.id == insert_id)
        else {
            return Err(SphereAudioError::NativeError(format!(
                "insert not found for editor: track={track_id} insert={insert_id}"
            )));
        };
        let Some(vst3) = insert.vst3.as_mut() else {
            return Err(SphereAudioError::NativeError(format!(
                "insert has no ready VST3 processor: track={track_id} insert={insert_id}"
            )));
        };
        let handle = vst3
            .open_editor(window_id, title, width, height)
            .ok_or_else(|| {
                SphereAudioError::NativeError(format!(
                    "failed to open VST3 editor: track={track_id} insert={insert_id}"
                ))
            })?;
        eprintln!(
            "[SphereAudio] opened insert editor track={} insert={} handle={} processorHandle=0x{:x}",
            track_id,
            insert_id,
            handle,
            vst3.handle_value()
        );
        Ok(handle)
    }

    pub fn close_insert_editor(
        &self,
        track_id: &str,
        insert_id: &str,
    ) -> Result<(), SphereAudioError> {
        let mut runtime = self.runtime.lock();
        if let Some(vst3) = runtime
            .tracks
            .iter_mut()
            .find(|track| track.id == track_id)
            .and_then(|track| {
                track
                    .inserts
                    .iter_mut()
                    .find(|insert| insert.id == insert_id)
            })
            .and_then(|insert| insert.vst3.as_mut())
        {
            vst3.close_editor();
            eprintln!(
                "[SphereAudio] closed insert editor track={} insert={}",
                track_id, insert_id
            );
        }
        Ok(())
    }

    /// Clone the live runtime VST3 processor handle for an insert, if it has a
    /// ready native plugin instance. The returned handle is `Arc`-backed and
    /// cheap to clone; holding it keeps the C++ instance alive even across a
    /// runtime rebuild or insert removal (the GUI editor relies on this so it
    /// can attach to / refresh the *existing* instance without re-locking the
    /// engine every frame). Used by the GPUI PluginView for the embedded editor.
    pub fn insert_processor(
        &self,
        track_id: &str,
        insert_id: &str,
    ) -> Option<crate::vst3_processor::Vst3RuntimeProcessor> {
        let mut runtime = self.runtime.lock();
        runtime
            .tracks
            .iter_mut()
            .find(|track| track.id == track_id)
            .and_then(|track| {
                track
                    .inserts
                    .iter_mut()
                    .find(|insert| insert.id == insert_id)
            })
            .and_then(|insert| insert.vst3.as_ref().cloned())
    }

    pub fn focus_insert_editor(
        &self,
        track_id: &str,
        insert_id: &str,
    ) -> Result<bool, SphereAudioError> {
        let mut runtime = self.runtime.lock();
        if let Some(vst3) = runtime
            .tracks
            .iter_mut()
            .find(|track| track.id == track_id)
            .and_then(|track| {
                track
                    .inserts
                    .iter_mut()
                    .find(|insert| insert.id == insert_id)
            })
            .and_then(|insert| insert.vst3.as_mut())
        {
            return Ok(vst3.focus_editor());
        }
        Ok(false)
    }

    // ── Project snapshot ───────────────────────────────────────────────────

    pub fn load_project(&self, snapshot: EngineProjectSnapshot) -> Result<(), SphereAudioError> {
        let old_state = AudioEngineState::from_u8(
            self.shared
                .engine_state
                .swap(AudioEngineState::LoadingProject as u8, Ordering::Relaxed),
        );
        eprintln!(
            "[AudioEngineState] old={old_state:?} new=LoadingProject source=load_project playing={}",
            self.shared.playing.load(Ordering::Relaxed)
        );
        let output_sample_rate = self.shared.sample_rate.load(Ordering::Relaxed).max(1);

        // Log how many clips have paths before building runtime.
        let clips_with_path = snapshot
            .clips
            .iter()
            .filter(|c| {
                c.media_path
                    .as_deref()
                    .map(|p| !p.is_empty())
                    .unwrap_or(false)
            })
            .count();
        eprintln!(
            "[SphereAudio] load_project: id='{}' tracks={} snapshot_clips={} clips_with_path={}",
            snapshot.project_id,
            snapshot.tracks.len(),
            snapshot.clips.len(),
            clips_with_path,
        );

        if clips_with_path == 0 && !snapshot.clips.is_empty() {
            eprintln!(
                "[SphereAudio] ⚠ WARNING: all {} clips have null/empty mediaPath — no audio will play!",
                snapshot.clips.len()
            );
        }

        // Extract existing VST3 processors from the current runtime so they can
        // be reused in the new build if the same insert ID + plugin path + class_id
        // + sample_rate still match.  This prevents spurious processor destruction
        // (and editor HWND invalidation) when just reloading the project.
        let mut existing_vst3: HashMap<String, crate::vst3_processor::Vst3RuntimeProcessor> = {
            let mut current = self.runtime.lock();
            let mut map = HashMap::new();
            for track in &mut current.tracks {
                for insert in &mut track.inserts {
                    if let Some(vst3) = insert.vst3.take() {
                        vst3.set_destroy_reason("replaced-by-load-project");
                        map.insert(insert.id.clone(), vst3);
                    }
                }
            }
            map
        };

        let runtime = {
            let mut audio_cache = self.audio_cache.lock();
            match RuntimeProject::build(
                &snapshot,
                output_sample_rate,
                &mut audio_cache,
                Some(&mut existing_vst3),
                self.pdc_enabled(),
            ) {
                Ok(project) => {
                    // Evict cache entries no longer referenced by any clip in the new snapshot.
                    // This keeps memory bounded when clips are removed between project loads.
                    let active_paths: std::collections::HashSet<&str> = snapshot
                        .clips
                        .iter()
                        .filter_map(|c| c.media_path.as_deref())
                        .filter(|p| !p.is_empty())
                        .collect();
                    audio_cache.retain(|path, _| active_paths.contains(path.as_str()));
                    project
                }
                Err(e) => {
                    let mut current = self.runtime.lock();
                    for track in &mut current.tracks {
                        for insert in &mut track.inserts {
                            if insert.vst3.is_none() {
                                if let Some(vst3) = existing_vst3.remove(&insert.id) {
                                    insert.vst3 = Some(vst3);
                                }
                            }
                        }
                    }
                    // Failed build must not leave the engine wedged in
                    // LoadingProject (permanent silence) — restore.
                    self.shared
                        .engine_state
                        .store(old_state as u8, Ordering::Relaxed);
                    eprintln!(
                        "[AudioEngineState] old=LoadingProject new={old_state:?} source=load_project_failed"
                    );
                    return Err(e.into());
                }
            }
        };
        // Processors left in existing_vst3 had no matching insert in the new
        // snapshot — they are dropped here with reason="replaced-by-load-project".
        drop(existing_vst3);

        eprintln!(
            "[SphereAudio] RuntimeProject built: {} runtime clips from {} snapshot clips (sr={}) graph_nodes={} pass2={} rejected_routes={}",
            runtime.clips.len(),
            snapshot.clips.len(),
            output_sample_rate,
            runtime.audio_graph.nodes.len(),
            runtime.audio_graph.pass2_routing_indices.len(),
            runtime.audio_graph.rejected_routes.len(),
        );

        // Build initial track states from snapshot.
        let mut tracks = self.tracks.lock();
        tracks.clear();
        for t in &snapshot.tracks {
            let mut ts = TrackState::new(&t.id);
            ts.volume = t.volume.clamp(0.0, 2.0);
            ts.pan = t.pan.clamp(-1.0, 1.0);
            ts.muted = t.muted;
            ts.solo = t.solo;
            tracks.push(ts);
        }
        drop(tracks);

        // Store snapshot for future reference.
        if let Err(error) = self.sync_live_input_stream(&snapshot) {
            let message = format!("Live input unavailable: {error}");
            eprintln!("[SphereAudio] {message}");
            self.status.lock().last_error = Some(message);
        }
        *self.project.lock() = Some(snapshot.clone());
        *self.runtime.lock() = runtime.clone();

        for t in &snapshot.tracks {
            let track_clips = runtime.clips.iter().filter(|c| c.track_id == t.id).count();
            eprintln!(
                "[SphereAudio] track '{}' type={} clips={} volume={:.2} pan={:.2} muted={} solo={}",
                t.id, t.track_type, track_clips, t.volume, t.pan, t.muted, t.solo
            );
        }
        // Initialise the graveyard drop-thread on this (control) thread so the
        // callback's first graph swap is a cheap atomic enqueue, never a
        // channel allocation + thread spawn on the audio thread.
        crate::graveyard::prime();

        match self.send_command(EngineCommand::LoadProject(Box::new(runtime))) {
            Ok(()) => eprintln!("[SphereAudio] LoadProject command sent to audio callback"),
            Err(SphereAudioError::EngineNotOpen) => {
                eprintln!(
                    "[SphereAudio] ⚠ WARNING: no audio stream open — runtime stored, \
                     will apply on next openDevice/openDaux"
                );
                // No callback will run the graph-swap transition; leave a
                // consistent Paused state instead of LoadingProject.
                self.shared
                    .engine_state
                    .store(AudioEngineState::Paused as u8, Ordering::Relaxed);
                eprintln!(
                    "[AudioEngineState] old=LoadingProject new=Paused source=load_project_no_stream"
                );
            }
            Err(e) => {
                self.shared
                    .engine_state
                    .store(old_state as u8, Ordering::Relaxed);
                eprintln!(
                    "[AudioEngineState] old=LoadingProject new={old_state:?} source=load_project_send_failed"
                );
                return Err(e);
            }
        }

        let _ = self.set_tempo_map(snapshot.bpm, snapshot.tempo_points.clone());
        let _ = self.set_time_signature(snapshot.time_signature[0], snapshot.time_signature[1]);

        Ok(())
    }

    // ── Debug info ─────────────────────────────────────────────────────────

    pub fn get_debug_info(&self) -> JsEngineDebugInfo {
        let runtime = self.runtime.lock();
        let project = self.project.lock();
        let sample_rate = self.shared.sample_rate.load(Ordering::Relaxed).max(1);

        let ready_clips = runtime
            .clips
            .iter()
            .filter(|c| c.source.frames() > 0)
            .count() as u32;
        let clip_summaries: Vec<String> = runtime
            .clips
            .iter()
            .map(|c| {
                format!(
                    "id={} track={} start={:.3}s dur={:.3}s frames={} gain={:.2}",
                    c.id,
                    c.track_id,
                    c.start_sample as f64 / sample_rate as f64,
                    c.duration_samples as f64 / sample_rate as f64,
                    c.source.frames(),
                    c.gain,
                )
            })
            .collect();
        let insert_summaries: Vec<String> = runtime
            .tracks
            .iter()
            .flat_map(|track| {
                track.inserts.iter().map(move |insert| {
                    let format = insert
                        .params
                        .get("format")
                        .and_then(|value| value.as_str())
                        .unwrap_or("");
                    let path = insert
                        .params
                        .get("path")
                        .and_then(|value| value.as_str())
                        .unwrap_or("");
                    let vst3_ready = insert
                        .vst3
                        .as_ref()
                        .map(|processor| processor.is_ready())
                        .unwrap_or(false);
                    let (process_count, input_peak, output_peak, diff_peak) = insert
                        .vst3
                        .as_ref()
                        .map(|processor| {
                            (
                                processor.process_count(),
                                processor.last_input_peak(),
                                processor.last_output_peak(),
                                processor.last_difference_peak(),
                            )
                        })
                        .unwrap_or((0, 0.0, 0.0, 0.0));
                    format!(
                        concat!(
                            "track={} insert={} kind={} format={} enabled={}",
                            "vst3Ready={} processCount={}",
                            "inPeak={:.4} outPeak={:.4} diffPeak={:.6} path={}"
                        ),
                        track.id,
                        insert.id,
                        insert.kind,
                        format,
                        insert.enabled,
                        vst3_ready,
                        process_count,
                        input_peak,
                        output_peak,
                        diff_peak,
                        path,
                    )
                })
            })
            .collect();

        let graph_rejected_route_summaries: Vec<String> = runtime
            .audio_graph
            .rejected_routes
            .iter()
            .map(|route| {
                format!(
                    "{} -> {} ({:?}): {}",
                    route.from_track_id, route.to_track_id, route.kind, route.reason
                )
            })
            .collect();

        let transport = self.transport_snapshot();
        let disk = crate::streaming_source::diagnostics();

        JsEngineDebugInfo {
            project_id: project.as_ref().map(|p| p.project_id.clone()),
            loaded_tracks: runtime.tracks.len() as u32,
            loaded_clips: runtime.clips.len() as u32,
            ready_clips,
            is_playing: transport.playing,
            position_seconds: transport.position_seconds,
            position_beats: transport.position_beats,
            loop_enabled: transport.loop_enabled,
            has_solo: runtime.has_solo,
            clip_summaries,
            insert_summaries,
            disk_underruns: disk.underruns as f64,
            disk_stream_active_sources: disk.active_sources as f64,
            disk_stream_cache_reads: disk.cache_reads as f64,
            disk_stream_cache_hits: disk.cache_hits as f64,
            disk_stream_cache_misses: disk.cache_misses as f64,
            disk_stream_cache_memory_used_mb: disk.cache_memory_used_bytes as f64
                / (1024.0 * 1024.0),
            disk_stream_cache_memory_budget_mb: disk.cache_memory_budget_bytes as f64
                / (1024.0 * 1024.0),
            disk_stream_blocks_decoded: disk.blocks_decoded as f64,
            disk_stream_frames_decoded: disk.frames_decoded as f64,
            graph_node_count: runtime.audio_graph.nodes.len() as u32,
            graph_pass1_count: runtime.audio_graph.pass1_source_indices.len() as u32,
            graph_pass2_count: runtime.audio_graph.pass2_routing_indices.len() as u32,
            graph_rejected_route_count: runtime.audio_graph.rejected_routes.len() as u32,
            graph_rejected_route_summaries,
        }
    }

    /// Structured per-insert instantiation status for UI readback (Phase 2b).
    ///
    /// Built-in DSP inserts are always `ready`. Native-plugin inserts are
    /// `ready` only when their `Vst3RuntimeProcessor` instantiated and is live;
    /// a native insert with `ready == false` is a definitive instantiation
    /// failure (the worker attempted it during `load_project` and got `None`).
    /// Used by the native shell to flip `InsertLoadStatus::Failed`.
    pub fn insert_statuses(&self) -> Vec<crate::native::EngineInsertStatus> {
        let runtime = self.runtime.lock();
        runtime
            .tracks
            .iter()
            .flat_map(|track| {
                let track_id = track.id.clone();
                track.inserts.iter().map(move |insert| {
                    let native = insert.kind.eq_ignore_ascii_case("native-plugin");
                    let ready = if native {
                        insert.vst3.as_ref().map(|p| p.is_ready()).unwrap_or(false)
                    } else {
                        true
                    };
                    crate::native::EngineInsertStatus {
                        track_id: track_id.clone(),
                        insert_id: insert.id.clone(),
                        native,
                        ready,
                    }
                })
            })
            .collect()
    }

    // ── Recording API ──────────────────────────────────────────────────────

    /// Open an input stream and begin writing armed tracks to WAV files.
    pub fn start_recording(&self, config: JsStartRecordingConfig) -> Result<(), SphereAudioError> {
        let mut guard = self.recording.lock();
        if guard.is_some() {
            return Err(SphereAudioError::NativeError(
                "A recording session is already active".to_string(),
            ));
        }
        let monitor_mix = config.monitor_mix;
        // Single-capture-stream invariant: the recording stream becomes the sole
        // input device client during a take (it feeds the monitor ring, preview
        // ring, and file writer). Stop the standalone monitor stream so two
        // WASAPI shared clients don't contend on the same endpoint — a likely
        // source of jitter while recording.
        self.stop_live_input_stream();
        match recording::start_recording(config, Arc::clone(&self.shared), monitor_mix) {
            Ok(session) => {
                *guard = Some(session);
                drop(guard);
                // Ensure the output stream runs so monitoring is audible.
                self.warm_output_for_monitoring();
                Ok(())
            }
            Err(e) => {
                drop(guard);
                // Restore standalone monitoring from the last snapshot on failure.
                if let Some(snapshot) = self.project.lock().clone() {
                    let _ = self.sync_live_input_stream(&snapshot);
                }
                Err(e)
            }
        }
    }

    /// Stop the active recording, finalize WAV files, and return per-track results.
    pub fn stop_recording(&self) -> Result<Vec<JsRecordingResult>, SphereAudioError> {
        let session = self.recording.lock().take().ok_or_else(|| {
            SphereAudioError::NativeError("No active recording session".to_string())
        })?;
        let mut results = recording::stop_recording(session)?;
        let runtime = self.runtime.lock();
        let buffer_frames = self.status.lock().buffer_size;
        let sample_rate = runtime.sample_rate.max(1) as f64;
        let round_trip_buffer = buffer_frames.saturating_mul(2);
        for result in &mut results {
            let Some(track_idx) = runtime
                .tracks
                .iter()
                .position(|track| track.id == result.track_id)
            else {
                continue;
            };
            let path_samples = runtime
                .latency_graph
                .max_path_latency_samples
                .saturating_sub(
                    runtime
                        .latency_graph
                        .track_pdc_delay
                        .get(track_idx)
                        .copied()
                        .unwrap_or(0),
                );
            let offset_samples = round_trip_buffer.saturating_add(path_samples);
            let offset_beats = runtime
                .tempo_map
                .beat_at_samples(offset_samples as u64, sample_rate);
            result.start_beat = (result.start_beat - offset_beats).max(0.0);
        }
        drop(runtime);

        // Restore the standalone monitoring input stream from the last snapshot
        // (re-opens it if any track is still armed/monitored).
        if let Some(snapshot) = self.project.lock().clone() {
            let _ = self.sync_live_input_stream(&snapshot);
        }
        Ok(results)
    }

    /// Snapshot of current recording state (for UI status polling).
    pub fn get_recording_status(&self) -> JsRecordingStatus {
        match self.recording.lock().as_ref() {
            None => JsRecordingStatus::default(),
            Some(s) => JsRecordingStatus {
                active: true,
                duration_seconds: s.started_at.elapsed().as_secs_f64(),
                track_count: s.track_count as u32,
            },
        }
    }

    // ── DAUx backend API ───────────────────────────────────────────────────

    /// List all available DAUx backends on this platform.
    pub fn list_daux_backends(&self) -> Vec<JsDauxBackendInfo> {
        list_available_backends()
            .into_iter()
            .map(|b| JsDauxBackendInfo {
                id: b.id,
                name: b.name,
                available: b.available,
                is_default: b.is_default,
                description: b.description,
            })
            .collect()
    }

    /// Open (or re-open) a stream using the full DAUx config.
    /// This is the preferred method; `open_device()` is kept for backward compat.
    ///
    /// On failure the stream stays closed — the caller should call `open_daux`
    /// again with a safe fallback config.  Use `open_daux_safe` if you want the
    /// engine to handle the fallback automatically.
    pub fn open_daux(&self, config: JsDauxConfig) -> Result<(), SphereAudioError> {
        self.close_device_inner();

        let backend = BackendKind::from_id(&config.backend_id);
        let daux_cfg = DauxDeviceConfig {
            backend: backend.clone(),
            output_device_id: config.output_device_id.filter(|s| !s.is_empty()),
            input_device_id: None,
            sample_rate: config.sample_rate.filter(|&v| v > 0),
            buffer_size: config.buffer_size.filter(|&v| v > 0),
            mmcss_priority: config.mmcss_priority,
            safe_mode: config.safe_mode,
        };

        *self.daux_config.lock() = daux_cfg.clone();
        self.glitch_counter.store(0, Ordering::Relaxed);

        let initial_runtime = self.get_initial_runtime(None);

        let stream = match backend {
            #[cfg(target_os = "windows")]
            BackendKind::WasapiExclusive => {
                let handle = wasapi_exclusive::open(
                    &daux_cfg,
                    Arc::clone(&self.shared),
                    initial_runtime,
                    Arc::clone(&self.glitch_counter),
                )?;
                let sr = handle.sample_rate;
                let bs = handle.buffer_size;
                let dev_name = handle.device_name.clone();
                let stream = ActiveStream::WasapiExclusive(handle);
                self.commit_stream_open(sr, bs, dev_name, "DAUx WASAPI Exclusive".into());
                stream
            }
            _ => {
                let handle = cpal_backend::open(
                    &daux_cfg,
                    Arc::clone(&self.shared),
                    initial_runtime,
                    Arc::clone(&self.glitch_counter),
                )?;
                let sr = handle.sample_rate;
                let bs = handle.buffer_size;
                let dev_name = handle.device_name.clone();
                let backend_name = handle.backend_name.clone();
                let stream = ActiveStream::Cpal(handle);
                self.commit_stream_open(sr, bs, dev_name, backend_name);
                stream
            }
        };

        // Clear any previous error / device-lost flag on success.
        self.status.lock().last_daux_error = None;
        self.shared.device_lost.store(false, Ordering::Relaxed);
        *self.active_stream.lock() = Some(stream);
        Ok(())
    }

    /// Re-open the audio device after a device-loss event using the last-known
    /// good DAUx config. Returns `Ok(true)` if a recovery was performed,
    /// `Ok(false)` if no recovery was needed (device not lost). On failure the
    /// `device_lost` flag stays set so the UI keeps showing DeviceLost.
    pub fn recover_daux(&self) -> Result<bool, SphereAudioError> {
        if !self.shared.device_lost.load(Ordering::Relaxed) {
            return Ok(false);
        }
        let cfg = {
            let prev = self.daux_config.lock().clone();
            JsDauxConfig {
                backend_id: prev.backend.id().to_string(),
                output_device_id: prev.output_device_id.clone(),
                sample_rate: prev.sample_rate,
                buffer_size: prev.buffer_size,
                mmcss_priority: prev.mmcss_priority,
                safe_mode: prev.safe_mode,
            }
        };
        // `open_daux` clears `device_lost` on success.
        self.open_daux(cfg)?;
        Ok(true)
    }

    /// Whether applying `new` would require a controlled device restart versus
    /// the currently-open config. In this engine every device-shaping change
    /// (backend, device, sample rate, buffer size, MMCSS, safe mode) reopens the
    /// stream, so the UI should mark those settings "restart required".
    pub fn daux_requires_restart(&self, new: &JsDauxConfig) -> bool {
        let cur = self.daux_config.lock();
        let norm = |s: &Option<String>| s.clone().filter(|v| !v.is_empty());
        let norm_u32 = |v: Option<u32>| v.filter(|&n| n > 0);
        new.backend_id != cur.backend.id()
            || norm(&new.output_device_id) != cur.output_device_id
            || norm_u32(new.sample_rate) != cur.sample_rate
            || norm_u32(new.buffer_size) != cur.buffer_size
            || new.mmcss_priority != cur.mmcss_priority
            || new.safe_mode != cur.safe_mode
    }

    /// Safe variant: tries `new_config`, and on failure restores the previous
    /// working config.  Returns `Ok(())` if the new config succeeded, or
    /// `Err(message)` describing why exclusive failed (after restoring the
    /// previous backend).  The engine always ends up with an open stream.
    pub fn open_daux_safe(&self, new_config: JsDauxConfig) -> Result<(), SphereAudioError> {
        // Save the previous working config before closing.
        let previous_config = {
            let prev = self.daux_config.lock().clone();
            JsDauxConfig {
                backend_id: prev.backend.id().to_string(),
                output_device_id: prev.output_device_id.clone(),
                sample_rate: prev.sample_rate,
                buffer_size: prev.buffer_size,
                mmcss_priority: prev.mmcss_priority,
                safe_mode: prev.safe_mode,
            }
        };
        let had_previous_stream = self.active_stream.lock().is_some();

        // Stop transport so playback doesn't resume mid-switch.
        self.shared.playing.store(false, Ordering::Relaxed);

        match self.open_daux(new_config) {
            Ok(()) => Ok(()),
            Err(open_err) => {
                let err_msg = open_err.to_string();
                eprintln!("[DAUx] open_daux_safe: failed ({err_msg}), attempting fallback");

                // Store the error before trying to restore.
                self.status.lock().last_daux_error = Some(err_msg.clone());

                // Attempt to restore the previous working config.
                if had_previous_stream {
                    match self.open_daux(previous_config) {
                        Ok(()) => {
                            eprintln!("[DAUx] open_daux_safe: previous backend restored");
                            let restore_msg = format!(
                                "Exclusive mode failed: {err_msg}. Reverted to previous backend."
                            );
                            self.status.lock().last_daux_error = Some(restore_msg.clone());
                            Err(SphereAudioError::StreamOpenFailed(restore_msg))
                        }
                        Err(restore_err) => {
                            eprintln!("[DAUx] open_daux_safe: fallback also failed: {restore_err}");
                            let combined = format!(
                                "Exclusive failed: {err_msg}. Fallback also failed: {restore_err}"
                            );
                            self.status.lock().last_daux_error = Some(combined.clone());
                            Err(SphereAudioError::StreamOpenFailed(combined))
                        }
                    }
                } else {
                    Err(open_err)
                }
            }
        }
    }

    fn sync_live_input_stream(
        &self,
        snapshot: &EngineProjectSnapshot,
    ) -> Result<(), SphereAudioError> {
        // The first armed/monitored audio track with a routable input source
        // drives which device + channels the shared capture stream uses.
        let desired_track = snapshot.tracks.iter().find(|track| {
            track.track_type == "audio"
                && (track.armed || track.input_monitor)
                && !track.input_source.channels.is_empty()
        });

        let wants_live_input = desired_track.is_some();

        // Any track that explicitly wants monitoring (not just record-arm) —
        // gates whether the render callback mixes the input bus to the output.
        let monitor_any = snapshot.tracks.iter().any(|track| {
            track.track_type == "audio"
                && track.input_monitor
                && !track.input_source.channels.is_empty()
        });
        self.shared
            .monitor_enabled_any
            .store(monitor_any, Ordering::Relaxed);

        if !wants_live_input {
            self.stop_live_input_stream();
            return Ok(());
        }

        // Device resolution order: the track's own pinned device, then the
        // global Preferences input device, then the system default.
        let desired_device = desired_track
            .and_then(|track| track.input_source.device_id.clone())
            .filter(|device_id| !device_id.trim().is_empty())
            .or_else(|| {
                snapshot
                    .preferred_input_device
                    .clone()
                    .filter(|d| !d.trim().is_empty())
            });

        // Which device channels feed the L/R input bus.
        let (mon_l_ch, mon_r_ch) = desired_track
            .map(|track| {
                let ch = &track.input_source.channels;
                let l = ch.first().copied().unwrap_or(0);
                let r = ch.get(1).copied().unwrap_or(l);
                (l as usize, r as usize)
            })
            .unwrap_or((0, 1));

        eprintln!(
            "[Engine] applied input device = {:?} (preferred={:?}) monitor_any={}",
            desired_device, snapshot.preferred_input_device, monitor_any
        );

        // Reuse the existing stream when the device is unchanged — only the
        // monitor flag/channels may have moved (already stored above).
        if self
            .live_input
            .lock()
            .as_ref()
            .is_some_and(|handle| handle.device_id == desired_device)
        {
            self.shared.live_input_active.store(true, Ordering::Relaxed);
            self.warm_output_for_monitoring();
            return Ok(());
        }

        self.stop_live_input_stream();
        let device = crate::recording::find_input_device(desired_device.as_deref())?;
        let default_cfg = device.default_input_config().map_err(|e| {
            SphereAudioError::NativeError(format!("Input device config error: {e}"))
        })?;
        let stream_config = cpal::StreamConfig {
            channels: default_cfg.channels(),
            sample_rate: default_cfg.sample_rate(),
            buffer_size: cpal::BufferSize::Default,
        };
        let channel_count = stream_config.channels.max(1) as usize;
        let input_sr = stream_config.sample_rate.0;
        let device_label = device
            .name()
            .unwrap_or_else(|_| desired_device.clone().unwrap_or_else(|| "default".into()));

        eprintln!(
            "[AudioDevice] opening input stream: device='{device_label}' sample_rate={input_sr} \
             channels={channel_count} buffer=default monitor_channels=({mon_l_ch},{mon_r_ch})"
        );
        eprintln!(
            "[WASAPI] input sample_rate={input_sr} channels={channel_count} format={:?}",
            default_cfg.sample_format()
        );
        let output_sr = self.shared.sample_rate.load(Ordering::Relaxed);
        if output_sr != 0 && input_sr != 0 && output_sr != input_sr {
            eprintln!(
                "[WASAPI] ⚠ input sample_rate ({input_sr}) != output sample_rate ({output_sr}); \
                 monitored audio will be pitch-shifted until resampling is added (Layer 9 TODO)"
            );
        }

        // Reset and arm the input ring for this stream's format.
        self.shared
            .input_ring
            .set_active(true, channel_count as u32, input_sr);

        let shared = Arc::clone(&self.shared);
        let stream = device
            .build_input_stream::<f32, _, _>(
                &stream_config,
                move |data: &[f32], _info| {
                    let frames = data.len() / channel_count.max(1);
                    shared.input_cb_count.fetch_add(1, Ordering::Relaxed);
                    shared
                        .input_frames_received
                        .fetch_add(frames as u64, Ordering::Relaxed);
                    let mut peak_l = 0.0f32;
                    let mut peak_r = 0.0f32;
                    let mut last_l = 0.0f32;
                    let mut last_r = 0.0f32;
                    for frame in data.chunks(channel_count) {
                        // Pick the configured monitor channels; fall back to the
                        // first sample when a channel index is out of range.
                        let first = frame.first().copied().unwrap_or(0.0);
                        let l = frame
                            .get(mon_l_ch)
                            .copied()
                            .unwrap_or(first)
                            .clamp(-1.0, 1.0);
                        let r = frame.get(mon_r_ch).copied().unwrap_or(l).clamp(-1.0, 1.0);
                        last_l = l;
                        last_r = r;
                        peak_l = peak_l.max(l.abs());
                        peak_r = peak_r.max(r.abs());
                        // Layer 4: publish the actual samples to the output side.
                        shared.input_ring.write_stereo(l, r);
                    }
                    shared
                        .live_input_l
                        .store(f32_store(last_l), Ordering::Relaxed);
                    shared
                        .live_input_r
                        .store(f32_store(last_r), Ordering::Relaxed);
                    atomic_max_f32_bits(&shared.live_input_peak_l, peak_l);
                    atomic_max_f32_bits(&shared.live_input_peak_r, peak_r);
                    shared.live_input_active.store(true, Ordering::Relaxed);
                },
                |err| eprintln!("[SphereAudio] Live input stream error: {err}"),
                None,
            )
            .map_err(|e| {
                self.shared.input_ring.set_active(false, 0, 0);
                SphereAudioError::NativeError(format!("Cannot open live input stream: {e}"))
            })?;
        stream
            .play()
            .map_err(|e| SphereAudioError::NativeError(format!("Live input play failed: {e}")))?;
        eprintln!("[AudioDevice] input stream started: device='{device_label}'");

        *self.live_input.lock() = Some(LiveInputHandle {
            _stream: stream,
            device_id: desired_device,
        });
        self.shared.live_input_active.store(true, Ordering::Relaxed);

        // Make sure the output render callback is actually running so per-track
        // input meters and the monitor mix update even before transport play.
        self.warm_output_for_monitoring();
        Ok(())
    }

    /// Resume the (already-open) output stream so `fill_output_f32` runs while
    /// monitoring/armed, without advancing the transport. No-op when no stream
    /// is open yet — the next explicit `start()` will pick it up.
    fn warm_output_for_monitoring(&self) {
        let already_running = self.status.lock().running;
        if already_running {
            return;
        }
        match self.start() {
            Ok(()) => eprintln!("[AudioDevice] output stream warmed for monitoring"),
            Err(SphereAudioError::EngineNotOpen) => {}
            Err(e) => eprintln!("[AudioDevice] could not warm output for monitoring: {e}"),
        }
    }

    fn stop_live_input_stream(&self) {
        *self.live_input.lock() = None;
        self.shared
            .live_input_active
            .store(false, Ordering::Relaxed);
        self.shared.input_ring.set_active(false, 0, 0);
        self.shared
            .monitor_enabled_any
            .store(false, Ordering::Relaxed);
        self.shared
            .live_input_l
            .store(f32_store(0.0), Ordering::Relaxed);
        self.shared
            .live_input_r
            .store(f32_store(0.0), Ordering::Relaxed);
        self.shared
            .live_input_peak_l
            .store(f32_store(0.0), Ordering::Relaxed);
        self.shared
            .live_input_peak_r
            .store(f32_store(0.0), Ordering::Relaxed);
        self.shared
            .input_bus_peak_l
            .store(f32_store(0.0), Ordering::Relaxed);
        self.shared
            .input_bus_peak_r
            .store(f32_store(0.0), Ordering::Relaxed);
    }

    /// Start an input-level test on `device_id` (or the default input). Opens a
    /// capture stream whose callback tracks the running peak; poll it with
    /// [`Self::get_input_test_level`] and stop with [`Self::stop_input_test`].
    /// Independent of recording and of the output device.
    pub fn start_input_test(&self, device_id: Option<String>) -> Result<(), SphereAudioError> {
        // Replace any existing test stream.
        *self.input_test.lock() = None;

        let device = crate::recording::find_input_device(device_id.as_deref())?;
        let default_cfg = device.default_input_config().map_err(|e| {
            SphereAudioError::NativeError(format!("Input device config error: {e}"))
        })?;
        let stream_config = cpal::StreamConfig {
            channels: default_cfg.channels(),
            sample_rate: default_cfg.sample_rate(),
            buffer_size: cpal::BufferSize::Default,
        };

        let peak = Arc::new(AtomicU32::new(0));
        let cb_peak = Arc::clone(&peak);
        let stream = device
            .build_input_stream::<f32, _, _>(
                &stream_config,
                move |data: &[f32], _info| {
                    let mut block_peak = 0.0f32;
                    for &s in data {
                        let a = s.abs();
                        if a > block_peak {
                            block_peak = a;
                        }
                    }
                    // Keep the running max until the UI polls (swap-resets it).
                    let mut cur = cb_peak.load(Ordering::Relaxed);
                    loop {
                        if block_peak <= f32::from_bits(cur) {
                            break;
                        }
                        match cb_peak.compare_exchange_weak(
                            cur,
                            block_peak.to_bits(),
                            Ordering::Relaxed,
                            Ordering::Relaxed,
                        ) {
                            Ok(_) => break,
                            Err(c) => cur = c,
                        }
                    }
                },
                |err| eprintln!("[SphereAudio] Input test stream error: {err}"),
                None,
            )
            .map_err(|e| {
                SphereAudioError::NativeError(format!("Cannot open input test stream: {e}"))
            })?;
        stream
            .play()
            .map_err(|e| SphereAudioError::NativeError(format!("Input test play failed: {e}")))?;

        *self.input_test.lock() = Some(InputTestHandle {
            _stream: stream,
            peak,
        });
        Ok(())
    }

    /// Read (and reset) the peak input level since the last poll, `0.0..=1.0`.
    /// Returns `0.0` when no input test is active.
    pub fn get_input_test_level(&self) -> f32 {
        self.input_test
            .lock()
            .as_ref()
            .map(|h| f32::from_bits(h.peak.swap(0, Ordering::Relaxed)))
            .unwrap_or(0.0)
    }

    /// Stop and release the input-level test stream.
    pub fn stop_input_test(&self) {
        *self.input_test.lock() = None;
    }

    /// Aggregate latency report (Phase V — reporting only).
    ///
    /// Sums each track's enabled native-plugin insert latencies (queried from
    /// the live VST3 processors on the control-side runtime copy) plus the
    /// device buffer latency. Full plug-in delay compensation is Phase W; this
    /// is the data the UI displays and the basis (`max_track_samples`) PDC will
    /// later use.
    pub fn get_latency_info(&self) -> crate::types::JsLatencyInfo {
        use crate::latency_graph::strip_plugin_latency_samples;
        use crate::types::{JsLatencyInfo, JsTrackLatency};
        let runtime = self.runtime.lock();
        let (sample_rate, buffer_frames) = {
            let st = self.status.lock();
            (st.sample_rate.max(1), st.buffer_size)
        };
        let to_ms = |samples: u32| samples as f64 / sample_rate as f64 * 1000.0;
        let pdc_enabled = self.pdc_enabled();

        let max_path_samples = runtime.latency_graph.max_path_latency_samples;
        let mut tracks = Vec::new();
        let mut master_samples = 0u32;
        let mut max_track_plugin_samples = 0u32;
        for (idx, track) in runtime.tracks.iter().enumerate() {
            let mut samples: i64 = 0;
            for insert in &track.inserts {
                if !insert.enabled {
                    continue;
                }
                if let Some(vst3) = insert.vst3.as_ref() {
                    if vst3.is_ready() {
                        samples += vst3.get_latency_samples().max(0) as i64;
                    }
                }
            }
            let plugin_samples = if samples > 0 {
                samples as u32
            } else {
                strip_plugin_latency_samples(track)
            };
            let path_samples = max_path_samples.saturating_sub(
                runtime
                    .latency_graph
                    .track_pdc_delay
                    .get(idx)
                    .copied()
                    .unwrap_or(0),
            );
            let pdc_delay_samples = runtime
                .latency_graph
                .track_pdc_delay
                .get(idx)
                .copied()
                .unwrap_or(0);
            if track.track_type == "master" {
                master_samples = plugin_samples;
            } else {
                max_track_plugin_samples = max_track_plugin_samples.max(plugin_samples);
                tracks.push(JsTrackLatency {
                    track_id: track.id.clone(),
                    plugin_samples,
                    plugin_ms: to_ms(plugin_samples),
                    path_samples,
                    path_ms: to_ms(path_samples),
                    pdc_delay_samples,
                    pdc_delay_ms: to_ms(pdc_delay_samples),
                });
            }
        }

        JsLatencyInfo {
            sample_rate,
            buffer_frames,
            buffer_ms: to_ms(buffer_frames),
            tracks,
            master_samples,
            master_ms: to_ms(master_samples),
            max_path_samples,
            max_path_ms: to_ms(max_path_samples),
            pdc_enabled,
            max_track_samples: max_track_plugin_samples,
        }
    }

    /// Return the current DAUx status (backend, device, latency, glitches).
    pub fn get_daux_status(&self) -> JsDauxStatus {
        let st = self.status.lock().clone();
        let daux_cfg = self.daux_config.lock().clone();
        let glitch_count = self.glitch_counter.load(Ordering::Relaxed) as f64;
        let mmcss_active = self.shared.mmcss_active.load(Ordering::Relaxed);
        let device_lost = self.shared.device_lost.load(Ordering::Relaxed);
        let running = self.shared.playing.load(Ordering::Relaxed);
        let device_state = if device_lost {
            "DeviceLost"
        } else if !st.stream_open {
            "Closed"
        } else if running {
            "Running"
        } else {
            "Ready"
        }
        .to_string();

        let backend_id = daux_cfg.backend.id().to_string();
        let backend_name = if let Some(stream) = self.active_stream.lock().as_ref() {
            stream.backend_name().to_string()
        } else {
            daux_cfg.backend.display_name().to_string()
        };

        let estimated_latency_ms = if st.sample_rate > 0 {
            st.buffer_size as f64 / st.sample_rate as f64 * 1000.0
        } else {
            0.0
        };

        JsDauxStatus {
            backend_id,
            backend_name,
            output_device: st.output_device,
            sample_rate: st.sample_rate,
            buffer_size: st.buffer_size,
            estimated_latency_ms,
            glitch_count,
            mmcss_active,
            last_error: st.last_daux_error.clone(),
            device_lost,
            device_state,
        }
    }

    // ── Internal helpers ───────────────────────────────────────────────────

    fn get_initial_runtime(&self, sample_rate_override: Option<u32>) -> RuntimeProject {
        let sr = sample_rate_override
            .unwrap_or_else(|| self.shared.sample_rate.load(Ordering::Relaxed).max(44100));

        // Prefer the live runtime built by `load_project`. Rebuilding from the
        // snapshot here re-decodes media on the caller thread and has frozen
        // the native UI when Play re-opened the device after a sync.
        {
            let cached = self.runtime.lock().clone();
            if !cached.tracks.is_empty() {
                let mut runtime = cached;
                runtime.sample_rate = sr;
                return runtime;
            }
        }

        self.project
            .lock()
            .as_ref()
            .map(|snapshot| {
                let mut audio_cache = self.audio_cache.lock();
                match RuntimeProject::build(snapshot, sr, &mut audio_cache, None, self.pdc_enabled()) {
                    Ok(runtime) => runtime,
                    Err(e) => {
                        eprintln!(
                            "[SphereAudio] get_initial_runtime: invalid routing graph ({e}), keeping previous runtime"
                        );
                        let mut runtime = self.runtime.lock().clone();
                        runtime.sample_rate = sr;
                        runtime
                    }
                }
            })
            .unwrap_or_else(|| {
                let mut runtime = self.runtime.lock().clone();
                runtime.sample_rate = sr;
                runtime
            })
    }

    fn commit_stream_open(&self, sr: u32, bs: u32, device_name: String, backend_name: String) {
        self.shared.sample_rate.store(sr, Ordering::Relaxed);
        let mut st = self.status.lock();
        st.stream_open = true;
        st.running = false;
        st.sample_rate = sr;
        st.buffer_size = bs;
        st.output_device = Some(device_name);
        st.last_error = None;
        eprintln!("[DAUx] Stream committed: backend={backend_name} sr={sr} buf={bs}");
    }

    fn cmd_sender(&self) -> Option<Sender<EngineCommand>> {
        if let Some(stream) = self.active_stream.lock().as_ref() {
            if let Some(tx) = stream.cmd_tx() {
                return Some(tx.clone());
            }
        }
        self.cmd_tx.lock().as_ref().cloned()
    }

    fn send_command(&self, cmd: EngineCommand) -> Result<(), SphereAudioError> {
        if transport_freeze_debug_enabled() {
            let label = match &cmd {
                EngineCommand::LoadProject(_) => "LoadProject",
                EngineCommand::SetTestTone { .. } => "SetTestTone",
                EngineCommand::SetMasterVolume { .. } => "SetMasterVolume",
                EngineCommand::SetTrackVolume { .. } => "SetTrackVolume",
                EngineCommand::SetTrackPan { .. } => "SetTrackPan",
                EngineCommand::SetTrackMute { .. } => "SetTrackMute",
                EngineCommand::SetTrackSolo { .. } => "SetTrackSolo",
                EngineCommand::SetTrackPreviewMode { .. } => "SetTrackPreviewMode",
                EngineCommand::SetInsertParam { .. } => "SetInsertParam",
                EngineCommand::MidiPreviewNoteOn { .. } => "MidiPreviewNoteOn",
                EngineCommand::MidiPreviewNoteOff { .. } => "MidiPreviewNoteOff",
                EngineCommand::MidiPreviewAllNotesOff { .. } => "MidiPreviewAllNotesOff",
                EngineCommand::PluginPreviewNoteOn { .. } => "PluginPreviewNoteOn",
                EngineCommand::PluginPreviewNoteOff { .. } => "PluginPreviewNoteOff",
                EngineCommand::PluginPreviewAllNotesOff { .. } => "PluginPreviewAllNotesOff",
                EngineCommand::StartTransport => "StartTransport",
                EngineCommand::StopTransport => "StopTransport",
                EngineCommand::Seek { .. } => "Seek",
                EngineCommand::SetMetronomeEnabled(_) => "SetMetronomeEnabled",
                EngineCommand::SetBpm(_) => "SetBpm",
                EngineCommand::SetTempoMap(_) => "SetTempoMap",
                EngineCommand::SetTimeSignature(_, _) => "SetTimeSignature",
                EngineCommand::SetTimeSignatureMap(_) => "SetTimeSignatureMap",
                EngineCommand::SetLoop { .. } => "SetLoop",
                EngineCommand::SetPluginBridgeSink { .. } => "SetPluginBridgeSink",
                EngineCommand::SetBridgeEditorActive { .. } => "SetBridgeEditorActive",
            };
            eprintln!("[play-debug engine] send_command {label}");
        }
        let tx = self.cmd_sender().ok_or(SphereAudioError::EngineNotOpen)?;
        tx.try_send(cmd)
            .map_err(|e| SphereAudioError::NativeError(e.to_string()))
    }
}

fn transport_freeze_debug_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("FUTUREBOARD_TRANSPORT_FREEZE_DEBUG").is_some())
}

// ── Audio callback builder ────────────────────────────────────────────────────

fn output_stream_candidates(
    device: &cpal::Device,
    requested: &JsDeviceOpenConfig,
) -> Result<Vec<OutputStreamCandidate>, String> {
    let default_supported = device.default_output_config().map_err(|e| e.to_string())?;
    let default_config = default_supported.config();
    let sample_format = default_supported.sample_format();

    let mut candidates = Vec::new();

    if requested.sample_rate.is_some() || requested.buffer_size.is_some() {
        let mut requested_config = default_config.clone();

        if let Some(sample_rate) = requested.sample_rate {
            requested_config.sample_rate = cpal::SampleRate(sample_rate);
        }
        if let Some(buffer_size) = requested.buffer_size {
            requested_config.buffer_size = BufferSize::Fixed(buffer_size);
        }

        candidates.push(OutputStreamCandidate {
            config: requested_config,
            sample_format,
            label: "requested",
        });
    }

    candidates.push(OutputStreamCandidate {
        config: default_config,
        sample_format,
        label: "default",
    });

    Ok(candidates)
}

fn reported_buffer_size(config: &cpal::StreamConfig) -> u32 {
    match config.buffer_size {
        BufferSize::Fixed(frames) => frames,
        BufferSize::Default => 0,
    }
}

#[inline]
pub fn render_project_sample(
    runtime: &mut RuntimeProject,
    project_sample: u64,
    master_volume: f32,
) -> (f32, f32) {
    let mut out_l = 0.0f32;
    let mut out_r = 0.0f32;
    let master_index = runtime.tracks.iter().position(|t| t.track_type == "master");
    let beat = sample_to_beat(runtime, project_sample);

    for clip_index in 0..runtime.clips.len() {
        let clip = &runtime.clips[clip_index];
        if clip.muted {
            continue;
        }
        let clip_start_sample = clip.start_sample;
        let clip_duration_samples = clip.duration_samples;
        if project_sample < clip_start_sample {
            continue;
        }
        let rel = project_sample - clip_start_sample;
        if rel >= clip_duration_samples {
            continue;
        }

        let clip_track_id = clip.track_id.clone();
        let clip_offset_seconds = clip.offset_seconds;
        let clip_speed_ratio = clip.speed_ratio;
        let clip_gain = clip.gain;
        let clip_fade_in = clip.fade_in_samples;
        let clip_fade_out = clip.fade_out_samples;
        let source = Arc::clone(&clip.source);

        let Some(track_index) = runtime.tracks.iter().position(|t| t.id == clip_track_id) else {
            continue;
        };
        if Some(track_index) == master_index {
            continue;
        }
        let has_solo = runtime.has_solo;
        if effective_track_muted(&runtime.tracks[track_index], beat)
            || (has_solo && !runtime.tracks[track_index].solo)
        {
            continue;
        }

        let source_pos_seconds = clip_offset_seconds
            + (rel as f64 / runtime.sample_rate.max(1) as f64) * clip_speed_ratio as f64;
        let source_pos = source_pos_seconds * source.sample_rate() as f64;
        let (mut l, mut r) = sample_source_stereo(&source, source_pos);
        if l == 0.0 && r == 0.0 {
            continue;
        }

        let fade = clip_fade_gain(rel, clip_duration_samples, clip_fade_in, clip_fade_out);
        let g = clip_gain * fade;
        l *= g;
        r *= g;

        let output_track_id = runtime.tracks[track_index].output_track_id.clone();
        let sends = runtime.tracks[track_index].sends.clone();
        let (track_l, track_r) =
            apply_track_chain_at_beat(l, r, &mut runtime.tracks[track_index], beat);
        let (track_l, track_r) =
            apply_preview_mode(track_l, track_r, runtime.tracks[track_index].preview_mode);
        runtime.accumulate_track_meter(track_index, track_l, track_r);

        if let Some(target_id) = output_track_id
            .as_deref()
            .filter(|id| !is_master_output(id))
        {
            if let Some(target_index) = runtime.tracks.iter().position(|t| t.id == target_id) {
                let (bus_l, bus_r) = apply_track_chain_at_beat(
                    track_l,
                    track_r,
                    &mut runtime.tracks[target_index],
                    beat,
                );
                let (bus_l, bus_r) =
                    apply_preview_mode(bus_l, bus_r, runtime.tracks[target_index].preview_mode);
                runtime.accumulate_track_meter(target_index, bus_l, bus_r);
                out_l += bus_l;
                out_r += bus_r;
            } else {
                out_l += track_l;
                out_r += track_r;
            }
        } else {
            out_l += track_l;
            out_r += track_r;
        }

        for send in sends {
            if !send.enabled || send.level <= 0.0 {
                continue;
            }
            let Some(return_track_index) = runtime
                .tracks
                .iter()
                .position(|t| t.id == send.return_track_id)
            else {
                continue;
            };
            let return_track = &runtime.tracks[return_track_index];
            if effective_track_muted(return_track, beat) || (runtime.has_solo && !return_track.solo)
            {
                continue;
            }
            let (send_l, send_r) = apply_track_chain_at_beat(
                track_l * send.level,
                track_r * send.level,
                &mut runtime.tracks[return_track_index],
                beat,
            );
            let (send_l, send_r) = apply_preview_mode(
                send_l,
                send_r,
                runtime.tracks[return_track_index].preview_mode,
            );
            runtime.accumulate_track_meter(return_track_index, send_l, send_r);
            out_l += send_l;
            out_r += send_r;
        }
    }

    // ── Master bus: apply master track inserts on the summed output ──
    if let Some(m_idx) = master_index {
        let muted = effective_track_muted(&runtime.tracks[m_idx], beat)
            || (runtime.has_solo && !runtime.tracks[m_idx].solo);
        if !muted {
            let master = &mut runtime.tracks[m_idx];
            for insert in &mut master.inserts {
                let (l, r) = apply_insert(out_l, out_r, insert);
                out_l = l;
                out_r = r;
            }
            let (l, r) = apply_preview_mode(out_l, out_r, master.preview_mode);
            out_l = l;
            out_r = r;
            runtime.accumulate_track_meter(m_idx, out_l, out_r);
        }
    }

    (
        crate::dsp::gain::soft_limit(out_l * master_volume),
        crate::dsp::gain::soft_limit(out_r * master_volume),
    )
}

/// Routing track kinds (Phase 3): receive sends rather than hosting clips.
#[inline]
fn is_routing_type(track_type: &str) -> bool {
    is_routing_track_type(track_type)
}

/// Two distinct mutable elements of a slice without allocation. Panics in
/// debug if `a == b`; callers guarantee distinct indices.
#[inline]
fn two_mut<T>(v: &mut [T], a: usize, b: usize) -> (&mut T, &mut T) {
    debug_assert!(a != b);
    if a < b {
        let (lo, hi) = v.split_at_mut(b);
        (&mut lo[a], &mut hi[0])
    } else {
        let (lo, hi) = v.split_at_mut(a);
        (&mut hi[0], &mut lo[b])
    }
}

#[inline]
fn tempo_map_from_project_snapshot(project: &EngineProjectSnapshot) -> TempoMap {
    if project.tempo_points.is_empty() {
        TempoMap::static_tempo(project.bpm)
    } else {
        TempoMap::from_points(
            project.bpm,
            project
                .tempo_points
                .iter()
                .map(|p| TempoPoint {
                    beat: p.beat,
                    bpm: p.bpm,
                })
                .collect(),
        )
    }
}

fn sample_to_beat(runtime: &RuntimeProject, sample: u64) -> f64 {
    runtime
        .tempo_map
        .beat_at_samples(sample, runtime.sample_rate.max(1) as f64)
}

/// Linear clip-fade gain for a sample at offset `rel` from the clip start.
///
/// `1.0` outside both fade regions; ramps `0→1` across the fade-in and `1→0`
/// across the fade-out. Linear is the current placeholder shape — the snapshot
/// carries per-fade curve names (`audio-system-plan.md` §6) which a later slice
/// can map to equal-power / exponential shaping here. Allocation-free.
#[inline]
fn clip_fade_gain(rel: u64, duration: u64, fade_in: u64, fade_out: u64) -> f32 {
    let mut gain = 1.0f32;
    if fade_in > 0 && rel < fade_in {
        gain *= rel as f32 / fade_in as f32;
    }
    if fade_out > 0 {
        let fade_out_start = duration.saturating_sub(fade_out);
        if rel >= fade_out_start {
            let into = (rel - fade_out_start) as f32;
            gain *= (1.0 - into / fade_out as f32).max(0.0);
        }
    }
    gain
}

#[inline]
fn effective_track_muted(track: &RuntimeTrack, beat: f64) -> bool {
    track
        .automation_values_at_beat(beat)
        .muted
        .unwrap_or(track.muted)
}

/// Apply a track's fader (volume / pan / preview mode) to its `block_*`
/// (which already holds the post-insert signal), write the post-fader result
/// back into `block_*`, and accumulate the track meter. Does **not** sum to any
/// destination — routing is done separately by [`route_main_output`]. No
/// allocation.
#[inline]
fn apply_fader(track: &mut RuntimeTrack, frames: usize, beat: f64) {
    let automation = track.automation_values_at_beat(beat);
    let volume = automation.volume.unwrap_or(track.volume);
    let pan = automation.pan.unwrap_or(track.pan);
    let (pan_l, pan_r) = pan_gains(pan);
    for frame_idx in 0..frames {
        let (l, r) = apply_preview_mode(
            track.block_l[frame_idx] * volume * pan_l,
            track.block_r[frame_idx] * volume * pan_r,
            track.preview_mode,
        );
        track.block_l[frame_idx] = l;
        track.block_r[frame_idx] = r;
    }
}

#[inline]
fn accumulate_block_meter(track: &mut RuntimeTrack, frames: usize) {
    for frame_idx in 0..frames {
        let l = track.block_l[frame_idx];
        let r = track.block_r[frame_idx];
        track.meter_peak_l = track.meter_peak_l.max(l.abs());
        track.meter_peak_r = track.meter_peak_r.max(r.abs());
        track.meter_sum_sq_l += l * l;
        track.meter_sum_sq_r += r * r;
    }
}

/// Sum a track's post-fader `block_*` into its output destination.
///
/// If `output_track_id` resolves to a routing track (bus/group/return) the
/// full post-fader signal is added to that track's receive buffer (`recv_*`),
/// so it is processed in Pass 2; otherwise it sums into the interleaved master
/// output. Cycle-safe like [`accumulate_sends`]: routing to self, to a
/// non-routing track, or backward between routing tracks falls back to master.
/// No allocation.
#[inline]
fn route_main_output(
    runtime: &mut RuntimeProject,
    src_index: usize,
    frames: usize,
    output: &mut [f32],
    channels: usize,
) {
    let target = match runtime.tracks[src_index].output_track_id.as_deref() {
        Some(id) if !is_master_output(id) => runtime.tracks.iter().position(|t| t.id == id),
        _ => None,
    };

    if let Some(t) = target {
        let src_routing = is_routing_type(&runtime.tracks[src_index].track_type);
        let accept = t != src_index
            && is_routing_type(&runtime.tracks[t].track_type)
            && (!src_routing || t > src_index);
        if accept {
            let (src, tgt) = two_mut(&mut runtime.tracks, src_index, t);
            for f in 0..frames {
                tgt.recv_l[f] += src.block_l[f];
                tgt.recv_r[f] += src.block_r[f];
            }
            return;
        }
    }

    // Default / fallback: sum into the master output.
    let track = &runtime.tracks[src_index];
    for f in 0..frames {
        let out = &mut output[f * channels..f * channels + channels];
        out[0] += track.block_l[f];
        out[1] += track.block_r[f];
    }
}

#[allow(clippy::too_many_arguments)]
fn process_track_block(
    runtime: &mut RuntimeProject,
    track_index: usize,
    frames: usize,
    output: &mut [f32],
    channels: usize,
    beat: f64,
    transport: RuntimeTransportContext,
) {
    apply_track_chain_block(
        &mut runtime.tracks[track_index],
        frames,
        &runtime.plugin_bridge_sinks,
        transport,
    );
    // Pre-fader sends tap the post-insert signal currently in block_*.
    accumulate_sends(runtime, track_index, frames, true);
    apply_fader(&mut runtime.tracks[track_index], frames, beat);
    let pdc_delay = runtime
        .latency_graph
        .track_pdc_delay
        .get(track_index)
        .copied()
        .unwrap_or(0);
    if pdc_delay > 0 {
        let track = &mut runtime.tracks[track_index];
        apply_pdc_delay_block(
            &mut track.block_l[..frames],
            &mut track.block_r[..frames],
            &mut track.pdc_delay_l,
            &mut track.pdc_delay_r,
            &mut track.pdc_write_pos,
            pdc_delay,
            frames,
        );
    }
    accumulate_block_meter(&mut runtime.tracks[track_index], frames);
    // Post-fader sends tap the post-fader (and PDC-aligned) signal in block_*.
    accumulate_sends(runtime, track_index, frames, false);
    // Route the post-fader signal to master or the track's output bus.
    route_main_output(runtime, track_index, frames, output, channels);
}

/// Add the source track's block (`block_*`, holding either the post-insert or
/// post-fader signal depending on `pre_fader`) into each accepted send target's
/// receive buffer (`recv_*`), scaled by the send level. Only sends whose
/// `pre_fader` flag matches the requested phase are routed.
///
/// Cycle-safe by construction: a send is accepted only when the target is a
/// routing track (bus/return); a *routing* source may additionally only target
/// a *later* routing track in array order. Sends to non-routing tracks, to
/// self, or backward between routing tracks are dropped (logged at build time
/// under `FUTUREBOARD_ROUTING_DEBUG`). No allocation on the audio thread.
#[inline]
fn accumulate_sends(
    runtime: &mut RuntimeProject,
    src_index: usize,
    frames: usize,
    pre_fader: bool,
) {
    let send_count = runtime.tracks[src_index].sends.len();
    if send_count == 0 {
        return;
    }
    let src_routing = is_routing_type(&runtime.tracks[src_index].track_type);
    for s in 0..send_count {
        let (enabled, level) = {
            let send = &runtime.tracks[src_index].sends[s];
            if send.pre_fader != pre_fader {
                continue;
            }
            (send.enabled, send.level)
        };
        if !enabled || level == 0.0 {
            continue;
        }
        let target_index = {
            let target_id = &runtime.tracks[src_index].sends[s].return_track_id;
            runtime.tracks.iter().position(|t| &t.id == target_id)
        };
        let Some(t) = target_index else {
            continue;
        };
        if t == src_index || !is_routing_type(&runtime.tracks[t].track_type) {
            continue;
        }
        if src_routing && t <= src_index {
            continue;
        }
        let (src, tgt) = two_mut(&mut runtime.tracks, src_index, t);
        for f in 0..frames {
            tgt.recv_l[f] += src.block_l[f] * level;
            tgt.recv_r[f] += src.block_r[f] * level;
        }
    }
}

/// `transport_active` — false when this block is rendered while the transport
/// is stopped (MIDI preview, post-panic bridge flush, open plugin editor). In
/// that mode the track/insert graph still runs (so bridged VSTi previews are
/// heard and the host handshake stays alive) but timeline clip material is
/// skipped — otherwise the frozen playhead would stutter-loop the same audio
/// clip slice every callback.
#[allow(clippy::too_many_arguments)]
pub fn render_project_block_interleaved(
    runtime: &mut RuntimeProject,
    base_sample: u64,
    master_volume: f32,
    output: &mut [f32],
    channels: usize,
    transport_active: bool,
    time_sig_num: u32,
    time_sig_den: u32,
) -> u64 {
    if channels < 2 {
        return 0;
    }
    let frames = output.len() / channels;
    if frames == 0 {
        return 0;
    }
    let block_beat = sample_to_beat(runtime, base_sample);
    // Real transport ProcessContext for every plugin processed this block —
    // tempo from the map at this position, time signature from the engine,
    // project position from the playhead, playing = transport state. Replaces
    // the old hardcoded 120 BPM / always-playing stub.
    let transport = RuntimeTransportContext {
        tempo_bpm: runtime.tempo_map.bpm_at_beat(block_beat),
        time_sig_num,
        time_sig_den,
        project_time_samples: base_sample as i64,
        ppq_position: block_beat,
        bar_position_ppq: RuntimeTransportContext::bar_start_ppq(
            block_beat,
            time_sig_num,
            time_sig_den,
        ),
        playing: transport_active,
        recording: false,
    };
    for frame in output.chunks_mut(channels) {
        frame[0] = 0.0;
        frame[1] = 0.0;
        for extra in frame.iter_mut().skip(2) {
            *extra = 0.0;
        }
    }

    for track in &mut runtime.tracks {
        if track.block_l.len() < frames {
            track.block_l.resize(frames, 0.0);
            track.block_r.resize(frames, 0.0);
        }
        // Receive buffers grow lazily to the largest block seen; the audio
        // thread only `fill`s, never allocates, once warmed.
        if track.recv_l.len() < frames {
            track.recv_l.resize(frames, 0.0);
            track.recv_r.resize(frames, 0.0);
        }
        track.block_l[..frames].fill(0.0);
        track.block_r[..frames].fill(0.0);
        track.recv_l[..frames].fill(0.0);
        track.recv_r[..frames].fill(0.0);
    }

    let master_index = runtime.audio_graph.master_index;

    for clip_index in 0..runtime.clips.len() {
        if !transport_active {
            break; // stopped-transport preview block — no timeline material
        }
        let clip = &runtime.clips[clip_index];
        if clip.muted {
            continue;
        }
        let Some(track_index) = runtime.tracks.iter().position(|t| t.id == clip.track_id) else {
            continue;
        };
        if effective_track_muted(&runtime.tracks[track_index], block_beat)
            || (runtime.has_solo && !runtime.tracks[track_index].solo)
        {
            continue;
        }

        let clip_start = clip.start_sample;
        let clip_end = clip.start_sample.saturating_add(clip.duration_samples);
        let block_start = base_sample;
        let block_end = base_sample.saturating_add(frames as u64);
        if block_end <= clip_start || block_start >= clip_end {
            continue;
        }

        let render_start = clip_start.saturating_sub(block_start) as usize;
        let render_end = (clip_end.min(block_end) - block_start) as usize;
        let source = Arc::clone(&clip.source);
        for frame_idx in render_start..render_end {
            let project_sample = base_sample + frame_idx as u64;
            let rel = project_sample - clip_start;
            let source_pos_seconds = clip.offset_seconds
                + (rel as f64 / runtime.sample_rate.max(1) as f64) * clip.speed_ratio as f64;
            let source_pos = source_pos_seconds * source.sample_rate() as f64;
            let (mut l, mut r) = sample_source_stereo(&source, source_pos);
            let fade = clip_fade_gain(
                rel,
                clip.duration_samples,
                clip.fade_in_samples,
                clip.fade_out_samples,
            );
            let g = clip.gain * fade;
            l *= g;
            r *= g;
            runtime.tracks[track_index].block_l[frame_idx] += l;
            runtime.tracks[track_index].block_r[frame_idx] += r;
        }
    }

    // ── Pass 1: source tracks (audio / midi / instrument) ───────────────
    // Clips → inserts → fader, sum the post-fader signal into the master
    // output, then feed sends into routing-track receive buffers. Routing
    // tracks (bus/return/group) are deferred to Pass 2 so their inputs are complete.
    let pass1_indices = runtime.audio_graph.pass1_source_indices.clone();
    for &track_index in &pass1_indices {
        if effective_track_muted(&runtime.tracks[track_index], block_beat)
            || (runtime.has_solo && !runtime.tracks[track_index].solo)
        {
            continue;
        }
        if callback_debug_enabled()
            && !runtime.tracks[track_index].inserts.is_empty()
            && !runtime.tracks[track_index].callback_clip_route_log_done
        {
            runtime.tracks[track_index].callback_clip_route_log_done = true;
            let track_id = runtime.tracks[track_index].id.clone();
            let block_start = base_sample;
            let block_end = base_sample.saturating_add(frames as u64);
            let input_peak_l = runtime.tracks[track_index].block_l[..frames]
                .iter()
                .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
            let input_peak_r = runtime.tracks[track_index].block_r[..frames]
                .iter()
                .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
            let mut clip_count = 0usize;
            let mut overlapping = 0usize;
            let mut first_clip = String::from("none");
            for clip in runtime
                .clips
                .iter()
                .filter(|clip| clip.track_id == track_id)
            {
                let clip_start = clip.start_sample;
                let clip_end = clip.start_sample.saturating_add(clip.duration_samples);
                let overlaps = block_end > clip_start && block_start < clip_end;
                if clip_count == 0 {
                    first_clip = format!(
                        "{} range={}..{} offset={:.3}s gain={:.3} speed={:.3} overlaps={}",
                        clip.id,
                        clip_start,
                        clip_end,
                        clip.offset_seconds,
                        clip.gain,
                        clip.speed_ratio,
                        overlaps
                    );
                }
                clip_count += 1;
                if overlaps {
                    overlapping += 1;
                }
            }
            eprintln!(
                "[SphereAudio callback] clipRoute track={} block={}..{} clips={} overlapping={} preInsertPeakL={:.6} preInsertPeakR={:.6} firstClip={}",
                track_id,
                block_start,
                block_end,
                clip_count,
                overlapping,
                input_peak_l,
                input_peak_r,
                first_clip
            );
        }
        process_track_block(runtime, track_index, frames, output, channels, block_beat, transport);
    }

    // ── Pass 2: routing tracks (bus / return / group) ───────────────────
    // Input = the accumulated send receive buffer. Process inserts → fader and
    // sum to the master output. Solo is ignored for routing tracks so soloing
    // a *source* track still lets its send reach the return. Order comes from
    // the precomputed topological sort in `RuntimeAudioGraph`.
    let pass2_indices = runtime.audio_graph.pass2_routing_indices.clone();
    for &track_index in &pass2_indices {
        if effective_track_muted(&runtime.tracks[track_index], block_beat) {
            continue;
        }
        {
            let track = &mut runtime.tracks[track_index];
            track.block_l[..frames].copy_from_slice(&track.recv_l[..frames]);
            track.block_r[..frames].copy_from_slice(&track.recv_r[..frames]);
        }
        process_track_block(runtime, track_index, frames, output, channels, block_beat, transport);
    }

    // ── Master bus: apply master track inserts on the summed output ──
    if let Some(m_idx) = master_index {
        let muted = effective_track_muted(&runtime.tracks[m_idx], block_beat)
            || (runtime.has_solo && !runtime.tracks[m_idx].solo);
        if !muted {
            let master = &mut runtime.tracks[m_idx];
            // Copy summed output into master scratch buffer.
            for i in 0..frames {
                let frame = &output[i * channels..i * channels + channels];
                master.block_l[i] = frame[0];
                master.block_r[i] = frame[1];
            }
            apply_track_chain_block(
                master,
                frames,
                &std::collections::HashMap::new(),
                transport,
            );
            // Write back, accumulate master meter, apply preview mode.
            for i in 0..frames {
                let (l, r) =
                    apply_preview_mode(master.block_l[i], master.block_r[i], master.preview_mode);
                master.meter_peak_l = master.meter_peak_l.max(l.abs());
                master.meter_peak_r = master.meter_peak_r.max(r.abs());
                master.meter_sum_sq_l += l * l;
                master.meter_sum_sq_r += r * r;
                let out = &mut output[i * channels..i * channels + channels];
                out[0] = l;
                out[1] = r;
            }
        }
    }

    // Final master volume + soft-knee limiter (graceful brick-wall instead of
    // a harsh hard clip when the bus is hot).
    for i in 0..frames {
        let out = &mut output[i * channels..i * channels + channels];
        out[0] = crate::dsp::gain::soft_limit(out[0] * master_volume);
        out[1] = crate::dsp::gain::soft_limit(out[1] * master_volume);
    }

    frames as u64
}

#[inline]
pub fn is_master_output(output: &str) -> bool {
    output.is_empty() || output == "master" || output == "none"
}

#[inline]
pub fn apply_track_chain_at_beat(
    mut l: f32,
    mut r: f32,
    track: &mut RuntimeTrack,
    beat: f64,
) -> (f32, f32) {
    if !track.inserts.is_empty() && !track.callback_insert_log_done {
        track.callback_insert_log_done = true;
        if callback_debug_enabled() {
            eprintln!(
                "[SphereAudio callback] track={} inserts={}",
                track.id,
                track.inserts.len()
            );
        }
    }
    for insert in &mut track.inserts {
        let processed = apply_insert(l, r, insert);
        l = processed.0;
        r = processed.1;
    }
    let automation = track.automation_values_at_beat(beat);
    let volume = automation.volume.unwrap_or(track.volume);
    let pan = automation.pan.unwrap_or(track.pan);
    let (pan_l, pan_r) = pan_gains(pan);
    (l * volume * pan_l, r * volume * pan_r)
}

pub fn apply_track_chain_block(
    track: &mut RuntimeTrack,
    frames: usize,
    bridge_sinks: &std::collections::HashMap<
        String,
        std::sync::Arc<dyn crate::plugin_bridge::PluginBridgeSink>,
    >,
    transport: RuntimeTransportContext,
) {
    if !track.inserts.is_empty() && !track.callback_insert_log_done {
        track.callback_insert_log_done = true;
        if callback_debug_enabled() {
            eprintln!(
                "[SphereAudio callback] track={} inserts={} blockFrames={}",
                track.id,
                track.inserts.len(),
                frames
            );
        }
    }
    let instrument_ix = track.midi_instrument_insert_ix;
    let midi_events = &track.midi_block_events;
    for (ix, insert) in track.inserts.iter_mut().enumerate() {
        let midi = instrument_ix
            .filter(|&i| i == ix)
            .map(|_| midi_events.as_slice());
        if insert.kind.eq_ignore_ascii_case("external-bridge-plugin") {
            let bridge_sink = bridge_sinks.get(&insert.id).map(|s| s.as_ref());
            apply_external_bridge_insert_block(
                &mut track.block_l[..frames],
                &mut track.block_r[..frames],
                insert,
                midi,
                bridge_sink,
                ix,
                transport,
            );
        } else {
            apply_insert_block(
                &mut track.block_l[..frames],
                &mut track.block_r[..frames],
                insert,
                midi,
                transport,
            );
        }
    }
}

fn push_vst3_midi_to_sink(
    sink: &dyn crate::plugin_bridge::PluginBridgeSink,
    events: &[crate::vst3_processor::Vst3MidiEvent],
    instance_id: &str,
) {
    let verbose = crate::runtime::midi_verbose_enabled();
    for ev in events {
        crate::runtime::push_vst3_midi_event_to_sink(sink, ev, instance_id, verbose);
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_external_bridge_insert_block(
    block_l: &mut [f32],
    block_r: &mut [f32],
    insert: &mut RuntimeInsert,
    midi_events: Option<&[crate::vst3_processor::Vst3MidiEvent]>,
    bridge_sink: Option<&dyn crate::plugin_bridge::PluginBridgeSink>,
    slot_index: usize,
    transport: RuntimeTransportContext,
) {
    let frames = block_l.len().min(block_r.len());
    if frames == 0 || !insert.enabled {
        return;
    }
    let Some(sink) = bridge_sink else {
        if plugin_restore_debug_enabled() && insert.bridge_missed_blocks == 0 {
            eprintln!(
                "[AudioGraph] processing insert skipped instance={} reason=no_bridge_sink",
                insert.id
            );
        }
        return;
    };
    if plugin_restore_debug_enabled() && insert.bridge_missed_blocks == 0 {
        let input_peak = block_l[..frames]
            .iter()
            .chain(block_r[..frames].iter())
            .fold(0.0f32, |p, s| p.max(s.abs()));
        eprintln!(
            "[BridgeProcess] track=<chain> slot={slot_index} instance={} input_peak={input_peak:.6}",
            insert.id
        );
    }

    // Clip MIDI for bridged plugins is pushed in schedule_midi_block. Preview
    // MIDI is pushed in drain_commands. Non-bridge inserts still use midi_block_events.
    if let Some(events) = midi_events.filter(|e| !e.is_empty()) {
        let verbose = crate::runtime::midi_verbose_enabled();
        if verbose {
            eprintln!(
                "[plugin-dsp-midi-write] instance={} events={}",
                insert.id,
                events.len()
            );
        }
        push_vst3_midi_to_sink(sink, events, &insert.id);
    }

    let role = insert
        .params
        .get("role")
        .and_then(|v| v.as_str())
        .unwrap_or("instrument");
    let is_effect = role.eq_ignore_ascii_case("effect");

    if is_effect {
        sink.write_input(&block_l[..frames], &block_r[..frames], frames);
    }

    if insert.scratch_l.len() < frames {
        insert.scratch_l.resize(frames, 0.0);
        insert.scratch_r.resize(frames, 0.0);
    }
    let got = sink.read_output(
        &mut insert.scratch_l[..frames],
        &mut insert.scratch_r[..frames],
        frames,
    );

    // Missed-deadline accounting: `read_output` returns 0 when the host has
    // not produced a fresh block (its service thread is stalled behind an
    // editor open/close or a plugin load). The block below then bypasses the
    // insert (effect keeps the dry signal, instrument contributes silence) —
    // stale output is never replayed. A few misses are normal on startup and
    // when resuming from pause, so only log once a stall is established.
    const BRIDGE_MISS_LOG_THRESHOLD: u32 = 8;
    if got == 0 {
        insert.bridge_missed_blocks = insert.bridge_missed_blocks.saturating_add(1);
        if plugin_restore_debug_enabled()
            && (insert.bridge_missed_blocks == 1
                || insert.bridge_missed_blocks == BRIDGE_MISS_LOG_THRESHOLD
                || insert.bridge_missed_blocks % 1024 == 0)
        {
            eprintln!(
                "[Bridge] missed/bypass instance_id={} missed_blocks={}",
                insert.id, insert.bridge_missed_blocks
            );
        }
        if insert.bridge_missed_blocks == BRIDGE_MISS_LOG_THRESHOLD
            || insert.bridge_missed_blocks % 1024 == 0
        {
            if is_effect {
                eprintln!(
                    "[AudioEngine] plugin missed deadline; bypassing to dry signal instance={} missed_blocks={}",
                    insert.id, insert.bridge_missed_blocks
                );
            } else {
                eprintln!(
                    "[VSTi] missed bridge block; output silence instance={} missed_blocks={}",
                    insert.id, insert.bridge_missed_blocks
                );
            }
        }
    } else {
        if plugin_restore_debug_enabled() {
            let out_peak = insert.scratch_l[..got]
                .iter()
                .chain(insert.scratch_r[..got].iter())
                .fold(0.0f32, |p, s| p.max(s.abs()));
            eprintln!(
                "[BridgeProcess] track=<chain> slot={slot_index} instance={} fresh output_peak={out_peak:.6} frames={got}",
                insert.id
            );
        }
        if insert.bridge_missed_blocks >= BRIDGE_MISS_LOG_THRESHOLD {
            if is_effect {
                eprintln!(
                    "[AudioEngine] plugin host recovered instance={} missed_blocks={}",
                    insert.id, insert.bridge_missed_blocks
                );
            } else {
                eprintln!(
                    "[VSTi] recovered after missed blocks={} instance={}",
                    insert.bridge_missed_blocks, insert.id
                );
            }
        }
        insert.bridge_missed_blocks = 0;
    }

    let mut out_peak_l = 0.0f32;
    let mut out_peak_r = 0.0f32;
    if is_effect && got > 0 {
        block_l[..got].copy_from_slice(&insert.scratch_l[..got]);
        block_r[..got].copy_from_slice(&insert.scratch_r[..got]);
        out_peak_l = insert.scratch_l[..got]
            .iter()
            .fold(0.0f32, |p, s| p.max(s.abs()));
        out_peak_r = insert.scratch_r[..got]
            .iter()
            .fold(0.0f32, |p, s| p.max(s.abs()));
    } else if !is_effect {
        for i in 0..got {
            block_l[i] += insert.scratch_l[i];
            block_r[i] += insert.scratch_r[i];
            out_peak_l = out_peak_l.max(insert.scratch_l[i].abs());
            out_peak_r = out_peak_r.max(insert.scratch_r[i].abs());
        }
    }
    if crate::forensic_trace::engine_midi_verbose_enabled()
        && (out_peak_l > 0.0001 || out_peak_r > 0.0001)
    {
        eprintln!(
            "[SphereAudio] external_bridge output_peak_l={:.6} output_peak_r={:.6}",
            out_peak_l, out_peak_r
        );
        eprintln!(
            "[plugin-host-dsp] response_peak_l={:.6} response_peak_r={:.6}",
            out_peak_l, out_peak_r
        );
    }

    // Publish the real transport ProcessContext for this block before kicking
    // the host, so the bridged plugin sees true tempo/position/playing instead
    // of the old hardcoded stub. Wait-free atomic stores.
    sink.set_transport(&transport);

    // Drive the host DSP handshake: MIDI was already pushed to the shared ring.
    if plugin_restore_debug_enabled() && insert.bridge_missed_blocks == 0 {
        eprintln!(
            "[Bridge] request block instance_id={} frames={frames}",
            insert.id
        );
    }
    sink.request_block(frames as u32);
}

#[inline]
pub fn apply_preview_mode(l: f32, r: f32, mode: RuntimePreviewMode) -> (f32, f32) {
    match mode {
        RuntimePreviewMode::Stereo => (l, r),
        RuntimePreviewMode::Mono | RuntimePreviewMode::Mid => {
            let m = (l + r) * 0.5;
            (m, m)
        }
        RuntimePreviewMode::Side => {
            let s = (l - r) * 0.5;
            (s, s)
        }
    }
}

#[inline]
pub fn apply_insert(l: f32, r: f32, insert: &mut RuntimeInsert) -> (f32, f32) {
    if insert.kind.eq_ignore_ascii_case("native-plugin") {
        let format = insert
            .params
            .get("format")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        if !insert.enabled {
            if !insert.callback_process_log_done {
                insert.callback_process_log_done = true;
                eprintln!(
                    "[SphereAudio callback] insert={} format={} bypass=true beforePeakL={:.6} beforePeakR={:.6} afterPeakL={:.6} afterPeakR={:.6}",
                    insert.id,
                    format,
                    l.abs(),
                    r.abs(),
                    l.abs(),
                    r.abs()
                );
            }
            return (l, r);
        }
        if let Some(vst3) = insert.vst3.as_mut() {
            let handle = vst3.handle_value();
            let processed = vst3.process_stereo_sample(l, r);
            let (out_l, out_r) = processed.unwrap_or((l, r));
            if !insert.callback_process_log_done {
                insert.callback_process_log_done = true;
                eprintln!(
                    "[SphereAudio callback] insert={} format={} processorHandle=0x{:x} bypass=false processOk={} beforePeakL={:.6} beforePeakR={:.6} afterPeakL={:.6} afterPeakR={:.6}",
                    insert.id,
                    format,
                    handle,
                    processed.is_some(),
                    l.abs(),
                    r.abs(),
                    out_l.abs(),
                    out_r.abs()
                );
            }
            return (out_l, out_r);
        }
        if !insert.callback_process_log_done {
            insert.callback_process_log_done = true;
            eprintln!(
                "[SphereAudio callback] insert={} format={} processorHandle=0x0 bypass=false processOk=false beforePeakL={:.6} beforePeakR={:.6} afterPeakL={:.6} afterPeakR={:.6}",
                insert.id,
                format,
                l.abs(),
                r.abs(),
                l.abs(),
                r.abs()
            );
        }
        return (l, r);
    }

    let plugin_id = canonical_plugin_id(&insert.kind);
    process_stereo_sample(
        plugin_id,
        insert.enabled,
        &insert.params,
        &mut insert.dsp,
        l,
        r,
    )
}

pub fn apply_insert_block(
    block_l: &mut [f32],
    block_r: &mut [f32],
    insert: &mut RuntimeInsert,
    midi_events: Option<&[crate::vst3_processor::Vst3MidiEvent]>,
    transport: RuntimeTransportContext,
) {
    if block_l.is_empty() || block_r.is_empty() {
        return;
    }
    if !insert.kind.eq_ignore_ascii_case("native-plugin") {
        for i in 0..block_l.len().min(block_r.len()) {
            let (l, r) = apply_insert(block_l[i], block_r[i], insert);
            block_l[i] = l;
            block_r[i] = r;
        }
        return;
    }

    let format = insert
        .params
        .get("format")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let before_peak_l = block_l
        .iter()
        .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
    let before_peak_r = block_r
        .iter()
        .fold(0.0f32, |peak, sample| peak.max(sample.abs()));

    if !insert.enabled {
        if !insert.callback_process_log_done {
            insert.callback_process_log_done = true;
            eprintln!(
                "[SphereAudio callback] insert={} format={} bypass=true blockFrames={} beforePeakL={:.6} beforePeakR={:.6} afterPeakL={:.6} afterPeakR={:.6}",
                insert.id,
                format,
                block_l.len().min(block_r.len()),
                before_peak_l,
                before_peak_r,
                before_peak_l,
                before_peak_r
            );
        }
        return;
    }

    let Some(vst3) = insert.vst3.as_mut() else {
        if !insert.callback_process_log_done {
            insert.callback_process_log_done = true;
            eprintln!(
                "[SphereAudio callback] insert={} format={} processorHandle=0x0 bypass=false processOk=false blockFrames={} beforePeakL={:.6} beforePeakR={:.6} afterPeakL={:.6} afterPeakR={:.6}",
                insert.id,
                format,
                block_l.len().min(block_r.len()),
                before_peak_l,
                before_peak_r,
                before_peak_l,
                before_peak_r
            );
        }
        return;
    };

    // Guard: if the underlying C++ processor was destroyed (e.g., Arc dropped
    // on another thread racing with this callback), bypass and log once.
    if !vst3.is_processor_valid() {
        if !insert.callback_process_log_done {
            insert.callback_process_log_done = true;
            eprintln!(
                "[SphereAudio callback] insert={} format={} processorHandle=0x{:x} INVALID/DESTROYED bypass=true — insert bypassed to prevent use-after-free",
                insert.id, format, vst3.handle_value()
            );
        }
        return;
    }

    let frames = block_l.len().min(block_r.len());
    if insert.scratch_l.len() < frames {
        insert.scratch_l.resize(frames, 0.0);
        insert.scratch_r.resize(frames, 0.0);
    }
    insert.scratch_l[..frames].fill(0.0);
    insert.scratch_r[..frames].fill(0.0);

    // Real transport ProcessContext for this block, immediately before the
    // plugin processes it (same thread, no race with process()).
    vst3.set_process_context(&transport);

    let handle = vst3.handle_value();
    let process_ok = if let Some(events) = midi_events.filter(|e| !e.is_empty()) {
        vst3.process_stereo_block_with_midi(
            &block_l[..frames],
            &block_r[..frames],
            &mut insert.scratch_l[..frames],
            &mut insert.scratch_r[..frames],
            events,
        )
    } else {
        vst3.process_stereo_block(
            &block_l[..frames],
            &block_r[..frames],
            &mut insert.scratch_l[..frames],
            &mut insert.scratch_r[..frames],
        )
    };
    if process_ok {
        block_l[..frames].copy_from_slice(&insert.scratch_l[..frames]);
        block_r[..frames].copy_from_slice(&insert.scratch_r[..frames]);
    }

    if before_peak_l <= 0.000001 && before_peak_r <= 0.000001 {
        insert.silent_process_blocks = insert.silent_process_blocks.saturating_add(1);
    }

    if !insert.callback_process_log_done
        && (before_peak_l > 0.000001
            || before_peak_r > 0.000001
            || insert.silent_process_blocks >= 200)
    {
        insert.callback_process_log_done = true;
        let after_peak_l = block_l[..frames]
            .iter()
            .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
        let after_peak_r = block_r[..frames]
            .iter()
            .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
        eprintln!(
            "[SphereAudio callback] insert={} format={} processorHandle=0x{:x} bypass=false processOk={} blockFrames={} silentBlocks={} beforePeakL={:.6} beforePeakR={:.6} afterPeakL={:.6} afterPeakR={:.6}",
            insert.id,
            format,
            handle,
            process_ok,
            frames,
            insert.silent_process_blocks,
            before_peak_l,
            before_peak_r,
            after_peak_l,
            after_peak_r
        );
    }
}

#[inline]
pub fn pan_gains(pan: f32) -> (f32, f32) {
    let pan = pan.clamp(-1.0, 1.0);
    if pan < 0.0 {
        (1.0, 1.0 + pan)
    } else {
        (1.0 - pan, 1.0)
    }
}

fn build_output_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: SampleFormat,
    cmd_rx: Receiver<EngineCommand>,
    shared: Arc<SharedState>,
    initial_runtime: RuntimeProject,
) -> Result<cpal::Stream, String> {
    match sample_format {
        SampleFormat::I8 => {
            build_output_stream_typed::<i8>(device, config, cmd_rx, shared, initial_runtime)
        }
        SampleFormat::I16 => {
            build_output_stream_typed::<i16>(device, config, cmd_rx, shared, initial_runtime)
        }
        SampleFormat::I32 => {
            build_output_stream_typed::<i32>(device, config, cmd_rx, shared, initial_runtime)
        }
        SampleFormat::I64 => {
            build_output_stream_typed::<i64>(device, config, cmd_rx, shared, initial_runtime)
        }
        SampleFormat::U8 => {
            build_output_stream_typed::<u8>(device, config, cmd_rx, shared, initial_runtime)
        }
        SampleFormat::U16 => {
            build_output_stream_typed::<u16>(device, config, cmd_rx, shared, initial_runtime)
        }
        SampleFormat::U32 => {
            build_output_stream_typed::<u32>(device, config, cmd_rx, shared, initial_runtime)
        }
        SampleFormat::U64 => {
            build_output_stream_typed::<u64>(device, config, cmd_rx, shared, initial_runtime)
        }
        SampleFormat::F32 => {
            build_output_stream_typed::<f32>(device, config, cmd_rx, shared, initial_runtime)
        }
        SampleFormat::F64 => {
            build_output_stream_typed::<f64>(device, config, cmd_rx, shared, initial_runtime)
        }
        format => Err(format!("unsupported output sample format: {format}")),
    }
}

fn build_output_stream_typed<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    cmd_rx: Receiver<EngineCommand>,
    shared: Arc<SharedState>,
    initial_runtime: RuntimeProject,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + Sample + FromSample<f32>,
{
    let output_sample_rate = config.sample_rate.0;
    let sr = output_sample_rate as f64;

    // Oscillator state — local to the audio callback (no lock needed).
    let mut osc_l = SineOscillator::new(440.0, sr);
    let mut osc_r = SineOscillator::new(440.0, sr);
    let mut osc_freq = 440.0f32;
    let mut osc_on = false;

    // Local playback state.
    let mut playing_local = false;
    let mut render_path_logged = false;
    let mut block_scratch: Vec<f32> = Vec::new();
    let mut metronome = crate::backend::render::LocalAudioState::new(sr);

    // Meter state with peak hold.
    let mut prev_peak_l = 0.0f32;
    let mut prev_peak_r = 0.0f32;

    let ch = config.channels as usize;
    let mut runtime = initial_runtime;
    runtime.sample_rate = output_sample_rate;

    let stream = device
        .build_output_stream::<T, _, _>(
            config,
            move |data: &mut [T], _info: &cpal::OutputCallbackInfo| {
                if ch > 0 {
                    for track in &mut runtime.tracks {
                        track.midi_block_events.clear();
                    }
                }

                // ── 1. Drain command queue ───────────────────────────────────
                // Runs first so commands take effect from the start of this block.
                let frames_for_flush = if ch > 0 {
                    data.len().checked_div(ch).unwrap_or(0)
                } else {
                    0
                };
                while let Ok(cmd) = cmd_rx.try_recv() {
                    match cmd {
                        EngineCommand::LoadProject(next_runtime) => {
                            if callback_debug_enabled() {
                                eprintln!(
                                    "[SphereAudio callback] LoadProject: {} tracks, {} clips (sr={})",
                                    next_runtime.tracks.len(),
                                    next_runtime.clips.len(),
                                    output_sample_rate,
                                );
                            }
                            // Release notes on the current graph before swapping
                            // it out, otherwise pending note-offs would be
                            // dropped with the retired VST3 processors.
                            runtime.all_notes_off("project_load");
                            runtime.flush_vst3_midi_inserts(frames_for_flush);
                            // Swap in the new graph and retire the old one off
                            // the realtime thread — its destructor frees
                            // buffers / munmaps sources / destroys VST3 handles
                            // and must not run here. See `crate::graveyard`.
                            let old = std::mem::replace(&mut runtime, *next_runtime);
                            runtime.sample_rate = output_sample_rate;
                            // Preserve the plugin-bridge sinks across reloads (Stage 3b).
                            runtime.plugin_bridge_sinks = old.plugin_bridge_sinks.clone();
                            runtime.bridge_editor_active = old.bridge_editor_active.clone();
                            // The panic pushed into the preserved sinks above
                            // still needs flushing through the new graph.
                            runtime.bridge_panic_flush_samples = old.bridge_panic_flush_samples;
                            let pos = shared.position_samples.load(Ordering::Relaxed);
                            metronome.set_tempo_map(
                                runtime.tempo_map.clone(),
                                pos,
                                output_sample_rate,
                            );
                            crate::graveyard::retire(old);
                            if crate::runtime::midi_engine_debug_enabled() {
                                let notes: usize =
                                    runtime.midi_clips.iter().map(|c| c.events.len() / 2).sum();
                                eprintln!(
                                    "[DAUx MIDI] snapshot clips={} notes={}",
                                    runtime.midi_clips.len(),
                                    notes
                                );
                            }
                            // Position MIDI cursors for the current transport
                            // position so a load mid-playback stays in sync.
                            let midi_pos = shared.position_samples.load(Ordering::Relaxed);
                            runtime.reset_midi_playback(midi_pos);
                        }
                        EngineCommand::SetTestTone { enabled, frequency } => {
                            osc_on = enabled;
                            osc_freq = frequency;
                            osc_l.set_frequency(frequency as f64);
                            osc_r.set_frequency(frequency as f64);
                        }
                        EngineCommand::StartTransport => {
                            let pos = shared.position_samples.load(Ordering::Relaxed);
                            let active_clips = runtime.active_clip_count_at_sample(pos);
                            if command_debug_enabled() {
                                eprintln!(
                                    "[SphereAudio callback] StartTransport: position={}sa ({:.3}s), active_clip_count={}, scheduled_clip_count={}",
                                    pos,
                                    pos as f64 / output_sample_rate as f64,
                                    active_clips,
                                    runtime.clips.len(),
                                );
                            }
                            playing_local = true;
                            shared.playing.store(true, Ordering::Relaxed);
                            // Position MIDI cursors at the start beat and clear
                            // any stale active notes so play-from is clean.
                            runtime.reset_midi_playback(pos);
                        }
                        EngineCommand::StopTransport => {
                            if command_debug_enabled() {
                                eprintln!("[SphereAudio callback] StopTransport");
                            }
                            playing_local = false;
                            shared.playing.store(false, Ordering::Relaxed);
                            // Release held notes so nothing is left stuck.
                            runtime.all_notes_off("stop");
                        }
                        EngineCommand::Seek { position_seconds } => {
                            let sr_local = shared.sample_rate.load(Ordering::Relaxed) as f64;
                            let pos = (position_seconds * sr_local) as u64;
                            if command_debug_enabled() {
                                eprintln!(
                                    "[SphereAudio callback] Seek -> {:.3}s ({}sa)",
                                    position_seconds, pos
                                );
                            }
                            shared.position_samples.store(pos, Ordering::Relaxed);
                            metronome.reset_metronome_schedule(pos, output_sample_rate);
                            // Re-seek MIDI cursors + flush held notes.
                            runtime.reset_midi_playback(pos);
                        }
                        EngineCommand::SetMetronomeEnabled(enabled) => {
                            let pos = shared.position_samples.load(Ordering::Relaxed);
                            shared
                                .metronome_enabled
                                .store(enabled, Ordering::Relaxed);
                            metronome.set_metronome_enabled(enabled, pos, output_sample_rate);
                        }
                        EngineCommand::SetBpm(bpm) => {
                            let pos = shared.position_samples.load(Ordering::Relaxed);
                            transport::store_f64_bits(&shared.bpm_bits, bpm);
                            let map = crate::tempo_map::RuntimeTempoMapSnapshot::static_tempo(bpm);
                            metronome.set_tempo_map(map.clone(), pos, output_sample_rate);
                            let next_pos = runtime.apply_tempo_map(map, pos);
                            shared.position_samples.store(next_pos, Ordering::Relaxed);
                        }
                        EngineCommand::SetTempoMap(map) => {
                            let pos = shared.position_samples.load(Ordering::Relaxed);
                            metronome.set_tempo_map(map.clone(), pos, output_sample_rate);
                            let next_pos = runtime.apply_tempo_map(map, pos);
                            shared.position_samples.store(next_pos, Ordering::Relaxed);
                        }
                        EngineCommand::SetTimeSignature(num, den) => {
                            let pos = shared.position_samples.load(Ordering::Relaxed);
                            shared.time_sig_num.store(num.max(1), Ordering::Relaxed);
                            shared.time_sig_den.store(den.max(1), Ordering::Relaxed);
                            metronome.set_time_signature(num, den, pos, output_sample_rate);
                        }
                        EngineCommand::SetTimeSignatureMap(map) => {
                            let pos = shared.position_samples.load(Ordering::Relaxed);
                            if let Some(pt) = map.points().first() {
                                shared
                                    .time_sig_num
                                    .store(pt.numerator.max(1) as u32, Ordering::Relaxed);
                                shared
                                    .time_sig_den
                                    .store(pt.denominator.max(1) as u32, Ordering::Relaxed);
                            }
                            metronome.set_time_signature_map(map, pos, output_sample_rate);
                        }
                        EngineCommand::SetLoop {
                            enabled,
                            start_seconds,
                            end_seconds,
                        } => {
                            let sr_local = shared.sample_rate.load(Ordering::Relaxed) as f64;
                            let start = (start_seconds.max(0.0) * sr_local) as u64;
                            let end = (end_seconds.max(0.0) * sr_local) as u64;
                            shared.loop_enabled.store(enabled, Ordering::Relaxed);
                            shared
                                .loop_start_samples
                                .store(start, Ordering::Relaxed);
                            shared.loop_end_samples.store(end, Ordering::Relaxed);
                        }
                        EngineCommand::SetMasterVolume { value } => {
                            shared
                                .master_volume
                                .store(f32_store(value), Ordering::Relaxed);
                        }
                        EngineCommand::SetPluginBridgeSink { insert_id, sink } => match sink {
                            Some(sink) => {
                                runtime.plugin_bridge_sinks.insert(insert_id, sink);
                            }
                            None => {
                                runtime.plugin_bridge_sinks.remove(&insert_id);
                            }
                        },
                        EngineCommand::SetBridgeEditorActive { track_id, active } => {
                            runtime.set_bridge_editor_active(&track_id, active);
                        }
                        EngineCommand::SetTrackVolume { track_id, value } => {
                            runtime.update_track_volume(&track_id, value);
                        }
                        EngineCommand::SetTrackPan { track_id, value } => {
                            runtime.update_track_pan(&track_id, value);
                        }
                        EngineCommand::SetTrackMute { track_id, muted } => {
                            if callback_debug_enabled() {
                                eprintln!("[SphereAudio callback] SetTrackMute track={track_id} muted={muted}");
                            }
                            runtime.all_notes_off("track_mute");
                            runtime.update_track_mute(&track_id, muted);
                        }
                        EngineCommand::SetTrackSolo { track_id, solo } => {
                            runtime.all_notes_off("track_solo");
                            runtime.update_track_solo(&track_id, solo);
                        }
                        EngineCommand::SetTrackPreviewMode { track_id, value } => {
                            runtime.update_track_preview_mode(&track_id, RuntimePreviewMode::from_code(value));
                        }
                        EngineCommand::SetInsertParam { track_id, insert_id, param_id, value } => {
                            runtime.update_insert_param(&track_id, &insert_id, &param_id, value);
                        }
                        EngineCommand::MidiPreviewNoteOn {
                            track_id,
                            channel,
                            pitch,
                            velocity,
                        } => {
                            runtime.midi_preview_note_on(&track_id, channel, pitch, velocity);
                        }
                        EngineCommand::MidiPreviewNoteOff {
                            track_id,
                            channel,
                            pitch,
                        } => {
                            runtime.midi_preview_note_off(&track_id, channel, pitch);
                        }
                        EngineCommand::MidiPreviewAllNotesOff { track_id } => {
                            runtime.midi_preview_all_notes_off(&track_id);
                        }
                        EngineCommand::PluginPreviewNoteOn {
                            track_id,
                            plugin_instance_id,
                            channel,
                            pitch,
                            velocity,
                        } => {
                            runtime.bridge_preview_note_on(
                                &track_id,
                                &plugin_instance_id,
                                channel,
                                pitch,
                                velocity,
                            );
                        }
                        EngineCommand::PluginPreviewNoteOff {
                            track_id,
                            plugin_instance_id,
                            channel,
                            pitch,
                        } => {
                            runtime.bridge_preview_note_off(
                                &track_id,
                                &plugin_instance_id,
                                channel,
                                pitch,
                            );
                        }
                        EngineCommand::PluginPreviewAllNotesOff {
                            track_id,
                            plugin_instance_id,
                        } => {
                            runtime.bridge_preview_all_notes_off(&track_id, &plugin_instance_id);
                        }
                    }
                }

                // ── 2. Read shared control state (Relaxed OK for audio) ──────
                let master_vol = f32_load(shared.master_volume.load(Ordering::Relaxed));
                // Re-sync local osc flags from atomics in case they changed externally.
                let tone_on = shared.test_tone_enabled.load(Ordering::Relaxed);
                let tone_freq = f32_load(shared.test_tone_freq.load(Ordering::Relaxed));
                if tone_freq != osc_freq {
                    osc_freq = tone_freq;
                    osc_l.set_frequency(tone_freq as f64);
                    osc_r.set_frequency(tone_freq as f64);
                }
                let gen_tone = tone_on || osc_on;

                // ── 3. Fill output buffer ────────────────────────────────────
                let mut peak_l = 0.0f32;
                let mut peak_r = 0.0f32;
                let mut sum_sq_l = 0.0f32;
                let mut sum_sq_r = 0.0f32;
                let mut frames = 0u64;
                let base_sample = shared.position_samples.load(Ordering::Relaxed);
                runtime.begin_meter_block();

                // MIDI scheduling — once per block when playing.
                if playing_local && ch > 0 {
                    let frames_needed = data.len().checked_div(ch).unwrap_or(0) as u64;
                    runtime.schedule_midi_block(base_sample, frames_needed);
                }

                let pending_midi = ch > 0
                    && runtime
                        .tracks
                        .iter()
                        .any(|t| !t.midi_block_events.is_empty());
                let frames_in_block = data.len().checked_div(ch).unwrap_or(0) as u64;
                let has_preview = runtime.has_active_midi_preview();
                if playing_local {
                    metronome.preview_tail_samples = 0;
                    // Playing blocks drive the bridge anyway — flush is implicit.
                    runtime.bridge_panic_flush_samples = 0;
                } else if has_preview || pending_midi {
                    // Keep release-tail processing queued past the note-off so a
                    // stopped-transport preview doesn't cut the instrument dead.
                    metronome.preview_tail_samples =
                        (runtime.sample_rate as u64).saturating_mul(2);
                }
                // Post-panic flush: keep the bridge handshake alive after a
                // stop/seek/mute panic so the host drains the panic CCs instead
                // of leaving VSTi voices stuck until the next play.
                let panic_flush = runtime.bridge_panic_flush_samples > 0;
                if !playing_local && panic_flush {
                    runtime.bridge_panic_flush_samples = runtime
                        .bridge_panic_flush_samples
                        .saturating_sub(frames_in_block);
                }
                let bridge_editor_wakeup = runtime.has_bridge_editor_active();
                let preview_render_active = has_preview
                    || pending_midi
                    || panic_flush
                    || metronome.preview_tail_samples > 0
                    || bridge_editor_wakeup;
                if preview_render_active
                    && !playing_local
                    && (has_preview || pending_midi || metronome.preview_tail_samples > 0)
                {
                    let active_notes: usize = runtime
                        .midi_tracks
                        .iter()
                        .map(|mt| mt.preview_active.len())
                        .sum();
                    let active_u32 = active_notes as u32;
                    let changed = active_u32 != metronome.prev_logged_preview_notes;
                    if changed {
                        eprintln!(
                            "[PreviewRenderWake] active_preview_notes changed {} -> {} tail_samples={}",
                            metronome.prev_logged_preview_notes,
                            active_u32,
                            metronome.preview_tail_samples
                        );
                        metronome.prev_logged_preview_notes = active_u32;
                        metronome.preview_wake_log_cooldown = 0;
                    } else if active_notes > 0 {
                        metronome.preview_wake_log_cooldown =
                            metronome.preview_wake_log_cooldown.saturating_add(1);
                        let sr = runtime.sample_rate.max(1);
                        let log_interval_blocks =
                            (sr / frames_in_block.max(1) as u32).max(1);
                        if metronome.preview_wake_log_cooldown >= log_interval_blocks {
                            metronome.preview_wake_log_cooldown = 0;
                            eprintln!(
                                "[PreviewRenderWake] active_preview_notes={} tail_samples={} rendering_while_stopped=true",
                                active_notes, metronome.preview_tail_samples
                            );
                        }
                    }
                    if !has_preview && !pending_midi {
                        metronome.preview_tail_samples =
                            metronome.preview_tail_samples.saturating_sub(frames_in_block);
                        if metronome.preview_tail_samples == 0 {
                            metronome.prev_logged_preview_notes = u32::MAX;
                        }
                    }
                }

                if ch >= 2 && (playing_local || preview_render_active) {
                    let frames_needed = data.len() / ch;
                    let scratch_len = frames_needed * ch;
                    if block_scratch.len() < scratch_len {
                        block_scratch.resize(scratch_len, 0.0);
                    }
                    let scratch = &mut block_scratch[..scratch_len];
                    frames = render_project_block_interleaved(
                        &mut runtime,
                        base_sample,
                        master_vol,
                        scratch,
                        ch,
                        playing_local,
                        shared.time_sig_num.load(Ordering::Relaxed),
                        shared.time_sig_den.load(Ordering::Relaxed),
                    );
                    if !render_path_logged {
                        render_path_logged = true;
                        if callback_debug_enabled() {
                            eprintln!(
                                "[SphereAudio callback] renderPath=legacy-block frames={} channels={} tracks={}",
                                frames,
                                ch,
                                runtime.tracks.len()
                            );
                        }
                    }
                    if gen_tone {
                        for frame in scratch.chunks_mut(ch) {
                            let tone_l = osc_l.next_sample() * TEST_TONE_AMPLITUDE * master_vol;
                            let tone_r = osc_r.next_sample() * TEST_TONE_AMPLITUDE * master_vol;
                            frame[0] = (frame[0] + tone_l).clamp(-1.0, 1.0);
                            frame[1] = (frame[1] + tone_r).clamp(-1.0, 1.0);
                        }
                    }
                    for (i, frame) in scratch.chunks_mut(ch).enumerate() {
                        let click =
                            metronome.metronome_sample(base_sample + i as u64, output_sample_rate);
                        if click != 0.0 {
                            frame[0] = (frame[0] + click * master_vol).clamp(-1.0, 1.0);
                            frame[1] = (frame[1] + click * master_vol).clamp(-1.0, 1.0);
                        }
                    }
                    // (Legacy NAPI path) Software monitoring is handled by the
                    // DAUx/cpal render kernel via the input ring; the old
                    // sample-and-hold monitor was removed (warble).
                    for (out_frame, frame) in data.chunks_mut(ch).zip(scratch.chunks(ch)) {
                        let l = frame[0].clamp(-1.0, 1.0);
                        let r = frame[1].clamp(-1.0, 1.0);
                        out_frame[0] = T::from_sample(l);
                        out_frame[1] = T::from_sample(r);
                        for extra in out_frame.iter_mut().skip(2) {
                            *extra = T::from_sample(0.0);
                        }
                        peak_l = peak_l.max(l.abs());
                        peak_r = peak_r.max(r.abs());
                        sum_sq_l += l * l;
                        sum_sq_r += r * r;
                    }
                } else if ch >= 2 {
                    for frame in data.chunks_mut(ch) {
                        let (tone_l, tone_r) = if gen_tone {
                            (
                                osc_l.next_sample() * TEST_TONE_AMPLITUDE * master_vol,
                                osc_r.next_sample() * TEST_TONE_AMPLITUDE * master_vol,
                            )
                        } else {
                            (0.0, 0.0)
                        };
                        let (project_l, project_r) = if playing_local {
                            render_project_sample(&mut runtime, base_sample + frames, master_vol)
                        } else {
                            (0.0, 0.0)
                        };
                        let click = if playing_local {
                            metronome.metronome_sample(base_sample + frames, output_sample_rate)
                                * master_vol
                        } else {
                            0.0
                        };
                        let l = (tone_l + project_l + click).clamp(-1.0, 1.0);
                        let r = (tone_r + project_r + click).clamp(-1.0, 1.0);
                        frame[0] = T::from_sample(l);
                        frame[1] = T::from_sample(r);
                        // Extra channels get silence.
                        for extra in frame.iter_mut().skip(2) {
                            *extra = T::from_sample(0.0);
                        }
                        peak_l = peak_l.max(l.abs());
                        peak_r = peak_r.max(r.abs());
                        sum_sq_l += l * l;
                        sum_sq_r += r * r;
                        frames += 1;
                    }
                } else if ch == 1 {
                    for sample in data.iter_mut() {
                        let tone = if gen_tone {
                            osc_l.next_sample() * TEST_TONE_AMPLITUDE * master_vol
                        } else {
                            0.0
                        };
                        let (project_l, project_r) = if playing_local {
                            render_project_sample(&mut runtime, base_sample + frames, master_vol)
                        } else {
                            (0.0, 0.0)
                        };
                        let click = if playing_local {
                            metronome.metronome_sample(base_sample + frames, output_sample_rate)
                                * master_vol
                        } else {
                            0.0
                        };
                        let value =
                            (tone + (project_l + project_r) * 0.5 + click).clamp(-1.0, 1.0);
                        *sample = T::from_sample(value);
                        peak_l = peak_l.max(value.abs());
                        sum_sq_l += value * value;
                        frames += 1;
                    }
                }

                if shared.live_input_active.load(Ordering::Relaxed) {
                    let input_l = f32_load(shared.live_input_l.load(Ordering::Relaxed));
                    let input_r = f32_load(shared.live_input_r.load(Ordering::Relaxed));
                    runtime.accumulate_live_input_meters(input_l, input_r);
                }

                // ── 4. Compute meters (no allocation) ────────────────────────
                let rms_l = if frames > 0 {
                    (sum_sq_l / frames as f32).sqrt()
                } else {
                    0.0
                };
                let (pk_r, rms_r) = if ch >= 2 {
                    (
                        peak_r,
                        if frames > 0 {
                            (sum_sq_r / frames as f32).sqrt()
                        } else {
                            0.0
                        },
                    )
                } else {
                    (peak_l, rms_l)
                };
                runtime.end_meter_block(frames);

                prev_peak_l = smooth_peak(prev_peak_l, peak_l, PEAK_DECAY);
                prev_peak_r = smooth_peak(prev_peak_r, pk_r, PEAK_DECAY);

                shared
                    .peak_l
                    .store(f32_store(prev_peak_l), Ordering::Relaxed);
                shared
                    .peak_r
                    .store(f32_store(prev_peak_r), Ordering::Relaxed);
                shared.rms_l.store(f32_store(rms_l), Ordering::Relaxed);
                shared.rms_r.store(f32_store(rms_r), Ordering::Relaxed);

                // Advance position counter.
                if playing_local && ch > 0 {
                    shared.position_samples.fetch_add(frames, Ordering::Relaxed);
                    transport::apply_loop_wrap(&shared, &mut runtime, output_sample_rate, |start| {
                        metronome.reset_metronome_schedule(start, output_sample_rate);
                    });
                }
            },
            move |err| {
                eprintln!("[SphereAudio] Stream error: {err}");
            },
            None,
        )
        .map_err(|e| e.to_string())?;

    Ok(stream)
}

#[cfg(test)]
mod clip_fade_tests {
    use super::clip_fade_gain;

    #[test]
    fn no_fades_is_unity() {
        assert_eq!(clip_fade_gain(0, 1000, 0, 0), 1.0);
        assert_eq!(clip_fade_gain(500, 1000, 0, 0), 1.0);
        assert_eq!(clip_fade_gain(999, 1000, 0, 0), 1.0);
    }

    #[test]
    fn fade_in_ramps_zero_to_one() {
        // 100-sample fade-in over a 1000-sample clip.
        assert_eq!(clip_fade_gain(0, 1000, 100, 0), 0.0);
        assert!((clip_fade_gain(50, 1000, 100, 0) - 0.5).abs() < 1e-6);
        // At/after the fade-in length it is full gain.
        assert_eq!(clip_fade_gain(100, 1000, 100, 0), 1.0);
        assert_eq!(clip_fade_gain(900, 1000, 100, 0), 1.0);
    }

    #[test]
    fn fade_out_ramps_one_to_zero() {
        // 100-sample fade-out: starts at sample 900 (duration - fade_out).
        assert_eq!(clip_fade_gain(899, 1000, 0, 100), 1.0);
        assert!((clip_fade_gain(900, 1000, 0, 100) - 1.0).abs() < 1e-6);
        assert!((clip_fade_gain(950, 1000, 0, 100) - 0.5).abs() < 1e-6);
        assert!(clip_fade_gain(1000, 1000, 0, 100) <= 0.0);
    }

    #[test]
    fn fade_in_and_out_combine() {
        // In the flat middle region both fades are unity.
        assert!((clip_fade_gain(500, 1000, 100, 100) - 1.0).abs() < 1e-6);
        // Inside the fade-in region only the fade-in shapes the gain.
        assert!((clip_fade_gain(25, 1000, 100, 100) - 0.25).abs() < 1e-6);
    }
}

#[cfg(test)]
mod bridge_insert_tests {
    use super::*;
    use crate::plugin_bridge::PluginBridgeSink;
    use crate::runtime::{InsertDspState, RuntimeInsert, RuntimePreviewMode, RuntimeTrack};
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    #[derive(Debug)]
    struct WetEffectSink {
        done_seq: AtomicU64,
        requests: AtomicU64,
    }

    impl PluginBridgeSink for WetEffectSink {
        fn dsp_ready(&self) -> bool {
            true
        }

        fn read_output(&self, out_l: &mut [f32], out_r: &mut [f32], frames: usize) -> usize {
            if self.done_seq.load(Ordering::Acquire) == 0 {
                return 0;
            }
            for i in 0..frames.min(out_l.len()).min(out_r.len()) {
                out_l[i] = 0.25;
                out_r[i] = 0.25;
            }
            frames
        }

        fn push_midi(&self, _: u8, _: u8, _: u8, _: u32) {}

        fn write_input(&self, _: &[f32], _: &[f32], _: usize) {}

        fn request_block(&self, _: u32) {
            self.requests.fetch_add(1, Ordering::Relaxed);
            self.done_seq.store(1, Ordering::Release);
        }
    }

    fn bridge_effect_track(block: f32) -> RuntimeTrack {
        let mut params = HashMap::new();
        params.insert("role".to_string(), serde_json::json!("effect"));
        RuntimeTrack {
            id: "track-1".to_string(),
            track_type: "audio".to_string(),
            volume: 1.0,
            pan: 0.0,
            muted: false,
            solo: false,
            record_armed: false,
            monitor_enabled: false,
            input_source: crate::runtime::RuntimeTrackInputSource::None,
            preview_mode: RuntimePreviewMode::Stereo,
            output_track_id: None,
            inserts: vec![RuntimeInsert {
                id: "insert-1".to_string(),
                kind: "external-bridge-plugin".to_string(),
                enabled: true,
                params,
                dsp: InsertDspState::default(),
                vst3: None,
                callback_process_log_done: false,
                silent_process_blocks: 0,
                bridge_missed_blocks: 0,
                scratch_l: vec![0.0; 8],
                scratch_r: vec![0.0; 8],
            }],
            sends: Vec::new(),
            automation_lanes: Vec::new(),
            meter: Arc::new(Default::default()),
            meter_peak_l: 0.0,
            meter_peak_r: 0.0,
            meter_sum_sq_l: 0.0,
            meter_sum_sq_r: 0.0,
            callback_insert_log_done: false,
            callback_clip_route_log_done: false,
            block_l: vec![block; 8],
            block_r: vec![block; 8],
            recv_l: vec![0.0; 8],
            recv_r: vec![0.0; 8],
            midi_block_events: Vec::new(),
            midi_instrument_insert_ix: None,
            plugin_latency_samples: 0,
            pdc_delay_l: Vec::new(),
            pdc_delay_r: Vec::new(),
            pdc_write_pos: 0,
        }
    }

    #[test]
    fn external_bridge_effect_processes_when_sink_is_bound() {
        let mut track = bridge_effect_track(1.0);
        let sink = WetEffectSink {
            done_seq: AtomicU64::new(1),
            requests: AtomicU64::new(0),
        };
        let mut sinks: std::collections::HashMap<
            String,
            std::sync::Arc<dyn PluginBridgeSink>,
        > = std::collections::HashMap::new();
        sinks.insert("insert-1".to_string(), std::sync::Arc::new(sink));
        apply_track_chain_block(&mut track, 4, &sinks, RuntimeTransportContext::default());
        assert!((track.block_l[0] - 0.25).abs() < 1e-6);
        assert!((track.block_r[0] - 0.25).abs() < 1e-6);
    }

    #[test]
    fn external_bridge_effect_stays_dry_without_sink() {
        let mut track = bridge_effect_track(1.0);
        apply_track_chain_block(
            &mut track,
            4,
            &std::collections::HashMap::new(),
            RuntimeTransportContext::default(),
        );
        assert!((track.block_l[0] - 1.0).abs() < 1e-6);
        assert!((track.block_r[0] - 1.0).abs() < 1e-6);
    }

    fn bridge_effect_track_with_id(id: &str) -> RuntimeInsert {
        let mut params = HashMap::new();
        params.insert("role".to_string(), serde_json::json!("effect"));
        RuntimeInsert {
            id: id.to_string(),
            kind: "external-bridge-plugin".to_string(),
            enabled: true,
            params,
            dsp: InsertDspState::default(),
            vst3: None,
            callback_process_log_done: false,
            silent_process_blocks: 0,
            bridge_missed_blocks: 0,
            scratch_l: vec![0.0; 8],
            scratch_r: vec![0.0; 8],
        }
    }

    #[test]
    fn serial_bridge_effect_chain_applies_both_gains() {
        #[derive(Debug)]
        struct MultSink {
            mult: f32,
            done: AtomicU64,
            buf_l: std::sync::Mutex<Vec<f32>>,
            buf_r: std::sync::Mutex<Vec<f32>>,
        }

        impl PluginBridgeSink for MultSink {
            fn dsp_ready(&self) -> bool {
                true
            }
            fn read_output(&self, out_l: &mut [f32], out_r: &mut [f32], frames: usize) -> usize {
                let done = self.done.load(Ordering::Acquire);
                if done == 0 {
                    return 0;
                }
                self.done.store(0, Ordering::Release);
                let bl = self.buf_l.lock().unwrap();
                let br = self.buf_r.lock().unwrap();
                let n = frames.min(out_l.len()).min(out_r.len()).min(bl.len());
                for i in 0..n {
                    out_l[i] = bl[i] * self.mult;
                    out_r[i] = br[i] * self.mult;
                }
                n
            }
            fn push_midi(&self, _: u8, _: u8, _: u8, _: u32) {}
            fn write_input(&self, in_l: &[f32], in_r: &[f32], frames: usize) {
                let n = frames.min(in_l.len()).min(in_r.len());
                *self.buf_l.lock().unwrap() = in_l[..n].to_vec();
                *self.buf_r.lock().unwrap() = in_r[..n].to_vec();
            }
            fn request_block(&self, _: u32) {
                self.done.store(1, Ordering::Release);
            }
        }

        let mut track = RuntimeTrack {
            id: "track-1".to_string(),
            track_type: "audio".to_string(),
            volume: 1.0,
            pan: 0.0,
            muted: false,
            solo: false,
            record_armed: false,
            monitor_enabled: false,
            input_source: crate::runtime::RuntimeTrackInputSource::None,
            preview_mode: RuntimePreviewMode::Stereo,
            output_track_id: None,
            inserts: vec![
                bridge_effect_track_with_id("insert-a"),
                bridge_effect_track_with_id("insert-b"),
            ],
            sends: Vec::new(),
            automation_lanes: Vec::new(),
            meter: Arc::new(Default::default()),
            meter_peak_l: 0.0,
            meter_peak_r: 0.0,
            meter_sum_sq_l: 0.0,
            meter_sum_sq_r: 0.0,
            callback_insert_log_done: false,
            callback_clip_route_log_done: false,
            block_l: vec![1.0; 8],
            block_r: vec![1.0; 8],
            recv_l: vec![0.0; 8],
            recv_r: vec![0.0; 8],
            midi_block_events: Vec::new(),
            midi_instrument_insert_ix: None,
            plugin_latency_samples: 0,
            pdc_delay_l: Vec::new(),
            pdc_delay_r: Vec::new(),
            pdc_write_pos: 0,
        };
        let sink_a = Arc::new(MultSink {
            mult: 2.0,
            done: AtomicU64::new(1),
            buf_l: std::sync::Mutex::new(vec![0.0; 8]),
            buf_r: std::sync::Mutex::new(vec![0.0; 8]),
        });
        let sink_b = Arc::new(MultSink {
            mult: 3.0,
            done: AtomicU64::new(1),
            buf_l: std::sync::Mutex::new(vec![0.0; 8]),
            buf_r: std::sync::Mutex::new(vec![0.0; 8]),
        });
        let mut sinks = std::collections::HashMap::new();
        sinks.insert("insert-a".to_string(), sink_a as Arc<dyn PluginBridgeSink>);
        sinks.insert("insert-b".to_string(), sink_b as Arc<dyn PluginBridgeSink>);
        apply_track_chain_block(&mut track, 4, &sinks, RuntimeTransportContext::default());
        assert!(
            (track.block_l[0] - 6.0).abs() < 1e-4,
            "serial chain expected 1*2*3=6 got {}",
            track.block_l[0]
        );
    }

    #[test]
    fn serial_bridge_effect_chain_bypasses_only_missed_middle_insert() {
        #[derive(Debug)]
        struct MultSink {
            mult: f32,
            done: AtomicU64,
            buf_l: std::sync::Mutex<Vec<f32>>,
            buf_r: std::sync::Mutex<Vec<f32>>,
        }

        impl PluginBridgeSink for MultSink {
            fn dsp_ready(&self) -> bool {
                true
            }
            fn read_output(&self, out_l: &mut [f32], out_r: &mut [f32], frames: usize) -> usize {
                let done = self.done.load(Ordering::Acquire);
                if done == 0 {
                    return 0;
                }
                self.done.store(0, Ordering::Release);
                let bl = self.buf_l.lock().unwrap();
                let br = self.buf_r.lock().unwrap();
                let n = frames.min(out_l.len()).min(out_r.len()).min(bl.len());
                for i in 0..n {
                    out_l[i] = bl[i] * self.mult;
                    out_r[i] = br[i] * self.mult;
                }
                n
            }
            fn push_midi(&self, _: u8, _: u8, _: u8, _: u32) {}
            fn write_input(&self, in_l: &[f32], in_r: &[f32], frames: usize) {
                let n = frames.min(in_l.len()).min(in_r.len());
                *self.buf_l.lock().unwrap() = in_l[..n].to_vec();
                *self.buf_r.lock().unwrap() = in_r[..n].to_vec();
            }
            fn request_block(&self, _: u32) {
                self.done.store(1, Ordering::Release);
            }
        }

        #[derive(Debug)]
        struct MissSink;

        impl PluginBridgeSink for MissSink {
            fn dsp_ready(&self) -> bool {
                true
            }
            fn read_output(&self, _: &mut [f32], _: &mut [f32], _: usize) -> usize {
                0
            }
            fn push_midi(&self, _: u8, _: u8, _: u8, _: u32) {}
            fn write_input(&self, _: &[f32], _: &[f32], _: usize) {}
            fn request_block(&self, _: u32) {}
        }

        let mut track = RuntimeTrack {
            id: "track-1".to_string(),
            track_type: "audio".to_string(),
            volume: 1.0,
            pan: 0.0,
            muted: false,
            solo: false,
            record_armed: false,
            monitor_enabled: false,
            input_source: crate::runtime::RuntimeTrackInputSource::None,
            preview_mode: RuntimePreviewMode::Stereo,
            output_track_id: None,
            inserts: vec![
                bridge_effect_track_with_id("insert-a"),
                bridge_effect_track_with_id("insert-b"),
                bridge_effect_track_with_id("insert-c"),
            ],
            sends: Vec::new(),
            automation_lanes: Vec::new(),
            meter: Arc::new(Default::default()),
            meter_peak_l: 0.0,
            meter_peak_r: 0.0,
            meter_sum_sq_l: 0.0,
            meter_sum_sq_r: 0.0,
            callback_insert_log_done: false,
            callback_clip_route_log_done: false,
            block_l: vec![1.0; 8],
            block_r: vec![1.0; 8],
            recv_l: vec![0.0; 8],
            recv_r: vec![0.0; 8],
            midi_block_events: Vec::new(),
            midi_instrument_insert_ix: None,
            plugin_latency_samples: 0,
            pdc_delay_l: Vec::new(),
            pdc_delay_r: Vec::new(),
            pdc_write_pos: 0,
        };
        let sink_a = Arc::new(MultSink {
            mult: 2.0,
            done: AtomicU64::new(1),
            buf_l: std::sync::Mutex::new(vec![0.0; 8]),
            buf_r: std::sync::Mutex::new(vec![0.0; 8]),
        });
        let sink_b = Arc::new(MissSink);
        let sink_c = Arc::new(MultSink {
            mult: 5.0,
            done: AtomicU64::new(1),
            buf_l: std::sync::Mutex::new(vec![0.0; 8]),
            buf_r: std::sync::Mutex::new(vec![0.0; 8]),
        });
        let mut sinks = std::collections::HashMap::new();
        sinks.insert("insert-a".to_string(), sink_a as Arc<dyn PluginBridgeSink>);
        sinks.insert("insert-b".to_string(), sink_b as Arc<dyn PluginBridgeSink>);
        sinks.insert("insert-c".to_string(), sink_c as Arc<dyn PluginBridgeSink>);
        apply_track_chain_block(&mut track, 4, &sinks, RuntimeTransportContext::default());
        assert!(
            (track.block_l[0] - 10.0).abs() < 1e-4,
            "A x2, B missed, C x5 expected 1*2*5=10 got {}",
            track.block_l[0]
        );
    }
}

#[cfg(test)]
mod routing_tests {
    use super::*;
    use crate::runtime::{
        volume_db_to_norm, RuntimeAutomationCurve, RuntimeAutomationLane, RuntimeAutomationPoint,
        RuntimeAutomationTarget, RuntimePreviewMode, RuntimeProject, RuntimeSend, RuntimeTrack,
    };
    use std::sync::Arc;

    fn track(id: &str, ty: &str, sends: Vec<RuntimeSend>) -> RuntimeTrack {
        let cap = 8;
        RuntimeTrack {
            id: id.to_string(),
            track_type: ty.to_string(),
            volume: 1.0,
            pan: 0.0,
            muted: false,
            solo: false,
            record_armed: false,
            monitor_enabled: false,
            input_source: crate::runtime::RuntimeTrackInputSource::None,
            preview_mode: RuntimePreviewMode::Stereo,
            output_track_id: None,
            inserts: Vec::new(),
            sends,
            automation_lanes: Vec::new(),
            meter: Arc::new(Default::default()),
            meter_peak_l: 0.0,
            meter_peak_r: 0.0,
            meter_sum_sq_l: 0.0,
            meter_sum_sq_r: 0.0,
            callback_insert_log_done: false,
            callback_clip_route_log_done: false,
            block_l: vec![0.0; cap],
            block_r: vec![0.0; cap],
            recv_l: vec![0.0; cap],
            recv_r: vec![0.0; cap],
            midi_block_events: Vec::new(),
            midi_instrument_insert_ix: None,
            plugin_latency_samples: 0,
            pdc_delay_l: Vec::new(),
            pdc_delay_r: Vec::new(),
            pdc_write_pos: 0,
        }
    }

    fn send(target: &str, level: f32) -> RuntimeSend {
        RuntimeSend {
            id: format!("send-{target}"),
            return_track_id: target.to_string(),
            level,
            enabled: true,
            pre_fader: false,
        }
    }

    fn automation_lane(
        target: RuntimeAutomationTarget,
        value: f32,
        enabled: bool,
    ) -> RuntimeAutomationLane {
        RuntimeAutomationLane {
            id: "auto-1".to_string(),
            name: "Automation".to_string(),
            target,
            enabled,
            points: vec![RuntimeAutomationPoint {
                beat: 0.0,
                value,
                curve: RuntimeAutomationCurve::Linear,
            }],
        }
    }

    #[test]
    fn track_volume_and_pan_automation_affect_fader_output() {
        let mut t = track("audio", "audio", vec![]);
        t.volume = 0.25;
        t.pan = 0.0;
        t.automation_lanes = vec![
            automation_lane(
                RuntimeAutomationTarget::TrackVolume,
                volume_db_to_norm(0.0),
                true,
            ),
            automation_lane(RuntimeAutomationTarget::TrackPan, 1.0, true),
        ];

        let (l, r) = apply_track_chain_at_beat(1.0, 1.0, &mut t, 0.0);

        assert!(l.abs() < 1e-6, "full-right pan should mute left");
        assert!(
            (r - 1.0).abs() < 1e-6,
            "0 dB automation should override base volume"
        );
    }

    #[test]
    fn track_mute_automation_overrides_base_mute_state() {
        let mut t = track("audio", "audio", vec![]);
        assert!(!effective_track_muted(&t, 0.0));

        t.automation_lanes = vec![automation_lane(
            RuntimeAutomationTarget::TrackMute,
            1.0,
            true,
        )];
        assert!(effective_track_muted(&t, 0.0));

        t.automation_lanes[0].enabled = false;
        assert!(!effective_track_muted(&t, 0.0));
    }

    #[test]
    fn send_to_return_accumulates_scaled() {
        let frames = 4;
        let mut p = RuntimeProject {
            tracks: vec![
                track("audio", "audio", vec![send("ret", 0.5)]),
                track("ret", "return", vec![]),
            ],
            ..Default::default()
        };
        // Source post-fader signal (accumulate_sends reads block_*).
        p.tracks[0].block_l[..frames].fill(1.0);
        p.tracks[0].block_r[..frames].fill(-2.0);

        accumulate_sends(&mut p, 0, frames, false);

        assert!(p.tracks[1].recv_l[..frames]
            .iter()
            .all(|&v| (v - 0.5).abs() < 1e-6));
        assert!(p.tracks[1].recv_r[..frames]
            .iter()
            .all(|&v| (v + 1.0).abs() < 1e-6));
    }

    #[test]
    fn pre_fader_filter_only_routes_matching_phase() {
        let frames = 4;
        let mut pre = send("ret", 1.0);
        pre.pre_fader = true;
        let mut p = RuntimeProject {
            tracks: vec![
                track("audio", "audio", vec![pre]),
                track("ret", "return", vec![]),
            ],
            ..Default::default()
        };
        p.tracks[0].block_l[..frames].fill(1.0);

        // Post-fader phase: the pre-fader send must NOT route.
        accumulate_sends(&mut p, 0, frames, false);
        assert!(p.tracks[1].recv_l[..frames].iter().all(|&v| v == 0.0));

        // Pre-fader phase: now it routes.
        accumulate_sends(&mut p, 0, frames, true);
        assert!(p.tracks[1].recv_l[..frames]
            .iter()
            .all(|&v| (v - 1.0).abs() < 1e-6));
    }

    #[test]
    fn send_to_non_routing_target_is_rejected() {
        let frames = 4;
        let mut p = RuntimeProject {
            tracks: vec![
                track("a", "audio", vec![send("b", 1.0)]),
                track("b", "audio", vec![]),
            ],
            ..Default::default()
        };
        p.tracks[0].block_l[..frames].fill(1.0);
        accumulate_sends(&mut p, 0, frames, false);
        // Target is a normal audio track → not a valid send destination.
        assert!(p.tracks[1].recv_l[..frames].iter().all(|&v| v == 0.0));
    }

    #[test]
    fn routing_to_earlier_routing_is_rejected_as_cycle_unsafe() {
        let frames = 4;
        // bus "early" at index 0 sends to "late" at index 1 (forward → OK),
        // and "late" sends back to "early" (backward → rejected).
        let mut p = RuntimeProject {
            tracks: vec![
                track("early", "bus", vec![send("late", 1.0)]),
                track("late", "return", vec![send("early", 1.0)]),
            ],
            ..Default::default()
        };
        p.tracks[0].block_l[..frames].fill(1.0);
        p.tracks[1].block_l[..frames].fill(1.0);

        accumulate_sends(&mut p, 0, frames, false); // early → late: forward, accepted
        accumulate_sends(&mut p, 1, frames, false); // late → early: backward, rejected

        assert!(p.tracks[1].recv_l[..frames]
            .iter()
            .all(|&v| (v - 1.0).abs() < 1e-6));
        assert!(p.tracks[0].recv_l[..frames].iter().all(|&v| v == 0.0));
    }

    #[test]
    fn main_output_to_bus_routes_into_bus_receive_not_master() {
        let frames = 4;
        let channels = 2;
        let mut a = track("a", "audio", vec![]);
        a.output_track_id = Some("bus".to_string());
        let mut p = RuntimeProject {
            tracks: vec![a, track("bus", "bus", vec![])],
            ..Default::default()
        };
        p.tracks[0].block_l[..frames].fill(0.8);
        p.tracks[0].block_r[..frames].fill(-0.4);

        let mut output = vec![0.0f32; frames * channels];
        route_main_output(&mut p, 0, frames, &mut output, channels);

        // The post-fader block went into the bus receive buffers …
        assert!(p.tracks[1].recv_l[..frames]
            .iter()
            .all(|&v| (v - 0.8).abs() < 1e-6));
        assert!(p.tracks[1].recv_r[..frames]
            .iter()
            .all(|&v| (v + 0.4).abs() < 1e-6));
        // … and NOT into the master output.
        assert!(output.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn main_output_to_master_sums_into_output() {
        let frames = 4;
        let channels = 2;
        let mut p = RuntimeProject {
            tracks: vec![track("a", "audio", vec![])], // output_track_id = None → master
            ..Default::default()
        };
        p.tracks[0].block_l[..frames].fill(0.5);
        p.tracks[0].block_r[..frames].fill(0.25);

        let mut output = vec![0.0f32; frames * channels];
        route_main_output(&mut p, 0, frames, &mut output, channels);

        for f in 0..frames {
            assert!((output[f * channels] - 0.5).abs() < 1e-6);
            assert!((output[f * channels + 1] - 0.25).abs() < 1e-6);
        }
    }

    #[test]
    fn main_output_to_non_routing_track_falls_back_to_master() {
        let frames = 4;
        let channels = 2;
        let mut a = track("a", "audio", vec![]);
        a.output_track_id = Some("b".to_string()); // "b" is a plain audio track
        let mut p = RuntimeProject {
            tracks: vec![a, track("b", "audio", vec![])],
            ..Default::default()
        };
        p.tracks[0].block_l[..frames].fill(1.0);
        p.tracks[0].block_r[..frames].fill(1.0);

        let mut output = vec![0.0f32; frames * channels];
        route_main_output(&mut p, 0, frames, &mut output, channels);

        // Not a routing target → summed to master, "b" untouched.
        assert!(output.iter().all(|&v| (v - 1.0).abs() < 1e-6));
        assert!(p.tracks[1].recv_l[..frames].iter().all(|&v| v == 0.0));
    }
}
