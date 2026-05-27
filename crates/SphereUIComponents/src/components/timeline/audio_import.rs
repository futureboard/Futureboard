//! Background audio import: probe → peak generation → disk cache.
//!
//! Never runs decode/peak work on the GPUI render thread. UI updates are
//! throttled to ≤10 Hz except on state transitions.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use gpui::{AsyncApp, Context, Entity, WeakEntity};

use super::timeline::Timeline;
use super::timeline_state::AudioImportState;
use super::waveform_cache::{self, WaveformFileMeta, WaveformPreview, CHUNK_PEAKS, PEAK_FINE_SPP};
use crate::layout::StudioLayout;

/// Bump when peak format or LOD ladder changes to invalidate disk cache.
pub const PEAK_DECODER_VERSION: u32 = waveform_cache::WAVEFORM_ALGORITHM_VERSION;
const TARGET_PEAK_SAMPLE_RATE: u32 = 48_000;

static NOTIFY_THROTTLE: OnceLock<Mutex<Instant>> = OnceLock::new();
static IMPORT_DEBUG: OnceLock<bool> = OnceLock::new();
static UI_NOTIFY_COUNT: OnceLock<Mutex<u64>> = OnceLock::new();

fn import_debug() -> bool {
    *IMPORT_DEBUG.get_or_init(|| std::env::var_os("FUTUREBOARD_AUDIO_IMPORT_DEBUG").is_some())
}

fn peaks_cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("futureboard")
        .join("Peaks")
}

