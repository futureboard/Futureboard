use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Copy)]
pub struct WaveformPeak {
    pub min: f32,
    pub max: f32,
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

#[derive(Debug, Clone)]
pub enum WaveformStatus {
    Pending,
    Ready(WaveformPreview),
    Error(String),
}

/// LOD levels exactly as required: a power-of-two ladder from 256 to 65536.
/// Anything inside this range is one bilinear interp away from the right level
/// of detail for the zoom factor.
pub const LOD_LEVELS: [usize; 9] = [256, 512, 1024, 2048, 4096, 8192, 16384, 32768, 65536];

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

fn decode_file_uncached(path: &Path) -> Option<WaveformPreview> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();

    let preview = match ext.as_str() {
        "wav" => decode_wav(path),
        "mp3" | "flac" | "ogg" => decode_via_symphonia(path),
        _ => None,
    }?;

    log_decoded(path, &preview);
    Some(preview)
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

// ── Decoders ──────────────────────────────────────────────────────────────────

fn decode_wav(path: &Path) -> Option<WaveformPreview> {
    let mut reader = hound::WavReader::open(path).ok()?;
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;
    let sample_rate = spec.sample_rate;
    let total_frames = reader.duration() as u64; // frames per channel

    let mut lods = LodSet::new(total_frames);
    let mut frames_seen: u64 = 0;
    let int_scale = if spec.bits_per_sample > 0 {
        (1u32 << (spec.bits_per_sample.saturating_sub(1))) as f32
    } else {
        1.0
    };

    match spec.sample_format {
        hound::SampleFormat::Float => {
            let mut iter = reader.samples::<f32>();
            loop {
                let mut sum = 0.0_f32;
                let mut got = 0usize;
                for _ in 0..channels {
                    match iter.next() {
                        Some(Ok(s)) => {
                            sum += s;
                            got += 1;
                        }
                        _ => break,
                    }
                }
                if got == 0 {
                    break;
                }
                let mono = (sum / got as f32).clamp(-1.0, 1.0);
                lods.push(mono);
                frames_seen += 1;
            }
        }
        hound::SampleFormat::Int => {
            let mut iter = reader.samples::<i32>();
            loop {
                let mut sum = 0.0_f32;
                let mut got = 0usize;
                for _ in 0..channels {
                    match iter.next() {
                        Some(Ok(s)) => {
                            sum += s as f32 / int_scale;
                            got += 1;
                        }
                        _ => break,
                    }
                }
                if got == 0 {
                    break;
                }
                let mono = (sum / got as f32).clamp(-1.0, 1.0);
                lods.push(mono);
                frames_seen += 1;
            }
        }
    }

    if frames_seen == 0 {
        return None;
    }

    let duration_seconds = if sample_rate > 0 {
        frames_seen as f64 / sample_rate as f64
    } else {
        0.0
    };

    Some(WaveformPreview {
        sample_rate,
        channels: channels as u16,
        duration_seconds,
        total_frames: frames_seen,
        lods: lods.finalize(),
    })
}

fn decode_via_symphonia(path: &Path) -> Option<WaveformPreview> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::DecoderOptions;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let file = std::fs::File::open(path).ok()?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .ok()?;
    let mut format = probed.format;
    let track = format.default_track()?.clone();
    let track_id = track.id;
    let sample_rate = track.codec_params.sample_rate.unwrap_or(44_100);
    let channels = track
        .codec_params
        .channels
        .map(|c| c.count() as u16)
        .unwrap_or(1)
        .max(1);
    let total_frames = track.codec_params.n_frames.unwrap_or(0);

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .ok()?;

    let mut lods = LodSet::new(total_frames);
    let mut sample_buf: Option<SampleBuffer<f32>> = None;
    let mut frames_decoded: u64 = 0;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(_) => break,
        };
        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(_) => break,
        };

        if sample_buf.is_none() {
            sample_buf = Some(SampleBuffer::<f32>::new(
                decoded.capacity() as u64,
                *decoded.spec(),
            ));
        }
        let buf = sample_buf.as_mut()?;
        buf.copy_interleaved_ref(decoded);

        let interleaved = buf.samples();
        let ch_count = channels as usize;
        let mut i = 0;
        while i + ch_count <= interleaved.len() {
            let mut sum = 0.0_f32;
            for c in 0..ch_count {
                sum += interleaved[i + c];
            }
            let mono = (sum / ch_count as f32).clamp(-1.0, 1.0);
            lods.push(mono);
            i += ch_count;
            frames_decoded += 1;
        }
    }

    if frames_decoded == 0 {
        return None;
    }

    let metadata_frames = total_frames.max(frames_decoded);
    let duration_seconds = metadata_frames as f64 / sample_rate as f64;

    Some(WaveformPreview {
        sample_rate,
        channels,
        duration_seconds,
        total_frames: metadata_frames,
        lods: lods.finalize(),
    })
}

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
