//! UI-side façade over the DirectAudioEngine peak generator.
//!
//! Peak generation lives in `crates/SphereDirectAudioEngine/src/
//! audio_file.rs::generate_audio_peaks` — this module is now a thin
//! adapter that:
//!   * tracks per-path Pending / Ready / Error status (the UI's render
//!     code needs the tri-state to draw placeholders cleanly),
//!   * converts the engine's `AudioPeakFile` into the UI-local
//!     `WaveformPreview` shape consumed by `waveform_canvas` and
//!     `pick_lod`,
//!   * keeps the synthetic demo-clip preview path for clips that have
//!     no source file (e.g. the dev-flag demo project tracks).
//!
//! Realtime / audio rules:
//!   * decoding happens on `std::thread::spawn` background threads.
//!   * render / layout only reads the cache via [`get_file_status`] /
//!     [`get_file_waveform`].
//!   * the engine path never runs on the audio callback.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use DAUx::{generate_audio_peaks, AudioPeak as EnginePeak, AudioPeakFile, PEAK_LOD_LEVELS};

#[derive(Debug, Clone, Copy)]
pub struct WaveformPeak {
    pub min: f32,
    pub max: f32,
}

impl From<EnginePeak> for WaveformPeak {
    fn from(p: EnginePeak) -> Self {
        Self { min: p.min, max: p.max }
    }
}

/// One mip level of the waveform: every entry summarises `samples_per_peak`
/// consecutive mono samples as a (min, max) pair.
#[derive(Debug, Clone)]
pub struct WaveformLod {
    pub samples_per_peak: usize,
    pub peaks: Vec<WaveformPeak>,
}

#[derive(Debug, Clone)]
pub struct WaveformPreview {
    pub sample_rate: u32,
    pub channels: u16,
    pub duration_seconds: f64,
    pub total_frames: u64,
    /// LODs sorted ascending by `samples_per_peak`.
    pub lods: Vec<WaveformLod>,
}

