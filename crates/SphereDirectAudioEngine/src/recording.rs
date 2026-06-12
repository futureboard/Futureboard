//! Audio recording session management.
//!
//! # Architecture
//!
//! ```text
//! JS control thread
//!   └─ start_recording()
//!        ├─ opens cpal input stream  ──► input callback
//!        │                                 └─ try_recv(pool) + try_send(block) ──► bounded channel
//!        └─ spawns disk writer thread ──► recv(block) → RaufWriter → .rauf
//!
//! JS control thread
//!   └─ stop_recording()
//!        ├─ drop(input_stream)  →  channel closes  →  disk writer exits loop
//!        └─ recv(results)  →  return to caller
//! ```
//!
//! The audio callback does not write files, encode containers, or block on disk
//! I/O. The recording path uses a bounded preallocated block pool and drops on
//! backpressure instead of allocating or blocking.

use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    Arc,
};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::bounded;
use sphere_encoder::rauf::{RaufConfig, RaufSampleFormat, RaufWriter, RAUF_FLAG_HAS_SIDECAR};

use crate::error::SphereAudioError;
use crate::types::{JsRecordingResult, JsStartRecordingConfig};

// ── Unique recording counter for collision-free filenames ─────────────────────

static RECORD_COUNTER: AtomicU64 = AtomicU64::new(1);

// ── Internal types ────────────────────────────────────────────────────────────

struct TrackWriterState {
    track_id: String,
    track_name: String,
    writer: RaufWriter,
    /// 0-based indices into the interleaved input block to capture.
    input_channels: Vec<usize>,
    /// Number of channels written to the output RAUF (= input_channels.len()).
    out_channels: u16,
    final_path: PathBuf,
    relative_path: String,
    sidecar_path: PathBuf,
    sidecar_relative_path: String,
    take_id: String,
    project_start_sample: u64,
    error: Option<String>,
}

pub struct RecordingResult {
    pub track_id: String,
    pub file_path: String,
    pub relative_path: String,
    pub start_beat: f64,
    pub duration_seconds: f64,
    pub sample_rate: u32,
    pub channels: u32,
    pub metadata_path: String,
    pub sample_format: String,
    pub success: bool,
    pub error: Option<String>,
}

pub struct RecordingSession {
    /// Dropping this stops input capture and disconnects the audio channel.
    _input_stream: cpal::Stream,
    /// Receives finalized per-track results from the disk writer thread.
    pub results_rx: std::sync::mpsc::Receiver<Vec<RecordingResult>>,
    pub start_beat: f64,
    pub sample_rate: u32,
    pub track_count: usize,
    pub recording_active: Arc<AtomicBool>,
    pub dropped_blocks: Arc<AtomicU64>,
    pub started_at: std::time::Instant,
    pub shared: Arc<crate::engine::SharedState>,
}

// Safety: cpal::Stream is !Send due to a PhantomData marker on Windows (COM
// thread affinity).  We only access RecordingSession from the JS/control thread
// under a parking_lot::Mutex — never from the audio thread.
unsafe impl Send for RecordingSession {}
unsafe impl Sync for RecordingSession {}

// ── Filename helpers ──────────────────────────────────────────────────────────

fn sanitize_filename(name: &str) -> String {
    let safe: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect();
    if safe.trim().is_empty() {
        "Recording".to_string()
    } else {
        safe.trim().to_string()
    }
}

/// Returns a unique path inside `dir` that does not already exist.
///
/// Filename contract:
/// `{ProjectName}-{timestamp}-{takenumber}.{ext}`
fn unique_recording_path(
    dir: &Path,
    project_name: &str,
    timestamp: &str,
    extension: &str,
) -> PathBuf {
    let project_name = sanitize_filename(project_name);
    let timestamp = sanitize_filename(timestamp);
    let extension = extension.trim_start_matches('.').trim();
    let extension = if extension.is_empty() {
        "rauf"
    } else {
        extension
    };

    loop {
        let n = RECORD_COUNTER.fetch_add(1, Ordering::Relaxed);
        // Zero-pad to 4 digits so alphabetical sort matches recording order.
        let filename = format!("{project_name}-{timestamp}-{n:04}.{extension}");
        let path = dir.join(filename);
        if !path.exists() {
            return path;
        }
    }
}