pub fn stable_cache_key(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path_s = path.to_string_lossy();
    let mut hasher = crc32fast::Hasher::new();
    use std::hash::Hasher;
    hasher.write(path_s.as_bytes());
    hasher.write(&meta.len().to_le_bytes());
    hasher.write(&modified.to_le_bytes());
    hasher.write(&TARGET_PEAK_SAMPLE_RATE.to_le_bytes());
    hasher.write(&waveform_cache::WAVEFORM_ALGORITHM_VERSION.to_le_bytes());
    Some(format!("{:08x}", hasher.finalize()))
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PeaksDiskCache {
    decoder_version: u32,
    preview: WaveformPreview,
}

fn disk_cache_path(cache_key: &str) -> PathBuf {
    peaks_cache_dir().join(format!("{cache_key}.peaks.json"))
}

fn try_load_disk_cache(cache_key: &str) -> Option<Arc<WaveformPreview>> {
    let path = disk_cache_path(cache_key);
    let bytes = std::fs::read(&path).ok()?;
    let envelope: PeaksDiskCache = serde_json::from_slice(&bytes).ok()?;
    if envelope.decoder_version != PEAK_DECODER_VERSION {
        return None;
    }
    if import_debug() {
        eprintln!("[audio-import] disk cache HIT key={cache_key} path={}", path.display());
    }
    Some(Arc::new(envelope.preview))
}

fn save_disk_cache(cache_key: &str, preview: &WaveformPreview) {
    let dir = peaks_cache_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = disk_cache_path(cache_key);
    let envelope = PeaksDiskCache {
        decoder_version: PEAK_DECODER_VERSION,
        preview: preview.clone(),
    };
    if let Ok(json) = serde_json::to_vec(&envelope) {
        let _ = std::fs::write(&path, json);
        if import_debug() {
            eprintln!("[audio-import] disk cache WRITE key={cache_key} path={}", path.display());
        }
    }
}

fn record_ui_notify() {
    if import_debug() {
        if let Ok(mut n) = UI_NOTIFY_COUNT.get_or_init(|| Mutex::new(0)).lock() {
            *n += 1;
        }
    }
}

fn maybe_log_notify_count() {
    if !import_debug() {
        return;
    }
    if let Ok(n) = UI_NOTIFY_COUNT.get_or_init(|| Mutex::new(0)).lock() {
        eprintln!("[audio-import] ui_notify_count={n}");
    }
}

/// Install peak chunks on the async executor with throttled UI refresh (WebUI-style progressive draw).
fn install_preview_chunks_progressive(
    path_key: &str,
    preview: Arc<WaveformPreview>,
    timeline: &WeakEntity<Timeline>,
    cx: &mut AsyncApp,
) {
    let lod = preview
        .lods
        .iter()
        .find(|l| l.samples_per_peak == PEAK_FINE_SPP)
        .or_else(|| preview.lods.first());
    let Some(lod) = lod else {
        waveform_cache::finish_peak_build(path_key, preview);
        return;
    };
    let meta = Arc::new(WaveformFileMeta {
        sample_rate: preview.sample_rate,
        channels: preview.channels,
        duration_seconds: preview.duration_seconds,
        total_frames: preview.total_frames,
        peak_count: lod.peaks.len(),
        primary_spp: lod.samples_per_peak,
    });
    let chunks_total = lod.peaks.len().div_ceil(CHUNK_PEAKS);
    waveform_cache::begin_peak_build(path_key, Arc::clone(&meta), chunks_total);
    waveform_cache::set_import_state(path_key, AudioImportState::GeneratingPeaks { progress: 0.0 });
    throttled_timeline_notify(timeline, cx, true);

    let spp = lod.samples_per_peak as u32;
    for chunk_index in 0..chunks_total {
        let start = chunk_index * CHUNK_PEAKS;
        let end = (start + CHUNK_PEAKS).min(lod.peaks.len());
        let slice = Arc::new(lod.peaks[start..end].to_vec());
        waveform_cache::install_chunk(path_key, spp, chunk_index as u32, slice);
        if chunk_index == 0 || chunk_index + 1 == chunks_total || chunk_index % 4 == 0 {
            let progress = (chunk_index + 1) as f32 / chunks_total as f32;
            waveform_cache::set_import_state(
                path_key,
                AudioImportState::GeneratingPeaks { progress },
            );
            throttled_timeline_notify(timeline, cx, chunk_index + 1 == chunks_total);
        }
    }

    // Install the remaining LOD chunks after the primary fine pass. This keeps
    // the first visible waveform quick while making zoomed-out renders read
    // from coarser chunk data instead of scanning excessive fine peaks.
    for other_lod in preview
        .lods
        .iter()
        .filter(|other_lod| other_lod.samples_per_peak != lod.samples_per_peak)
    {
        let spp = other_lod.samples_per_peak as u32;
        let lod_chunks_total = other_lod.peaks.len().div_ceil(CHUNK_PEAKS);
        for chunk_index in 0..lod_chunks_total {
            let start = chunk_index * CHUNK_PEAKS;
            let end = (start + CHUNK_PEAKS).min(other_lod.peaks.len());
            let slice = Arc::new(other_lod.peaks[start..end].to_vec());
            waveform_cache::install_chunk(path_key, spp, chunk_index as u32, slice);
        }
    }
    waveform_cache::finish_peak_build(path_key, preview);
}

/// Throttled UI refresh: ≤10 Hz unless `force` (state transition).
pub fn throttled_timeline_notify(
    timeline: &WeakEntity<Timeline>,
    cx: &mut AsyncApp,
    force: bool,
) {
    let throttle = NOTIFY_THROTTLE.get_or_init(|| Mutex::new(Instant::now() - Duration::from_secs(1)));
    let mut last = throttle.lock().expect("notify throttle");
    if !force && last.elapsed() < Duration::from_millis(100) {
        return;
    }
    *last = Instant::now();
    drop(last);

    let _ = timeline.update(cx, |_, cx| {
        record_ui_notify();
        cx.notify();
    });
}

fn run_peak_job(path: &Path, path_key: &str) -> Result<Arc<WaveformPreview>, String> {
    let started = Instant::now();
    let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if import_debug() {
        eprintln!(
            "[audio-import] peak job start path={} size={} bytes",
            path.display(),
            file_size
        );
    }

    if let Some(key) = stable_cache_key(path) {
        if let Some(preview) = try_load_disk_cache(&key) {
            waveform_cache::ingest_preview_as_chunks(path_key, Arc::clone(&preview));
            return Ok(preview);
        }
    }

    waveform_cache::set_import_state(path_key, AudioImportState::Decoding { progress: 0.0 });

    let peaks = DAUx::generate_audio_peaks(path).map_err(|e| e.to_string())?;
    let preview: WaveformPreview = peaks.into();
    let preview = Arc::new(preview);

    if let Some(key) = stable_cache_key(path) {
        save_disk_cache(&key, preview.as_ref());
    }

    if import_debug() {
        let total_peaks: usize = preview.lods.iter().map(|l| l.peaks.len()).sum();
        eprintln!(
            "[audio-import] peak job done path={} decode_ms={} total_peaks={}",
            path.display(),
            started.elapsed().as_millis(),
            total_peaks
        );
        maybe_log_notify_count();
    }

    Ok(preview)
}

/// Idempotent: one background job per absolute path. Call from any `cx.spawn`.
pub async fn run_import_pipeline(
    path: PathBuf,
    timeline: WeakEntity<Timeline>,
    layout: Option<WeakEntity<StudioLayout>>,
    cx: &mut AsyncApp,
) {
    let key = path.to_string_lossy().to_string();
    if !waveform_cache::try_begin_import(&key) {
        return;
    }

    let path_for_job = path.clone();
    let timeline_probe = timeline.clone();
    let timeline_peaks = timeline.clone();
    let layout_weak = layout.clone();

    // ── Probe metadata ───────────────────────────────────────────────
        waveform_cache::set_import_state(&key, AudioImportState::Probing);
        throttled_timeline_notify(&timeline_probe, cx, true);

        let meta_path = path_for_job.clone();
        let probe = cx
            .background_executor()
            .spawn(async move { DAUx::probe_audio_file(&meta_path) })
            .await;

        match probe {
            Ok(info) => {
                let format = info.format.as_str().to_string();
                let path_key = key.clone();
                let layout_for_meta = layout_weak.clone();
                let _ = timeline_probe.update(cx, move |timeline, cx| {
                    let changed = timeline.state.update_audio_clip_metadata(
                        &path_key,
                        &format,
                        info.sample_rate,
                        info.channels,
                        info.total_frames,
                        info.duration_seconds,
                    );
                    timeline
                        .state
                        .set_audio_import_for_path(&path_key, AudioImportState::Decoding { progress: 0.0 });
                    if changed {
                        if let Some(owner) = layout_for_meta.as_ref() {
                            let _ = owner.update(cx, |this, cx| {
                                this.mark_engine_media_dirty();
                                this.schedule_audio_project_sync(cx, false, "audio_import_probe");
                            });
                        }
                    }
                });
                throttled_timeline_notify(&timeline_probe, cx, true);
            }
            Err(error) => {
                eprintln!(
                    "[audio-import] probe failed path={} error={}",
                    key, error
                );
            }
        }

        // ── Peak generation (full decode, off UI thread) ───────────────────
        waveform_cache::set_import_state(&key, AudioImportState::GeneratingPeaks { progress: 0.0 });
        throttled_timeline_notify(&timeline_peaks, cx, true);

        let decode_path = path_for_job.clone();
        let path_key = key.clone();
        let path_key_for_job = path_key.clone();
        let result = cx
            .background_executor()
            .spawn(async move { run_peak_job(&decode_path, &path_key_for_job) })
            .await;

        match result {
            Ok(preview) => {
                install_preview_chunks_progressive(
                    &path_key,
                    preview,
                    &timeline_peaks,
                    cx,
                );
                let _ = timeline_peaks.update(cx, move |timeline, cx| {
                    timeline
                        .state
                        .set_audio_import_for_path(&path_key, AudioImportState::Ready);
                    cx.notify();
                });
                throttled_timeline_notify(&timeline_peaks, cx, true);
            }
            Err(message) => {
                waveform_cache::install_failed(&path_key, message.clone());
                let _ = timeline_peaks.update(cx, move |timeline, cx| {
                    timeline.state.set_audio_import_for_path(
                        &path_key,
                        AudioImportState::Failed {
                            message: message.clone(),
                        },
                    );
                    cx.notify();
                });
                throttled_timeline_notify(&timeline_peaks, cx, true);
            }
        }
}

/// Timeline drop import entry point.
///
/// Must be called from inside `Timeline`'s own `update` (e.g. file-drop handler).
/// Do not call `timeline.update` here — the caller already holds the entity lease.
/// Clip `audio_import` is set to `Pending` in `insert_audio_clip`.
pub fn spawn_timeline_import(
    path: PathBuf,
    _timeline: Entity<Timeline>,
    layout: Option<Entity<StudioLayout>>,
    cx: &mut Context<Timeline>,
) {
    waveform_cache::request_decode_file(path.clone());

    let timeline_weak = _timeline.downgrade();
    let layout_weak = layout.map(|e| e.downgrade());
    cx.spawn(async move |_timeline, cx| {
        run_import_pipeline(path, timeline_weak, layout_weak, cx).await;
    })
    .detach();
}

/// Browser / layout import entry (StudioLayout context).
pub fn spawn_timeline_import_from_layout(
    path: PathBuf,
    timeline: Entity<Timeline>,
    layout: Entity<StudioLayout>,
    cx: &mut Context<StudioLayout>,
) {
    let path_key = path.to_string_lossy().to_string();
    let _ = timeline.update(cx, |timeline, _cx| {
        timeline
            .state
            .set_audio_import_for_path(&path_key, AudioImportState::Pending);
    });
    waveform_cache::request_decode_file(path.clone());

    let timeline_weak = timeline.downgrade();
    let layout_weak = layout.downgrade();
    cx.spawn(async move |_layout, cx| {
        run_import_pipeline(path, timeline_weak, Some(layout_weak), cx).await;
    })
    .detach();
}
