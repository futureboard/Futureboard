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

use crate::audio_file::AudioFileBuffer;
use crate::command::EngineCommand;
use crate::device;
use crate::dsp::{meter::smooth_peak, oscillator::SineOscillator};
use crate::error::SphereAudioError;
use crate::graph::{MasterState, TrackState};
use crate::runtime::RuntimeProject;
use crate::types::{
    EngineProjectSnapshot, EngineStatus, JsAudioDeviceInfo, JsDeviceOpenConfig, JsEngineDebugInfo,
    JsMeterSnapshot, JsSphereAudioStatus,
};

// ── Version ───────────────────────────────────────────────────────────────────

pub const ENGINE_VERSION: &str = "0.1.0";

// ── Atomic helpers ─────────────────────────────────────────────────────────────

#[inline]
fn f32_store(v: f32) -> u32 {
    v.to_bits()
}
#[inline]
fn f32_load(v: u32) -> f32 {
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
        }
    }
}

// ── Engine inner ───────────────────────────────────────────────────────────────

pub struct EngineInner {
    // Shared atomic state
    pub shared: Arc<SharedState>,

    // Stream lifecycle (Mutex so we can close/reopen without rebuilding everything)
    stream: Mutex<Option<cpal::Stream>>,

    // Command channel — Sender lives here; Receiver is moved into the callback.
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
            stream: Mutex::new(None),
            cmd_tx: Mutex::new(None),
            status: Mutex::new(EngineStatus::default()),
            tracks: Mutex::new(Vec::new()),
            master: Mutex::new(MasterState::default()),
            project: Mutex::new(None),
            runtime: Mutex::new(RuntimeProject::default()),
            audio_cache: Mutex::new(HashMap::new()),
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
        // Drop stream first — this stops the callback.
        *self.stream.lock() = None;
        *self.cmd_tx.lock() = None;

        let mut st = self.status.lock();
        st.stream_open = false;
        st.running = false;
    }

    /// Start audio playback (calls `stream.play()`).
    pub fn start(&self) -> Result<(), SphereAudioError> {
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
        if let Some(stream) = self.stream.lock().as_ref() {
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
        JsMeterSnapshot {
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
        self.send_command(EngineCommand::SetInsertParam {
            track_id: track_id.into(),
            insert_id: insert_id.into(),
            param_id: param_id.into(),
            value: value as f32,
        })
    }

    // ── Project snapshot ───────────────────────────────────────────────────

    pub fn load_project(&self, snapshot: EngineProjectSnapshot) -> Result<(), SphereAudioError> {
        let output_sample_rate = self.shared.sample_rate.load(Ordering::Relaxed).max(1);

        // Log how many clips have paths before building runtime.
        let clips_with_path = snapshot
            .clips
            .iter()
            .filter(|c| c.media_path.as_deref().map(|p| !p.is_empty()).unwrap_or(false))
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

        let runtime = {
            let mut audio_cache = self.audio_cache.lock();
            let project = RuntimeProject::build(&snapshot, output_sample_rate, &mut audio_cache);

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

        if self.cmd_tx.lock().is_some() {
            self.send_command(EngineCommand::LoadProject(runtime))?;
            eprintln!("[SphereAudio] LoadProject command sent to audio callback");
        } else {
            eprintln!("[SphereAudio] ⚠ WARNING: no audio stream open — LoadProject not sent to callback (will apply on next openDevice)");
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

        JsEngineDebugInfo {
            project_id: project.as_ref().map(|p| p.project_id.clone()),
            loaded_tracks: runtime.tracks.len() as u32,
            loaded_clips: runtime.clips.len() as u32,
            ready_clips,
            is_playing: self.shared.playing.load(Ordering::Relaxed),
            position_seconds: position_samples as f64 / sample_rate as f64,
            has_solo: runtime.has_solo,
            clip_summaries,
        }
    }

    // ── Internal helpers ───────────────────────────────────────────────────

    fn send_command(&self, cmd: EngineCommand) -> Result<(), SphereAudioError> {
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

const TEST_TONE_AMPLITUDE: f32 = 0.125; // −18 dBFS  (safe default test level)
const PEAK_DECAY: f32 = 0.94; // per audio block, responsive UI peak decay

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
fn render_project_sample(
    runtime: &RuntimeProject,
    project_sample: u64,
    master_volume: f32,
) -> (f32, f32) {
    let mut out_l = 0.0f32;
    let mut out_r = 0.0f32;

    for clip in &runtime.clips {
        if project_sample < clip.start_sample {
            continue;
        }
        let rel = project_sample - clip.start_sample;
        if rel >= clip.duration_samples {
            continue;
        }

        let Some(track) = runtime.tracks.iter().find(|t| t.id == clip.track_id) else {
            continue;
        };
        if track.muted || (runtime.has_solo && !track.solo) {
            continue;
        }

        let source_pos_seconds = clip.offset_seconds
            + (rel as f64 / runtime.sample_rate.max(1) as f64) * clip.speed_ratio as f64;
        let source_pos = source_pos_seconds * clip.source.sample_rate as f64;
        let (mut l, mut r) = sample_source_stereo(&clip.source, source_pos);
        if l == 0.0 && r == 0.0 {
            continue;
        }

        let (pan_l, pan_r) = pan_gains(track.pan);
        let gain = clip.gain * track.volume * master_volume;
        l *= gain * pan_l;
        r *= gain * pan_r;
        out_l += l;
        out_r += r;
    }

    (out_l.clamp(-1.0, 1.0), out_r.clamp(-1.0, 1.0))
}

#[inline]
fn pan_gains(pan: f32) -> (f32, f32) {
    let pan = pan.clamp(-1.0, 1.0);
    if pan < 0.0 {
        (1.0, 1.0 + pan)
    } else {
        (1.0 - pan, 1.0)
    }
}

#[inline]
fn sample_source_stereo(source: &crate::audio_file::AudioFileBuffer, pos: f64) -> (f32, f32) {
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
fn read_frame_stereo(source: &crate::audio_file::AudioFileBuffer, frame: usize) -> (f32, f32) {
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
                        EngineCommand::SetInsertParam { track_id, insert_id, param_id, value } => {
                            eprintln!(
                                "[SphereAudio callback] SetInsertParam track={track_id} insert={insert_id} {param_id}={value}"
                            );
                            // Insert DSP pass-through — params received but not yet applied.
                            // Audio continues to flow through the insert chain unaffected.
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

                if ch >= 2 {
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
                            render_project_sample(&runtime, base_sample + frames, master_vol)
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
                            render_project_sample(&runtime, base_sample + frames, master_vol)
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