fn make_take_id(session_id: &str, track_index: u64) -> [u8; 16] {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in session_id.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let counter = RECORD_COUNTER
        .load(Ordering::Relaxed)
        .wrapping_add(track_index);
    let mut id = [0u8; 16];
    id[0..8].copy_from_slice(&hash.to_le_bytes());
    id[8..16].copy_from_slice(&counter.to_le_bytes());
    id
}

fn format_take_id(take_id: [u8; 16]) -> String {
    let mut out = String::with_capacity(32);
    for byte in take_id {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

// ── Device lookup ─────────────────────────────────────────────────────────────

pub fn find_input_device(device_id: Option<&str>) -> Result<cpal::Device, SphereAudioError> {
    let host = cpal::default_host();
    if let Some(id) = device_id {
        if !id.is_empty() {
            let mut devices = host
                .input_devices()
                .map_err(|e| SphereAudioError::NativeError(e.to_string()))?;
            if let Some(dev) = devices.find(|d| d.name().as_deref().ok() == Some(id)) {
                return Ok(dev);
            }
            return Err(SphereAudioError::NativeError(format!(
                "Input device not found: '{id}'"
            )));
        }
    }
    host.default_input_device()
        .ok_or_else(|| SphereAudioError::NativeError("No default input device".to_string()))
}

// ── RAUF / disk writer thread ─────────────────────────────────────────────────

fn disk_writer_thread(
    audio_rx: crossbeam_channel::Receiver<Vec<i32>>,
    free_tx: crossbeam_channel::Sender<Vec<i32>>,
    mut writers: Vec<TrackWriterState>,
    sample_rate: u32,
    input_ch: usize, // channels per interleaved input frame
    start_beat: f64,
    finalize_tx: std::sync::mpsc::Sender<Vec<RecordingResult>>,
) {
    let mut total_frames = 0u64;

    // Drain audio blocks until the sender (input stream) disconnects.
    while let Ok(mut block) = audio_rx.recv() {
        let frames = block.len().checked_div(input_ch).unwrap_or(0);
        if frames == 0 {
            block.clear();
            let _ = free_tx.try_send(block);
            continue;
        }
        for w in &mut writers {
            let mut selected = Vec::with_capacity(frames * w.input_channels.len());
            for f in 0..frames {
                for &ch in &w.input_channels {
                    let s = if ch < input_ch {
                        block[f * input_ch + ch]
                    } else {
                        0
                    };
                    selected.push(s);
                }
            }
            if w.error.is_none() {
                if let Err(error) = w.writer.write_s32le_interleaved(&selected) {
                    w.error = Some(error.to_string());
                }
            }
        }
        total_frames += frames as u64;
        block.clear();
        let _ = free_tx.try_send(block);
    }

    let duration_seconds = if sample_rate > 0 {
        total_frames as f64 / sample_rate as f64
    } else {
        0.0
    };

    let mut results = Vec::with_capacity(writers.len());

    for mut w in writers {
        w.writer.set_flags(RAUF_FLAG_HAS_SIDECAR);
        let sidecar = RaufSidecarData {
            sidecar_path: w.sidecar_path.clone(),
            relative_path: w.relative_path.clone(),
            take_id: w.take_id.clone(),
            track_id: w.track_id.clone(),
            track_name: w.track_name.clone(),
            project_start_sample: w.project_start_sample,
            out_channels: w.out_channels,
        };
        let write_error = w.error.take();
        let finalized = w.writer.finalize();
        let frames_recorded = finalized
            .as_ref()
            .map(|report| report.frames_written)
            .unwrap_or(0);
        let sidecar_result =
            write_rauf_sidecar(&sidecar, sample_rate, frames_recorded, true, false);
        let ok = write_error.is_none() && finalized.is_ok() && sidecar_result.is_ok();
        let error = write_error.or_else(|| {
            finalized
                .err()
                .map(|error| error.to_string())
                .or_else(|| sidecar_result.err().map(|error| error.to_string()))
        });
        results.push(RecordingResult {
            track_id: w.track_id,
            file_path: w.final_path.to_string_lossy().into_owned(),
            relative_path: w.relative_path,
            start_beat,
            duration_seconds,
            sample_rate,
            channels: w.out_channels as u32,
            metadata_path: w.sidecar_relative_path,
            sample_format: "s32le".to_string(),
            success: ok,
            error,
        });
    }

    let _ = finalize_tx.send(results);
}

struct RaufSidecarData {
    sidecar_path: PathBuf,
    relative_path: String,
    take_id: String,
    track_id: String,
    track_name: String,
    project_start_sample: u64,
    out_channels: u16,
}

fn write_rauf_sidecar(
    w: &RaufSidecarData,
    sample_rate: u32,
    frames_recorded: u64,
    finalized: bool,
    recovered: bool,
) -> std::io::Result<()> {
    let audio_file = Path::new(&w.relative_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("take.rauf");
    let peak_file = format!(
        "{}.peak",
        Path::new(audio_file)
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("take")
    );
    let metadata = serde_json::json!({
        "format": "futureboard.rauf.sidecar",
        "version": 1,
        "audio_file": audio_file,
        "take_id": w.take_id,
        "track_id": w.track_id,
        "track_name": w.track_name,
        "record_mode": "live_input",
        "project_start_sample": w.project_start_sample,
        "sample_rate": sample_rate,
        "channels": w.out_channels,
        "sample_format": "s32le",
        "interleaved": true,
        "frames_recorded": frames_recorded,
        "finalized": finalized,
        "recovered": recovered,
        "peak_file": peak_file,
    });
    let text = serde_json::to_string_pretty(&metadata).unwrap_or_else(|_| "{}".to_string());
    std::fs::write(&w.sidecar_path, text)
}

/// Realtime-safe max into an f32-bits atomic (no allocation, no lock).
#[inline]
fn atomic_max_bits(target: &AtomicU32, value: f32) {
    let value = value.max(0.0);
    let mut cur = target.load(Ordering::Relaxed);
    loop {
        if value <= f32::from_bits(cur) {
            break;
        }
        match target.compare_exchange_weak(
            cur,
            value.to_bits(),
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(c) => cur = c,
        }
    }
}

// ── Input stream builder (f32 samples) ───────────────────────────────────────

/// Build the single recording capture stream. Its one realtime callback fans
/// out to four independent paths, none of which blocks another:
///   1. monitor    → `shared.input_ring` (read by the output render callback)
///   2. record     → `tx` channel → disk-writer worker thread
///   3. preview     → min/max/rms bins → `shared.preview_ring` (drained by UI)
///   4. meters/diag → raw input peak + lightweight counters
///
/// Realtime-safe: the record path uses a bounded preallocated block pool and
/// drops when the pool or writer queue is full; the monitor/preview/meter paths
/// are atomics-only.
#[allow(clippy::too_many_arguments)]
fn build_f32_input_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    tx: crossbeam_channel::Sender<Vec<i32>>,
    free_rx: crossbeam_channel::Receiver<Vec<i32>>,
    free_tx: crossbeam_channel::Sender<Vec<i32>>,
    active: Arc<AtomicBool>,
    dropped_blocks: Arc<AtomicU64>,
    shared: Arc<crate::engine::SharedState>,
    channels: usize,
    monitor_channels: Vec<usize>,
    samples_per_bin: usize,
) -> Result<cpal::Stream, SphereAudioError> {
    use crate::engine::{f32_load, f32_store};
    use crate::input_ring::WaveformPeak;

    let mon_l_ch = monitor_channels.first().copied().unwrap_or(0);
    let mon_r_ch = monitor_channels.get(1).copied().unwrap_or(mon_l_ch);
    let samples_per_bin = samples_per_bin.max(1);

    // Preview accumulator — captured (FnMut) state, no allocation per callback.
    let mut bin_min = f32::MAX;
    let mut bin_max = f32::MIN;
    let mut bin_sumsq = 0.0f32;
    let mut bin_count = 0usize;

    device
        .build_input_stream::<f32, _, _>(
            config,
            move |data: &[f32], _info| {
                let ch = channels.max(1);
                let frames = data.len() / ch;
                shared.input_cb_count.fetch_add(1, Ordering::Relaxed);
                shared
                    .input_frames_received
                    .fetch_add(frames as u64, Ordering::Relaxed);

                let mut raw_peak_l = 0.0f32;
                let mut raw_peak_r = 0.0f32;
                let mut last_l = 0.0f32;
                let mut last_r = 0.0f32;
                let mut rec_peak = 0.0f32;

                for frame in data.chunks(ch) {
                    let first = frame.first().copied().unwrap_or(0.0);
                    let l = frame
                        .get(mon_l_ch)
                        .copied()
                        .unwrap_or(first)
                        .clamp(-1.0, 1.0);
                    let r = frame.get(mon_r_ch).copied().unwrap_or(l).clamp(-1.0, 1.0);
                    last_l = l;
                    last_r = r;
                    raw_peak_l = raw_peak_l.max(l.abs());
                    raw_peak_r = raw_peak_r.max(r.abs());

                    // 1. Monitor bridge → output render callback.
                    shared.input_ring.write_stereo(l, r);

                    // 3. Preview bins (mono mix of the monitored channels).
                    // Guard with the same session-active flag as the writer so
                    // stopping a take cannot publish late bins while the UI is
                    // finalizing the committed clip.
                    if active.load(Ordering::Relaxed) {
                        let m = (l + r) * 0.5;
                        bin_min = bin_min.min(m);
                        bin_max = bin_max.max(m);
                        bin_sumsq += m * m;
                        bin_count += 1;
                        if bin_count >= samples_per_bin {
                            let rms = (bin_sumsq / bin_count as f32).sqrt();
                            shared.preview_ring.push(WaveformPeak {
                                min: bin_min,
                                max: bin_max,
                                rms,
                            });
                            bin_min = f32::MAX;
                            bin_max = f32::MIN;
                            bin_sumsq = 0.0;
                            bin_count = 0;
                        }
                    } else {
                        bin_min = f32::MAX;
                        bin_max = f32::MIN;
                        bin_sumsq = 0.0;
                        bin_count = 0;
                    }

                    // 4. Record peak across all channels (diagnostics).
                    for &s in frame {
                        rec_peak = rec_peak.max(s.abs());
                    }
                }

                // Meters / diagnostics atomics.
                shared
                    .live_input_l
                    .store(f32_store(last_l), Ordering::Relaxed);
                shared
                    .live_input_r
                    .store(f32_store(last_r), Ordering::Relaxed);
                atomic_max_bits(&shared.live_input_peak_l, raw_peak_l);
                atomic_max_bits(&shared.live_input_peak_r, raw_peak_r);
                shared.live_input_active.store(true, Ordering::Relaxed);
                let prev_rec = f32_load(shared.record_peak.load(Ordering::Relaxed)) * 0.9;
                shared
                    .record_peak
                    .store(f32_store(prev_rec.max(rec_peak)), Ordering::Relaxed);

                // 2. Record path → disk writer worker (only while armed/active).
                if active.load(Ordering::Relaxed) {
                    match free_rx.try_recv() {
                        Ok(mut block) => {
                            if block.capacity() < data.len() {
                                dropped_blocks.fetch_add(1, Ordering::Relaxed);
                                shared.record_ring_overruns.fetch_add(1, Ordering::Relaxed);
                                let _ = free_tx.try_send(block);
                                return;
                            }
                            block.clear();
                            block.extend(data.iter().copied().map(f32_to_s32));
                            if let Err(error) = tx.try_send(block) {
                                dropped_blocks.fetch_add(1, Ordering::Relaxed);
                                shared.record_ring_overruns.fetch_add(1, Ordering::Relaxed);
                                let _ = free_tx.try_send(error.into_inner());
                            }
                        }
                        Err(_) => {
                            dropped_blocks.fetch_add(1, Ordering::Relaxed);
                            shared.record_ring_overruns.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            },
            |err| eprintln!("[SphereAudio] Input stream error: {err}"),
            None,
        )
        .map_err(|e| SphereAudioError::NativeError(format!("Cannot open input stream: {e}")))
}

#[inline]
fn f32_to_s32(sample: f32) -> i32 {
    let x = sample.clamp(-1.0, 1.0);
    if x >= 0.0 {
        (x * i32::MAX as f32) as i32
    } else {
        (x * -(i32::MIN as f32)) as i32
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Open an input stream and begin recording armed tracks.
pub fn start_recording(
    config: JsStartRecordingConfig,
    shared: Arc<crate::engine::SharedState>,
    monitor_mix: bool,
) -> Result<RecordingSession, SphereAudioError> {
    if config.tracks.is_empty() {
        return Err(SphereAudioError::NativeError(
            "No armed tracks — nothing to record".to_string(),
        ));
    }

    let device = find_input_device(config.input_device_id.as_deref())?;

    let default_cfg = device
        .default_input_config()
        .map_err(|e| SphereAudioError::NativeError(format!("Input device config error: {e}")))?;

    let input_ch = default_cfg.channels() as usize;
    let sample_rate = default_cfg.sample_rate().0;

    let stream_config = cpal::StreamConfig {
        channels: default_cfg.channels(),
        sample_rate: default_cfg.sample_rate(),
        buffer_size: cpal::BufferSize::Default,
    };

    // Ensure directory structure exists.
    let project_root = Path::new(&config.project_root);
    let recordings_dir = project_root.join("recordings");

    std::fs::create_dir_all(&recordings_dir).map_err(|e| {
        SphereAudioError::NativeError(format!("Cannot create recordings folder: {e}"))
    })?;
    let project_start_sample = shared.position_samples.load(Ordering::Relaxed);

    // Build per-track writer states.
    let mut track_writers: Vec<TrackWriterState> = Vec::new();
    for (track_index, track) in config.tracks.iter().enumerate() {
        let project_name = if config.project_name.trim().is_empty() {
            "Recording"
        } else {
            config.project_name.as_str()
        };
        let timestamp = if config.timestamp.trim().is_empty() {
            config.session_id.as_str()
        } else {
            config.timestamp.as_str()
        };
        let final_path = unique_recording_path(&recordings_dir, project_name, timestamp, "rauf");
        let filename = final_path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| "recording.rauf".to_string());
        let relative_path = format!("recordings/{filename}");
        let sidecar_path = final_path.with_extension("rauf.json");
        let sidecar_filename = sidecar_path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| "recording.rauf.json".to_string());
        let sidecar_relative_path = format!("recordings/{sidecar_filename}");

        let in_chs: Vec<usize> = track.input_channels.iter().map(|&c| c as usize).collect();
        if in_chs.is_empty() {
            return Err(SphereAudioError::NativeError(format!(
                "{} has no input channels selected",
                track.name
            )));
        }
        if let Some(channel) = in_chs.iter().find(|&&channel| channel >= input_ch) {
            return Err(SphereAudioError::NativeError(format!(
                "{} input channel {} is unavailable on the active input device ({input_ch} channel(s))",
                track.name,
                channel + 1
            )));
        }
        let out_channels = in_chs.len().max(1) as u16;
        let take_id = make_take_id(&config.session_id, track_index as u64);
        let writer = RaufWriter::create(
            &final_path,
            RaufConfig {
                sample_rate,
                channels: out_channels,
                sample_format: RaufSampleFormat::S32,
                interleaved: true,
                project_start_sample,
                take_id,
            },
        )
        .map_err(|e| {
            SphereAudioError::NativeError(format!("Cannot create RAUF recording file: {e}"))
        })?;

        track_writers.push(TrackWriterState {
            track_id: track.track_id.clone(),
            track_name: track.name.clone(),
            writer,
            input_channels: in_chs,
            out_channels,
            final_path,
            relative_path,
            sidecar_path,
            sidecar_relative_path,
            take_id: format_take_id(take_id),
            project_start_sample,
            error: None,
        });
    }

    let track_count = track_writers.len();

    // Bounded channel: if the disk writer falls behind, `try_send` drops the
    // block rather than blocking the audio callback.
    let (audio_tx, audio_rx) = bounded::<Vec<i32>>(512);
    let (free_tx, free_rx) = bounded::<Vec<i32>>(512);
    let max_record_block_samples = input_ch.saturating_mul(8192).max(input_ch.max(1));
    for _ in 0..512 {
        let _ = free_tx.try_send(Vec::with_capacity(max_record_block_samples));
    }

    // Spawn disk writer — owns `audio_rx` and all file handles.
    let (finalize_tx, finalize_rx) = std::sync::mpsc::channel();
    let start_beat = config.start_beat;
    let writer_free_tx = free_tx.clone();
    std::thread::spawn(move || {
        disk_writer_thread(
            audio_rx,
            writer_free_tx,
            track_writers,
            sample_rate,
            input_ch,
            start_beat,
            finalize_tx,
        );
    });

    // AtomicBool: the input callback checks this before sending.
    let recording_active = Arc::new(AtomicBool::new(true));
    let dropped_blocks = Arc::new(AtomicU64::new(0));
    shared.recording_active.store(true, Ordering::Relaxed);
    shared
        .recording_monitor_mix
        .store(monitor_mix, Ordering::Relaxed);
    let monitor_channels: Vec<usize> = config
        .monitor_channels
        .iter()
        .copied()
        .filter_map(|channel| {
            let channel = channel as usize;
            (channel < input_ch).then_some(channel)
        })
        .collect();

    // ── Realtime preview + monitor setup (Parts 1 & 2) ────────────────────
    // The recording stream is the single capture source during a take: it
    // feeds the monitor ring, the preview ring, and the file writer. Monitoring
    // is mixed by the output callback from the ring (clean), not by the old
    // sample-and-hold path.
    const PREVIEW_PEAKS_PER_SEC: u32 = 150;
    let samples_per_bin = (sample_rate / PREVIEW_PEAKS_PER_SEC).max(1) as usize;
    let preview_channels = monitor_channels.len().max(1) as u32;
    let start_sample = shared.position_samples.load(Ordering::Relaxed);

    shared.preview_ring.reset();
    shared.recording_preview_id.fetch_add(1, Ordering::Relaxed);
    shared
        .recording_preview_start_sample
        .store(start_sample, Ordering::Relaxed);
    shared
        .recording_preview_sample_rate
        .store(sample_rate, Ordering::Relaxed);
    shared
        .recording_preview_channels
        .store(preview_channels, Ordering::Relaxed);
    shared
        .recording_preview_peaks_per_sec
        .store(PREVIEW_PEAKS_PER_SEC, Ordering::Relaxed);
    shared
        .recording_preview_active
        .store(true, Ordering::Relaxed);

    // Arm the monitor ring for this stream's format and enable output monitoring
    // when the user requested it.
    shared
        .input_ring
        .set_active(true, input_ch as u32, sample_rate);
    shared
        .monitor_enabled_any
        .store(monitor_mix, Ordering::Relaxed);

    // Build the input stream — `audio_tx` is moved into the closure.
    let input_stream = build_f32_input_stream(
        &device,
        &stream_config,
        audio_tx,
        free_rx,
        free_tx,
        Arc::clone(&recording_active),
        Arc::clone(&dropped_blocks),
        Arc::clone(&shared),
        input_ch,
        monitor_channels,
        samples_per_bin,
    )?;

    input_stream
        .play()
        .map_err(|e| SphereAudioError::NativeError(format!("Cannot start input stream: {e}")))?;

    eprintln!(
        "[SphereAudio] Recording started: {track_count} track(s), \
         {input_ch}ch input @ {sample_rate} Hz"
    );

    Ok(RecordingSession {
        _input_stream: input_stream,
        results_rx: finalize_rx,
        start_beat,
        sample_rate,
        track_count,
        recording_active,
        dropped_blocks,
        started_at: std::time::Instant::now(),
        shared,
    })
}

/// Stop recording, finalize RAUF files, and return per-track results.
pub fn stop_recording(
    session: RecordingSession,
) -> Result<Vec<JsRecordingResult>, SphereAudioError> {
    // Tell the callback to stop sending.
    session.recording_active.store(false, Ordering::Relaxed);
    session
        .shared
        .recording_active
        .store(false, Ordering::Relaxed);
    session
        .shared
        .recording_monitor_mix
        .store(false, Ordering::Relaxed);
    session
        .shared
        .recording_preview_active
        .store(false, Ordering::Relaxed);
    session.shared.input_ring.set_active(false, 0, 0);
    session
        .shared
        .monitor_enabled_any
        .store(false, Ordering::Relaxed);
    let dropped_blocks = session.dropped_blocks.load(Ordering::Relaxed);

    // Dropping the stream disconnects `audio_tx` (it lived inside the closure),
    // which causes `audio_rx.recv()` in the disk writer to return Err → loop exits.
    drop(session._input_stream);

    // Wait up to 60 s for the disk writer to flush and finalize.
    let mut results = session
        .results_rx
        .recv_timeout(std::time::Duration::from_secs(60))
        .map_err(|e| {
            SphereAudioError::NativeError(format!("Recording finalization timed out: {e}"))
        })?;

    eprintln!(
        "[SphereAudio] Recording stopped: {} file(s) finalized",
        results.len()
    );

    if dropped_blocks > 0 {
        for result in &mut results {
            result.success = false;
            result.error = Some(format!(
                "Recording writer could not keep up; dropped {dropped_blocks} input block(s)"
            ));
        }
    }

    Ok(results
        .into_iter()
        .map(|r| JsRecordingResult {
            track_id: r.track_id,
            file_path: r.file_path,
            relative_path: r.relative_path,
            start_beat: r.start_beat,
            duration_seconds: r.duration_seconds,
            sample_rate: r.sample_rate,
            channels: r.channels,
            metadata_path: r.metadata_path,
            sample_format: r.sample_format,
            success: r.success,
            error: r.error,
        })
        .collect())
}
