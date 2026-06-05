//! Audio recording session management.
//!
//! # Architecture
//!
//! ```text
//! JS control thread
//!   └─ start_recording()
//!        ├─ opens cpal input stream  ──► input callback
//!        │                                 └─ try_send(block) ──► crossbeam channel
//!        └─ spawns disk writer thread ──► recv(block) → write WAV temp file
//!
//! JS control thread
//!   └─ stop_recording()
//!        ├─ drop(input_stream)  →  channel closes  →  disk writer exits loop
//!        └─ recv(results)  →  return to caller
//! ```
//!
//! The audio callback does only two things: load an `AtomicBool` and
//! call `try_send`.  No allocation, no locking, no file I/O.

use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::bounded;

use crate::error::SphereAudioError;
use crate::types::{JsRecordingResult, JsStartRecordingConfig};

// ── Unique recording counter for collision-free filenames ─────────────────────

static RECORD_COUNTER: AtomicU64 = AtomicU64::new(1);

// ── Internal types ────────────────────────────────────────────────────────────

struct TrackWriterState {
    track_id: String,
    file: std::fs::File,
    data_bytes: u64,
    /// 0-based indices into the interleaved input block to capture.
    input_channels: Vec<usize>,
    /// Number of channels written to the output WAV (= input_channels.len()).
    out_channels: u16,
    temp_path: PathBuf,
    final_path: PathBuf,
    relative_path: String,
}

