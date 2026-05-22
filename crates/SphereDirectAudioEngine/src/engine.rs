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
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{BufferSize, FromSample, Sample, SampleFormat, SizedSample};
use crossbeam_channel::{bounded, Receiver, Sender};
use parking_lot::Mutex;
use sphere_audio_plugins::{canonical_plugin_id, process_stereo_sample};

use crate::audio_file::AudioFileBuffer;
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
use crate::recording::{self, RecordingSession};
use crate::runtime::{RuntimeInsert, RuntimePreviewMode, RuntimeProject, RuntimeTrack};
use crate::types::{
    EngineProjectSnapshot, EngineStatus, JsAudioDeviceInfo, JsDauxBackendInfo, JsDauxConfig,
    JsDauxStatus, JsDeviceOpenConfig, JsEngineDebugInfo, JsMeterSnapshot, JsRecordingResult,
    JsRecordingStatus, JsSphereAudioStatus, JsStartRecordingConfig, JsTrackMeterSnapshot,
};

// ── Version ───────────────────────────────────────────────────────────────────

pub const ENGINE_VERSION: &str = "0.1.0";

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
    pub position_samples: AtomicU64, // samples elapsed from start
    pub sample_rate: AtomicU32,

    // DAUx diagnostics (incremented by audio thread, read by control thread)
    pub glitch_count: AtomicU64,
    pub mmcss_active: AtomicBool,
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
            position_samples: AtomicU64::new(0),
            sample_rate: AtomicU32::new(44100),
            glitch_count: AtomicU64::new(0),
            mmcss_active: AtomicBool::new(false),
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
    fn sample_rate(&self) -> u32 {
        match self {
            ActiveStream::Cpal(h) => h.sample_rate,
            #[cfg(target_os = "windows")]
            ActiveStream::WasapiExclusive(h) => h.sample_rate,
        }
    }
    fn buffer_size(&self) -> u32 {
        match self {
            ActiveStream::Cpal(h) => h.buffer_size,
            #[cfg(target_os = "windows")]
            ActiveStream::WasapiExclusive(h) => h.buffer_size,
        }
    }
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
    master: Mutex<MasterState>,

    // Last loaded project snapshot (optional, for reference/debugging).
    project: Mutex<Option<EngineProjectSnapshot>>,

    // Prepared render graph shared with new streams and pushed to callbacks.
    runtime: Mutex<RuntimeProject>,
    audio_cache: Mutex<HashMap<String, Arc<AudioFileBuffer>>>,

    // DAUx config & glitch counter (shared with audio thread for diagnostics).
    glitch_counter: Arc<AtomicU64>,
    daux_config: Mutex<DauxDeviceConfig>,

    // Active recording session (None when not recording).
    recording: Mutex<Option<RecordingSession>>,
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
        }
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
                    RuntimeProject::build(
                        snapshot,
                        candidate.config.sample_rate.0,
                        &mut audio_cache,
                        None,
                    )
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
        // Try DAUx active stream first.
        if let Some(stream) = self.active_stream.lock().as_ref() {
            stream.play().map_err(SphereAudioError::StreamStartFailed)?;
            self.shared.playing.store(false, Ordering::Relaxed); // transport starts paused
            self.status.lock().running = true;
            return Ok(());
        }
        // Legacy cpal path.
        let guard = self.stream.lock();
        let stream = guard.as_ref().ok_or(SphereAudioError::EngineNotOpen)?;
        stream
            .play()
            .map_err(|e| SphereAudioError::StreamStartFailed(e.to_string()))?;
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

    // ── Transport ──────────────────────────────────────────────────────────

    pub fn play(&self) -> Result<(), SphereAudioError> {
        self.send_command(EngineCommand::StartTransport)
    }

    pub fn pause(&self) -> Result<(), SphereAudioError> {
        self.send_command(EngineCommand::StopTransport)
    }

    pub fn seek(&self, position_seconds: f64) -> Result<(), SphereAudioError> {
        self.send_command(EngineCommand::Seek { position_seconds })
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
        let Some(insert) = track.inserts.iter_mut().find(|insert| insert.id == insert_id) else {
            return Err(SphereAudioError::NativeError(format!(
                "insert not found for editor: track={track_id} insert={insert_id}"
            )));
        };
        let Some(vst3) = insert.vst3.as_mut() else {
            return Err(SphereAudioError::NativeError(format!(
                "insert has no ready VST3 processor: track={track_id} insert={insert_id}"
            )));
        };
        let handle = vst3.open_editor(window_id, title, width, height).ok_or_else(|| {
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
            .and_then(|track| track.inserts.iter_mut().find(|insert| insert.id == insert_id))
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
            .and_then(|track| track.inserts.iter_mut().find(|insert| insert.id == insert_id))
            .and_then(|insert| insert.vst3.as_mut())
        {
            return Ok(vst3.focus_editor());
        }
        Ok(false)
    }

    // ── Project snapshot ───────────────────────────────────────────────────

    pub fn load_project(&self, snapshot: EngineProjectSnapshot) -> Result<(), SphereAudioError> {
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
            let project = RuntimeProject::build(
                &snapshot,
                output_sample_rate,
                &mut audio_cache,
                Some(&mut existing_vst3),
            );

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
        };
        // Processors left in existing_vst3 had no matching insert in the new
        // snapshot — they are dropped here with reason="replaced-by-load-project".
        drop(existing_vst3);

        eprintln!(
            "[SphereAudio] RuntimeProject built: {} runtime clips from {} snapshot clips (sr={})",
            runtime.clips.len(),
            snapshot.clips.len(),
            output_sample_rate,
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
        *self.project.lock() = Some(snapshot.clone());
        *self.runtime.lock() = runtime.clone();

        for t in &snapshot.tracks {
            let track_clips = runtime.clips.iter().filter(|c| c.track_id == t.id).count();
            eprintln!(
                "[SphereAudio] track '{}' type={} clips={} volume={:.2} pan={:.2} muted={} solo={}",
                t.id, t.track_type, track_clips, t.volume, t.pan, t.muted, t.solo
            );
        }
        match self.send_command(EngineCommand::LoadProject(runtime)) {
            Ok(()) => eprintln!("[SphereAudio] LoadProject command sent to audio callback"),
            Err(SphereAudioError::EngineNotOpen) => {
                eprintln!(
                    "[SphereAudio] ⚠ WARNING: no audio stream open — runtime stored, \
                     will apply on next openDevice/openDaux"
                );
            }
            Err(e) => return Err(e),
        }

        Ok(())
    }

    // ── Debug info ─────────────────────────────────────────────────────────

    pub fn get_debug_info(&self) -> JsEngineDebugInfo {
        let runtime = self.runtime.lock();
        let project = self.project.lock();
        let sample_rate = self.shared.sample_rate.load(Ordering::Relaxed).max(1);
        let position_samples = self.shared.position_samples.load(Ordering::Relaxed);

        let ready_clips = runtime.clips.iter().filter(|c| c.source.frames > 0).count() as u32;
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
                    c.source.frames,
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
                        "track={} insert={} kind={} format={} enabled={} vst3Ready={} processCount={} inPeak={:.4} outPeak={:.4} diffPeak={:.6} path={}",
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

        JsEngineDebugInfo {
            project_id: project.as_ref().map(|p| p.project_id.clone()),
            loaded_tracks: runtime.tracks.len() as u32,
            loaded_clips: runtime.clips.len() as u32,
            ready_clips,
            is_playing: self.shared.playing.load(Ordering::Relaxed),
            position_seconds: position_samples as f64 / sample_rate as f64,
            has_solo: runtime.has_solo,
            clip_summaries,
            insert_summaries,
        }
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
        let session = recording::start_recording(config)?;
        *guard = Some(session);
        Ok(())
    }

    /// Stop the active recording, finalize WAV files, and return per-track results.
    pub fn stop_recording(&self) -> Result<Vec<JsRecordingResult>, SphereAudioError> {
        let session = self.recording.lock().take().ok_or_else(|| {
            SphereAudioError::NativeError("No active recording session".to_string())
        })?;
        recording::stop_recording(session)
    }

    /// Snapshot of current recording state (for UI status polling).
    pub fn get_recording_status(&self) -> JsRecordingStatus {
        match self.recording.lock().as_ref() {
            None => JsRecordingStatus::default(),
            Some(s) => {
                // We don't track elapsed time with an atomic — returning track_count
                // is sufficient for the UI to show a "recording" badge.
                JsRecordingStatus {
                    active: true,
                    duration_seconds: 0.0,
                    track_count: s.track_count as u32,
                }
            }
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

        // Clear any previous error on success.
        self.status.lock().last_daux_error = None;
        *self.active_stream.lock() = Some(stream);
        Ok(())
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

    /// Return the current DAUx status (backend, device, latency, glitches).
    pub fn get_daux_status(&self) -> JsDauxStatus {
        let st = self.status.lock().clone();
        let daux_cfg = self.daux_config.lock().clone();
        let glitch_count = self.glitch_counter.load(Ordering::Relaxed) as f64;
        let mmcss_active = self.shared.mmcss_active.load(Ordering::Relaxed);

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
        }
    }

    // ── Internal helpers ───────────────────────────────────────────────────

    fn get_initial_runtime(&self, sample_rate_override: Option<u32>) -> RuntimeProject {
        let sr = sample_rate_override
            .unwrap_or_else(|| self.shared.sample_rate.load(Ordering::Relaxed).max(44100));

        self.project
            .lock()
            .as_ref()
            .map(|snapshot| {
                let mut audio_cache = self.audio_cache.lock();
                // Pass None for existing_vst3: opening a new device may change
                // the sample rate, so processors cannot be safely reused here.
                RuntimeProject::build(snapshot, sr, &mut audio_cache, None)
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

    fn send_command(&self, cmd: EngineCommand) -> Result<(), SphereAudioError> {
        // Prefer active_stream (DAUx path); fall back to legacy cmd_tx.
        if let Some(stream) = self.active_stream.lock().as_ref() {
            if let Some(tx) = stream.cmd_tx() {
                return tx
                    .try_send(cmd)
                    .map_err(|e| SphereAudioError::NativeError(e.to_string()));
            }
        }
        let guard = self.cmd_tx.lock();
        match guard.as_ref() {
            Some(tx) => tx
                .try_send(cmd)
                .map_err(|e| SphereAudioError::NativeError(e.to_string())),
            None => Err(SphereAudioError::EngineNotOpen),
        }
    }
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

    for clip_index in 0..runtime.clips.len() {
        let clip = &runtime.clips[clip_index];
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
        let source = Arc::clone(&clip.source);

        let Some(track_index) = runtime.tracks.iter().position(|t| t.id == clip_track_id) else {
            continue;
        };
        if Some(track_index) == master_index {
            continue;
        }
        let has_solo = runtime.has_solo;
        if runtime.tracks[track_index].muted || (has_solo && !runtime.tracks[track_index].solo) {
            continue;
        }

        let source_pos_seconds = clip_offset_seconds
            + (rel as f64 / runtime.sample_rate.max(1) as f64) * clip_speed_ratio as f64;
        let source_pos = source_pos_seconds * source.sample_rate as f64;
        let (mut l, mut r) = sample_source_stereo(&source, source_pos);
        if l == 0.0 && r == 0.0 {
            continue;
        }

        l *= clip_gain;
        r *= clip_gain;

        let output_track_id = runtime.tracks[track_index].output_track_id.clone();
        let sends = runtime.tracks[track_index].sends.clone();
        let (track_l, track_r) = apply_track_chain(l, r, &mut runtime.tracks[track_index]);
        let (track_l, track_r) =
            apply_preview_mode(track_l, track_r, runtime.tracks[track_index].preview_mode);
        runtime.accumulate_track_meter(track_index, track_l, track_r);

        if let Some(target_id) = output_track_id
            .as_deref()
            .filter(|id| !is_master_output(id))
        {
            if let Some(target_index) = runtime.tracks.iter().position(|t| t.id == target_id) {
                let (bus_l, bus_r) =
                    apply_track_chain(track_l, track_r, &mut runtime.tracks[target_index]);
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
            if return_track.muted || (runtime.has_solo && !return_track.solo) {
                continue;
            }
            let (send_l, send_r) = apply_track_chain(
                track_l * send.level,
                track_r * send.level,
                &mut runtime.tracks[return_track_index],
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
        let muted = runtime.tracks[m_idx].muted
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
        (out_l * master_volume).clamp(-1.0, 1.0),
        (out_r * master_volume).clamp(-1.0, 1.0),
    )
}

pub fn render_project_block_interleaved(
    runtime: &mut RuntimeProject,
    base_sample: u64,
    master_volume: f32,
    output: &mut [f32],
    channels: usize,
) -> u64 {
    if channels < 2 {
        return 0;
    }
    let frames = output.len() / channels;
    if frames == 0 {
        return 0;
    }
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
        track.block_l[..frames].fill(0.0);
        track.block_r[..frames].fill(0.0);
    }

    let master_index = runtime.tracks.iter().position(|t| t.track_type == "master");

    for clip_index in 0..runtime.clips.len() {
        let clip = &runtime.clips[clip_index];
        let Some(track_index) = runtime.tracks.iter().position(|t| t.id == clip.track_id) else {
            continue;
        };
        if runtime.tracks[track_index].muted
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
            let source_pos = source_pos_seconds * source.sample_rate as f64;
            let (mut l, mut r) = sample_source_stereo(&source, source_pos);
            l *= clip.gain;
            r *= clip.gain;
            runtime.tracks[track_index].block_l[frame_idx] += l;
            runtime.tracks[track_index].block_r[frame_idx] += r;
        }
    }

    for track_index in 0..runtime.tracks.len() {
        if Some(track_index) == master_index {
            continue;
        }
        if runtime.tracks[track_index].muted
            || (runtime.has_solo && !runtime.tracks[track_index].solo)
        {
            continue;
        }
        if !runtime.tracks[track_index].inserts.is_empty()
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
            for clip in runtime.clips.iter().filter(|clip| clip.track_id == track_id) {
                let clip_start = clip.start_sample;
                let clip_end = clip.start_sample.saturating_add(clip.duration_samples);
                let overlaps = block_end > clip_start && block_start < clip_end;
                if clip_count == 0 {
                    first_clip = format!(
                        "{} range={}..{} offset={:.3}s gain={:.3} speed={:.3} overlaps={}",
                        clip.id, clip_start, clip_end, clip.offset_seconds, clip.gain, clip.speed_ratio, overlaps
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
        let track = &mut runtime.tracks[track_index];
        apply_track_chain_block(track, frames);
        let (pan_l, pan_r) = pan_gains(track.pan);
        for frame_idx in 0..frames {
            let (l, r) = apply_preview_mode(
                track.block_l[frame_idx] * track.volume * pan_l,
                track.block_r[frame_idx] * track.volume * pan_r,
                track.preview_mode,
            );
            track.meter_peak_l = track.meter_peak_l.max(l.abs());
            track.meter_peak_r = track.meter_peak_r.max(r.abs());
            track.meter_sum_sq_l += l * l;
            track.meter_sum_sq_r += r * r;
            let out = &mut output[frame_idx * channels..frame_idx * channels + channels];
            out[0] += l;
            out[1] += r;
        }
    }

    // ── Master bus: apply master track inserts on the summed output ──
    if let Some(m_idx) = master_index {
        let muted = runtime.tracks[m_idx].muted
            || (runtime.has_solo && !runtime.tracks[m_idx].solo);
        if !muted {
            let master = &mut runtime.tracks[m_idx];
            // Copy summed output into master scratch buffer.
            for i in 0..frames {
                let frame = &output[i * channels..i * channels + channels];
                master.block_l[i] = frame[0];
                master.block_r[i] = frame[1];
            }
            apply_track_chain_block(master, frames);
            // Write back, accumulate master meter, apply preview mode.
            for i in 0..frames {
                let (l, r) = apply_preview_mode(
                    master.block_l[i],
                    master.block_r[i],
                    master.preview_mode,
                );
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

    // Final master volume + clamp.
    for i in 0..frames {
        let out = &mut output[i * channels..i * channels + channels];
        out[0] = (out[0] * master_volume).clamp(-1.0, 1.0);
        out[1] = (out[1] * master_volume).clamp(-1.0, 1.0);
    }

    frames as u64
}

#[inline]
pub fn is_master_output(output: &str) -> bool {
    output.is_empty() || output == "master" || output == "none"
}

#[inline]
pub fn apply_track_chain(mut l: f32, mut r: f32, track: &mut RuntimeTrack) -> (f32, f32) {
    if !track.inserts.is_empty() && !track.callback_insert_log_done {
        track.callback_insert_log_done = true;
        eprintln!(
            "[SphereAudio callback] track={} inserts={}",
            track.id,
            track.inserts.len()
        );
    }
    for insert in &mut track.inserts {
        let processed = apply_insert(l, r, insert);
        l = processed.0;
        r = processed.1;
    }
    let (pan_l, pan_r) = pan_gains(track.pan);
    (l * track.volume * pan_l, r * track.volume * pan_r)
}

pub fn apply_track_chain_block(track: &mut RuntimeTrack, frames: usize) {
    if !track.inserts.is_empty() && !track.callback_insert_log_done {
        track.callback_insert_log_done = true;
        eprintln!(
            "[SphereAudio callback] track={} inserts={} blockFrames={}",
            track.id,
            track.inserts.len(),
            frames
        );
    }
    for insert in &mut track.inserts {
        apply_insert_block(&mut track.block_l[..frames], &mut track.block_r[..frames], insert);
    }
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

pub fn apply_insert_block(block_l: &mut [f32], block_r: &mut [f32], insert: &mut RuntimeInsert) {
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
    let before_peak_l = block_l.iter().fold(0.0f32, |peak, sample| peak.max(sample.abs()));
    let before_peak_r = block_r.iter().fold(0.0f32, |peak, sample| peak.max(sample.abs()));

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

    let handle = vst3.handle_value();
    let process_ok = vst3.process_stereo_block(
        &block_l[..frames],
        &block_r[..frames],
        &mut insert.scratch_l[..frames],
        &mut insert.scratch_r[..frames],
    );
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

#[inline]
pub fn sample_source_stereo(source: &crate::audio_file::AudioFileBuffer, pos: f64) -> (f32, f32) {
    if pos < 0.0 || source.frames == 0 {
        return (0.0, 0.0);
    }

    let idx = pos.floor() as usize;
    if idx >= source.frames {
        return (0.0, 0.0);
    }
    let frac = (pos - idx as f64) as f32;
    let next_idx = (idx + 1).min(source.frames - 1);

    let (l0, r0) = read_frame_stereo(source, idx);
    let (l1, r1) = read_frame_stereo(source, next_idx);

    (l0 + (l1 - l0) * frac, r0 + (r1 - r0) * frac)
}

#[inline]
pub fn read_frame_stereo(source: &crate::audio_file::AudioFileBuffer, frame: usize) -> (f32, f32) {
    let base = frame * source.channels;
    match source.channels {
        0 => (0.0, 0.0),
        1 => {
            let v = source.samples.get(base).copied().unwrap_or(0.0);
            (v, v)
        }
        _ => (
            source.samples.get(base).copied().unwrap_or(0.0),
            source.samples.get(base + 1).copied().unwrap_or(0.0),
        ),
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
                // ── 1. Drain command queue ───────────────────────────────────
                // Runs first so commands take effect from the start of this block.
                while let Ok(cmd) = cmd_rx.try_recv() {
                    match cmd {
                        EngineCommand::LoadProject(next_runtime) => {
                            eprintln!(
                                "[SphereAudio callback] LoadProject: {} tracks, {} clips (sr={})",
                                next_runtime.tracks.len(),
                                next_runtime.clips.len(),
                                output_sample_rate,
                            );
                            runtime = next_runtime;
                            runtime.sample_rate = output_sample_rate;
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
                            eprintln!(
                                "[SphereAudio callback] StartTransport: position={}sa ({:.3}s), active_clip_count={}, scheduled_clip_count={}",
                                pos,
                                pos as f64 / output_sample_rate as f64,
                                active_clips,
                                runtime.clips.len(),
                            );
                            playing_local = true;
                            shared.playing.store(true, Ordering::Relaxed);
                        }
                        EngineCommand::StopTransport => {
                            eprintln!("[SphereAudio callback] StopTransport");
                            playing_local = false;
                            shared.playing.store(false, Ordering::Relaxed);
                        }
                        EngineCommand::Seek { position_seconds } => {
                            let sr_local = shared.sample_rate.load(Ordering::Relaxed) as f64;
                            let pos = (position_seconds * sr_local) as u64;
                            eprintln!(
                                "[SphereAudio callback] Seek → {:.3}s ({}sa)",
                                position_seconds, pos
                            );
                            shared.position_samples.store(pos, Ordering::Relaxed);
                        }
                        EngineCommand::SetMasterVolume { value } => {
                            shared
                                .master_volume
                                .store(f32_store(value), Ordering::Relaxed);
                        }
                        EngineCommand::SetTrackVolume { track_id, value } => {
                            runtime.update_track_volume(&track_id, value);
                        }
                        EngineCommand::SetTrackPan { track_id, value } => {
                            runtime.update_track_pan(&track_id, value);
                        }
                        EngineCommand::SetTrackMute { track_id, muted } => {
                            eprintln!("[SphereAudio callback] SetTrackMute track={track_id} muted={muted}");
                            runtime.update_track_mute(&track_id, muted);
                        }
                        EngineCommand::SetTrackSolo { track_id, solo } => {
                            runtime.update_track_solo(&track_id, solo);
                        }
                        EngineCommand::SetTrackPreviewMode { track_id, value } => {
                            runtime.update_track_preview_mode(&track_id, RuntimePreviewMode::from_code(value));
                        }
                        EngineCommand::SetInsertParam { track_id, insert_id, param_id, value } => {
                            runtime.update_insert_param(&track_id, &insert_id, &param_id, value);
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

                if ch >= 2 && playing_local {
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
                    );
                    if !render_path_logged {
                        render_path_logged = true;
                        eprintln!(
                            "[SphereAudio callback] renderPath=legacy-block frames={} channels={} tracks={}",
                            frames,
                            ch,
                            runtime.tracks.len()
                        );
                    }
                    if gen_tone {
                        for frame in scratch.chunks_mut(ch) {
                            let tone_l = osc_l.next_sample() * TEST_TONE_AMPLITUDE * master_vol;
                            let tone_r = osc_r.next_sample() * TEST_TONE_AMPLITUDE * master_vol;
                            frame[0] = (frame[0] + tone_l).clamp(-1.0, 1.0);
                            frame[1] = (frame[1] + tone_r).clamp(-1.0, 1.0);
                        }
                    }
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
                        let l = (tone_l + project_l).clamp(-1.0, 1.0);
                        let r = (tone_r + project_r).clamp(-1.0, 1.0);
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
                        let value = (tone + (project_l + project_r) * 0.5).clamp(-1.0, 1.0);
                        *sample = T::from_sample(value);
                        peak_l = peak_l.max(value.abs());
                        sum_sq_l += value * value;
                        frames += 1;
                    }
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