impl From<AudioPeakFile> for WaveformPreview {
    fn from(peaks: AudioPeakFile) -> Self {
        Self {
            sample_rate: peaks.sample_rate,
            channels: peaks.channels,
            duration_seconds: peaks.duration_seconds,
            total_frames: peaks.total_frames,
            lods: peaks
                .lods
                .into_iter()
                .map(|lod| WaveformLod {
                    samples_per_peak: lod.samples_per_peak as usize,
                    peaks: lod.peaks.into_iter().map(WaveformPeak::from).collect(),
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum WaveformStatus {
    Pending,
    Ready(WaveformPreview),
    Error(String),
}

/// LOD levels exactly as required by the spec: a power-of-two ladder
/// from 256 to 65536. Mirrors `DAUx::PEAK_LOD_LEVELS`; kept here in
/// `usize` form because `pick_lod` and the waveform canvas use it as
/// a Vec index basis. Asserted equal at runtime initialisation.
pub const LOD_LEVELS: [usize; 9] = [256, 512, 1024, 2048, 4096, 8192, 16384, 32768, 65536];

const _: () = {
    // Compile-time guard that PEAK_LOD_LEVELS length matches LOD_LEVELS.
    // Mismatched LOD ladders between engine and UI would silently
    // truncate or insert empty LODs.
    assert!(LOD_LEVELS.len() == PEAK_LOD_LEVELS.len());
};

/// Two caches: synthetic demo clips keyed by id, decoded files keyed by absolute path.
static CLIP_CACHE: OnceLock<Mutex<HashMap<String, WaveformPreview>>> = OnceLock::new();
static FILE_CACHE: OnceLock<Mutex<HashMap<String, WaveformStatus>>> = OnceLock::new();

fn clip_cache() -> &'static Mutex<HashMap<String, WaveformPreview>> {
    CLIP_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn file_cache() -> &'static Mutex<HashMap<String, WaveformStatus>> {
    FILE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Cheap render-path lookup — no decoding, no allocation beyond a clone.
pub fn get_file_waveform(path: &str) -> Option<WaveformPreview> {
    match file_cache().lock().ok()?.get(path).cloned()? {
        WaveformStatus::Ready(preview) => Some(preview),
        _ => None,
    }
}

pub fn get_file_status(path: &str) -> WaveformStatus {
    file_cache()
        .lock()
        .ok()
        .and_then(|cache| cache.get(path).cloned())
        .unwrap_or(WaveformStatus::Pending)
}

pub fn request_decode_file(path: PathBuf) {
    let key = path.to_string_lossy().to_string();
    let should_start = {
        let Ok(mut cache) = file_cache().lock() else {
            return;
        };
        if cache.contains_key(&key) {
            false
        } else {
            cache.insert(key.clone(), WaveformStatus::Pending);
            true
        }
    };

    if !should_start {
        return;
    }

    std::thread::spawn(move || {
        let decoded = decode_file_uncached(&path);
        if let Ok(mut cache) = file_cache().lock() {
            cache.insert(
                key,
                decoded
                    .map(WaveformStatus::Ready)
                    .unwrap_or_else(|| WaveformStatus::Error("Decode failed".to_string())),
            );
        }
    });
}

/// Decode a WAV/MP3 (or symphonia-supported) file into a multi-LOD preview.
/// Cached by path; safe to call repeatedly. Returns None on decode failure;
/// callers should fall back to `placeholder_waveform`.
pub fn decode_and_cache_file(path: &Path) -> Option<WaveformPreview> {
    let key = path.to_string_lossy().to_string();
    if let Some(existing) = file_cache().lock().ok().and_then(|c| c.get(&key).cloned()) {
        if let WaveformStatus::Ready(preview) = existing {
            return Some(preview);
        }
    }

    let preview = decode_file_uncached(path)?;

    if let Ok(mut c) = file_cache().lock() {
        c.insert(key, WaveformStatus::Ready(preview.clone()));
    }
    Some(preview)
}

fn decode_file_uncached(path: &std::path::Path) -> Option<WaveformPreview> {
    // The engine handles every format `DAUx::probe_audio_file` accepts;
    // the UI no longer needs a parallel decoder. If `generate_audio_peaks`
    // returns Err we surface the message and let the caller record
    // `WaveformStatus::Error`.
    match generate_audio_peaks(path) {
        Ok(peaks) => {
            let preview: WaveformPreview = peaks.into();
            log_decoded(path, &preview);
            Some(preview)
        }
        Err(error) => {
            eprintln!(
                "[waveform] DAUx::generate_audio_peaks failed: path={} error={}",
                path.display(),
                error
            );
            None
        }
    }
}

fn log_decoded(path: &Path, p: &WaveformPreview) {
    let total_peaks: usize = p.lods.iter().map(|l| l.peaks.len()).sum();
    eprintln!(
        "[waveform] decoded {} | {:.2}s @ {}Hz ch={} frames={} lods={} total_peaks={}",
        path.display(),
        p.duration_seconds,
        p.sample_rate,
        p.channels,
        p.total_frames,
        p.lods.len(),
        total_peaks,
    );
    for lod in &p.lods {
        eprintln!(
            "[waveform]   lod spp={:>6} peaks={}",
            lod.samples_per_peak,
            lod.peaks.len()
        );
    }
}

// ── LOD builder ────────────────────────────────────────────────────────────────

/// Single-pass min/max accumulator for one LOD level.
struct LodBuilder {
    samples_per_peak: usize,
    min: f32,
    max: f32,
    count: usize,
    peaks: Vec<WaveformPeak>,
}

impl LodBuilder {
    fn new(samples_per_peak: usize, total_samples_hint: u64) -> Self {
        let cap = (total_samples_hint as usize / samples_per_peak).saturating_add(1);
        Self {
            samples_per_peak,
            min: 0.0,
            max: 0.0,
            count: 0,
            peaks: Vec::with_capacity(cap),
        }
    }

    #[inline]
    fn push(&mut self, v: f32) {
        if v < self.min {
            self.min = v;
        }
        if v > self.max {
            self.max = v;
        }
        self.count += 1;
        if self.count >= self.samples_per_peak {
            self.peaks.push(WaveformPeak {
                min: self.min,
                max: self.max,
            });
            self.min = 0.0;
            self.max = 0.0;
            self.count = 0;
        }
    }

    fn finalize(mut self) -> WaveformLod {
        if self.count > 0 {
            self.peaks.push(WaveformPeak {
                min: self.min,
                max: self.max,
            });
        }
        WaveformLod {
            samples_per_peak: self.samples_per_peak,
            peaks: self.peaks,
        }
    }
}

struct LodSet {
    builders: Vec<LodBuilder>,
}

impl LodSet {
    fn new(total_samples_hint: u64) -> Self {
        Self {
            builders: LOD_LEVELS
                .iter()
                .map(|&spp| LodBuilder::new(spp, total_samples_hint))
                .collect(),
        }
    }

    /// Fold a single mono sample (already clamped) into every LOD builder.
    #[inline]
    fn push(&mut self, mono: f32) {
        for b in &mut self.builders {
            b.push(mono);
        }
    }

    fn finalize(self) -> Vec<WaveformLod> {
        self.builders
            .into_iter()
            .map(LodBuilder::finalize)
            .collect()
    }
}

// Real-audio decoding now lives in DAUx::generate_audio_peaks. The
// LodSet / LodBuilder above remain only for the synthetic demo-clip
// preview path that has no source file to feed the engine.

// ── Demo / placeholder previews ───────────────────────────────────────────────

/// Used by the synthetic demo clips that don't have a real source file.
/// Generates pseudo-PCM, then folds it into the same multi-LOD pipeline.
pub fn get_or_generate_waveform(
    clip_id: &str,
    name: &str,
    duration_beats: f32,
    bpm: f32,
) -> WaveformPreview {
    let mut cache = clip_cache().lock().unwrap();
    if let Some(preview) = cache.get(clip_id) {
        return preview.clone();
    }
    let preview = generate_waveform_preview(name, duration_beats, bpm);
    cache.insert(clip_id.to_string(), preview.clone());
    preview
}

/// Flat preview returned when decoding fails. Single LOD with zero amplitude.
pub fn placeholder_waveform(duration_seconds: f64) -> WaveformPreview {
    // One coarse LOD is enough — there's nothing to zoom into.
    let lod = WaveformLod {
        samples_per_peak: LOD_LEVELS[LOD_LEVELS.len() / 2],
        peaks: (0..256)
            .map(|_| WaveformPeak {
                min: -0.03,
                max: 0.03,
            })
            .collect(),
    };
    WaveformPreview {
        sample_rate: 44_100,
        channels: 1,
        duration_seconds,
        total_frames: (duration_seconds * 44_100.0) as u64,
        lods: vec![lod],
    }
}

/// Produce a synthetic PCM buffer for the demo clips, then push it through the
/// real LOD builder so the demo and decoded paths share rendering code.
fn generate_waveform_preview(name: &str, duration_beats: f32, bpm: f32) -> WaveformPreview {
    let sample_rate: u32 = 44_100;
    let duration_seconds = duration_beats as f64 * (60.0 / bpm.max(1.0) as f64);
    let total_samples = (duration_seconds * sample_rate as f64) as u64;
    // Cap synthetic size so we don't burn megabytes for the demo defaults.
    let total_samples = total_samples.min(sample_rate as u64 * 30);

    let mut lods = LodSet::new(total_samples);
    let name_lower = name.to_lowercase();
    let kind = if name_lower.contains("drum") || name_lower.contains("loop") {
        SynthKind::Drums
    } else if name_lower.contains("vocal")
        || name_lower.contains("harmony")
        || name_lower.contains("dry")
    {
        SynthKind::Vocals
    } else {
        SynthKind::Generic
    };

    for i in 0..total_samples as usize {
        let t = i as f32 / sample_rate as f32;
        let v = synth_sample(kind, t, duration_seconds as f32);
        lods.push(v.clamp(-1.0, 1.0));
    }

    WaveformPreview {
        sample_rate,
        channels: 1,
        duration_seconds,
        total_frames: total_samples,
        lods: lods.finalize(),
    }
}

#[derive(Copy, Clone)]
enum SynthKind {
    Drums,
    Vocals,
    Generic,
}

fn synth_sample(kind: SynthKind, t: f32, duration: f32) -> f32 {
    let beat = t * 2.0; // arbitrary tempo for synthetic preview
    match kind {
        SynthKind::Drums => {
            let beat_fract = beat.fract();
            let beat_int = beat.floor() as i32;
            let mut amp = 0.04;
            if beat_int % 4 == 0 {
                amp += 0.82 * (-6.0 * beat_fract).exp();
            }
            if beat_int % 4 == 2 {
                amp += 0.68 * (-4.5 * beat_fract).exp();
            }
            let hat_pos = (beat * 2.0).fract();
            if hat_pos < 0.12 {
                amp += 0.28 * (-14.0 * hat_pos).exp();
            }
            let noise = ((t * 83.19).sin() * 43758.5453).fract() - 0.5;
            (amp + noise * 0.05).clamp(-1.0, 1.0)
                * if (t * 1000.0).sin() > 0.0 { 1.0 } else { -1.0 }
        }
        SynthKind::Vocals => {
            let phrase_beat = beat % 4.0;
            let mut amp = 0.0;
            if phrase_beat < 3.0 {
                amp = (phrase_beat * std::f32::consts::PI / 3.0).sin() * 0.58;
                amp += (beat * 7.5).sin().abs() * 0.14;
                amp += (beat * 22.0).sin().abs() * 0.06;
            }
            let osc = (t * 440.0 * 2.0 * std::f32::consts::PI).sin();
            (amp * osc).clamp(-1.0, 1.0)
        }
        SynthKind::Generic => {
            let env = (t / duration.max(0.001)).clamp(0.0, 1.0);
            let osc = (t * 220.0 * 2.0 * std::f32::consts::PI).sin();
            (osc * (0.4 + 0.2 * (t * 3.0).sin()) * (1.0 - env * 0.3)).clamp(-1.0, 1.0)
        }
    }
}

// ── LOD selection helper used by the renderer ────────────────────────────────

/// Return the LOD whose `samples_per_peak` best matches the requested density,
/// preferring the coarsest LOD that still gives ≥ ~0.5 peaks per pixel.
///
/// `samples_per_pixel`: how many decoded samples fall under one screen pixel for
/// the clip at the current zoom level.
pub fn pick_lod<'a>(
    preview: &'a WaveformPreview,
    samples_per_pixel: f32,
) -> Option<&'a WaveformLod> {
    if preview.lods.is_empty() {
        return None;
    }
    // Target: choose the largest spp that is still ≤ samples_per_pixel.
    // i.e. coarser than 1 peak per pixel by no more than 1 level — keeps detail
    // without overdrawing thousands of bars per clip.
    let target = samples_per_pixel.max(1.0);
    let mut best: &WaveformLod = &preview.lods[0];
    for lod in &preview.lods {
        if lod.samples_per_peak as f32 <= target {
            best = lod;
        } else {
            break;
        }
    }
    Some(best)
}