pub struct RecordingResult {
    pub track_id: String,
    pub file_path: String,
    pub relative_path: String,
    pub start_beat: f64,
    pub duration_seconds: f64,
    pub sample_rate: u32,
    pub channels: u32,
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

// ── WAV writing ───────────────────────────────────────────────────────────────

/// Write a 44-byte WAV header with placeholder sizes (filled in on finalize).
fn write_wav_placeholder(file: &mut std::fs::File, channels: u16, sample_rate: u32) {
    let byte_rate = sample_rate * channels as u32 * 4;
    let block_align = channels * 4;
    let _ = file.write_all(b"RIFF");
    let _ = file.write_all(&0u32.to_le_bytes()); // riff size — filled later
    let _ = file.write_all(b"WAVE");
    let _ = file.write_all(b"fmt ");
    let _ = file.write_all(&16u32.to_le_bytes()); // fmt chunk length
    let _ = file.write_all(&3u16.to_le_bytes()); // IEEE float PCM
    let _ = file.write_all(&channels.to_le_bytes());
    let _ = file.write_all(&sample_rate.to_le_bytes());
    let _ = file.write_all(&byte_rate.to_le_bytes());
    let _ = file.write_all(&block_align.to_le_bytes());
    let _ = file.write_all(&32u16.to_le_bytes()); // 32-bit float
    let _ = file.write_all(b"data");
    let _ = file.write_all(&0u32.to_le_bytes()); // data size — filled later
}

/// Seek to the start of the file and patch in the correct RIFF / data sizes.
fn finalize_wav(file: &mut std::fs::File, data_bytes: u64, channels: u16, sample_rate: u32) {
    let data_size = data_bytes.min(u32::MAX as u64) as u32;
    let riff_size = data_size.saturating_add(36); // header after "RIFF\x04"
    let byte_rate = sample_rate * channels as u32 * 4;
    let block_align = channels * 4;

    let _ = file.seek(SeekFrom::Start(0));
    let _ = file.write_all(b"RIFF");
    let _ = file.write_all(&riff_size.to_le_bytes());
    let _ = file.write_all(b"WAVE");
    let _ = file.write_all(b"fmt ");
    let _ = file.write_all(&16u32.to_le_bytes());
    let _ = file.write_all(&3u16.to_le_bytes());
    let _ = file.write_all(&channels.to_le_bytes());
    let _ = file.write_all(&sample_rate.to_le_bytes());
    let _ = file.write_all(&byte_rate.to_le_bytes());
    let _ = file.write_all(&block_align.to_le_bytes());
    let _ = file.write_all(&32u16.to_le_bytes());
    let _ = file.write_all(b"data");
    let _ = file.write_all(&data_size.to_le_bytes());
    let _ = file.flush();
}

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
        "wav"
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

// ── Disk writer thread ────────────────────────────────────────────────────────

fn disk_writer_thread(
    audio_rx: crossbeam_channel::Receiver<Vec<f32>>,
    mut writers: Vec<TrackWriterState>,
    sample_rate: u32,
    input_ch: usize, // channels per interleaved input frame
    start_beat: f64,
    finalize_tx: std::sync::mpsc::Sender<Vec<RecordingResult>>,
) {
    // Write placeholder WAV headers (will be overwritten on finalize).
    for w in &mut writers {
        write_wav_placeholder(&mut w.file, w.out_channels, sample_rate);
    }

    let mut total_frames = 0u64;

    // Drain audio blocks until the sender (input stream) disconnects.
    while let Ok(block) = audio_rx.recv() {
        let frames = block.len().checked_div(input_ch).unwrap_or(0);
        if frames == 0 {
            continue;
        }
        for w in &mut writers {
            for f in 0..frames {
                for &ch in &w.input_channels {
                    let s = if ch < input_ch {
                        block[f * input_ch + ch]
                    } else {
                        0.0f32
                    };
                    let _ = w.file.write_all(&s.to_le_bytes());
                    w.data_bytes += 4;
                }
            }
        }
        total_frames += frames as u64;
    }

    let duration_seconds = if sample_rate > 0 {
        total_frames as f64 / sample_rate as f64
    } else {
        0.0
    };

    let mut results = Vec::with_capacity(writers.len());

    for mut w in writers {
        finalize_wav(&mut w.file, w.data_bytes, w.out_channels, sample_rate);
        drop(w.file); // close before rename

        let ok = std::fs::rename(&w.temp_path, &w.final_path).is_ok();
        results.push(RecordingResult {
            track_id: w.track_id,
            file_path: if ok {
                w.final_path.to_string_lossy().into_owned()
            } else {
                w.temp_path.to_string_lossy().into_owned()
            },
            relative_path: w.relative_path,
            start_beat,
            duration_seconds,
            sample_rate,
            channels: w.out_channels as u32,
            success: ok,
            error: if ok {
                None
            } else {
                Some("Failed to move recording file to final location".to_string())
            },
        });
    }

    let _ = finalize_tx.send(results);
}

/// Mix the latest captured input sample onto interleaved output (Phase U monitor).
pub fn apply_recording_monitor_mix(
    data: &mut [f32],
    channels: usize,
    shared: &crate::engine::SharedState,
    master_vol: f32,
) {
    use std::sync::atomic::Ordering;
    if channels < 2 || !shared.recording_monitor_mix.load(Ordering::Relaxed) {
        return;
    }
    let mon_l =
        f32::from_bits(shared.recording_monitor_l.load(Ordering::Relaxed)) * master_vol * 0.85;
    let mon_r =
        f32::from_bits(shared.recording_monitor_r.load(Ordering::Relaxed)) * master_vol * 0.85;
    for frame in data.chunks_mut(channels) {
        frame[0] = (frame[0] + mon_l).clamp(-1.0, 1.0);
        frame[1] = (frame[1] + mon_r).clamp(-1.0, 1.0);
    }
}

// ── Input stream builder (f32 samples) ───────────────────────────────────────

fn build_f32_input_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    tx: crossbeam_channel::Sender<Vec<f32>>,
    active: Arc<AtomicBool>,
    dropped_blocks: Arc<AtomicU64>,
    shared: Arc<crate::engine::SharedState>,
    channels: usize,
    monitor_channels: Vec<usize>,
) -> Result<cpal::Stream, SphereAudioError> {
    device
        .build_input_stream::<f32, _, _>(
            config,
            move |data: &[f32], _info| {
                if active.load(Ordering::Relaxed) {
                    // `to_vec()` allocates once per block — not in the output hot path,
                    // so occasional allocation here is acceptable for recording.
                    if tx.try_send(data.to_vec()).is_err() {
                        dropped_blocks.fetch_add(1, Ordering::Relaxed);
                    }
                    if shared.recording_monitor_mix.load(Ordering::Relaxed) && channels > 0 {
                        let frames = data.len() / channels.max(1);
                        if frames > 0 {
                            let last = frames - 1;
                            let left_ch = monitor_channels.first().copied().unwrap_or(0);
                            let right_ch = monitor_channels.get(1).copied().unwrap_or(left_ch);
                            let left = data.get(last * channels + left_ch).copied().unwrap_or(0.0);
                            let right = data
                                .get(last * channels + right_ch)
                                .copied()
                                .unwrap_or(left);
                            shared
                                .recording_monitor_l
                                .store(left.to_bits(), Ordering::Relaxed);
                            shared
                                .recording_monitor_r
                                .store(right.to_bits(), Ordering::Relaxed);
                        }
                    }
                }
            },
            |err| eprintln!("[SphereAudio] Input stream error: {err}"),
            None,
        )
        .map_err(|e| SphereAudioError::NativeError(format!("Cannot open input stream: {e}")))
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
    let media_dir = project_root.join("Media").join("Audio");
    let temp_dir = media_dir.join(".rec").join(&config.session_id);

    std::fs::create_dir_all(&media_dir)
        .map_err(|e| SphereAudioError::NativeError(format!("Cannot create Media/Audio: {e}")))?;
    std::fs::create_dir_all(&temp_dir).map_err(|e| {
        SphereAudioError::NativeError(format!("Cannot create temp recording dir: {e}"))
    })?;

    // Build per-track writer states.
    let mut track_writers: Vec<TrackWriterState> = Vec::new();
    for track in &config.tracks {
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
        let final_path = unique_recording_path(&media_dir, project_name, timestamp, "wav");
        let filename = final_path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| "recording.wav".to_string());
        let relative_path = format!("Media/Audio/{filename}");
        let temp_path = temp_dir.join(format!("{}.tmp.wav", sanitize_filename(&track.track_id)));

        let file = std::fs::File::create(&temp_path).map_err(|e| {
            SphereAudioError::NativeError(format!("Cannot create recording temp file: {e}"))
        })?;

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

        track_writers.push(TrackWriterState {
            track_id: track.track_id.clone(),
            file,
            data_bytes: 0,
            input_channels: in_chs,
            out_channels,
            temp_path,
            final_path,
            relative_path,
        });
    }

    let track_count = track_writers.len();

    // Bounded channel: if the disk writer falls behind, `try_send` drops the
    // block rather than blocking the audio callback.
    let (audio_tx, audio_rx) = bounded::<Vec<f32>>(512);

    // Spawn disk writer — owns `audio_rx` and all file handles.
    let (finalize_tx, finalize_rx) = std::sync::mpsc::channel();
    let start_beat = config.start_beat;
    std::thread::spawn(move || {
        disk_writer_thread(
            audio_rx,
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

    // Build the input stream — `audio_tx` is moved into the closure.
    let input_stream = build_f32_input_stream(
        &device,
        &stream_config,
        audio_tx,
        Arc::clone(&recording_active),
        Arc::clone(&dropped_blocks),
        Arc::clone(&shared),
        input_ch,
        monitor_channels,
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

/// Stop recording, finalize WAV files, and return per-track results.
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
            success: r.success,
            error: r.error,
        })
        .collect())
}
