use gpui::{div, px, IntoElement, ParentElement, Styled};
use crate::components::timeline::timeline_state::{ClipState, ClipType, TimelineState};
use super::waveform_cache::{self, WaveformPeak, WaveformPreview};

/// Soft upper bound on how far past the clip start we'll attempt to render.
/// The clip element itself sets `overflow_hidden`, so anything outside this is
/// cheap to clip; this is just to keep the bar count finite for very long
/// clips that have been zoomed to thousands of pixels.
const MAX_VISIBLE_WIDTH: f32 = 4096.0;

/// Hard cap on the number of bars rendered per clip. Practical perf ceiling —
/// we never need this many on screen at once.
const MAX_BARS: usize = 1200;

pub fn waveform_canvas(
    clip: &ClipState,
    color: gpui::Rgba,
    state: &TimelineState,
    clip_left: f32,
    clip_width: f32,
) -> impl IntoElement {
    // ── Resolve preview ──────────────────────────────────────────────────────
    let preview = match &clip.clip_type {
        ClipType::Audio { source_path: Some(path), .. } => {
            waveform_cache::get_file_waveform(path).unwrap_or_else(|| {
                waveform_cache::placeholder_waveform(clip.duration_beats * 60.0 / state.bpm.max(1.0))
            })
        }
        _ => waveform_cache::get_or_generate_waveform(
            &clip.id,
            &clip.name,
            clip.duration_beats,
            state.bpm,
        ),
    };

    // ── Visible clip-relative range ──────────────────────────────────────────
    // `clip_left` is screen-space (already shifted by -scroll_x), so anything
    // negative is to the left of the viewport. We render only the portion that
    // could plausibly be visible and lean on the clip's overflow_hidden to
    // clip the rest.
    let visible_start = (-clip_left).max(0.0);
    let visible_end = (clip_width).min(visible_start + MAX_VISIBLE_WIDTH);
    let visible_w = (visible_end - visible_start).max(1.0);

    // ── LOD selection ────────────────────────────────────────────────────────
    // Total decoded samples covered by the *full* clip width.
    let total_samples = preview.total_samples.max(1) as f32;
    let samples_per_pixel = (total_samples / clip_width.max(1.0)).max(1.0);
    let Some(lod) = waveform_cache::pick_lod(&preview, samples_per_pixel) else {
        return empty_canvas();
    };
    let total_peaks = lod.peaks.len().max(1);

    // ── Bar grid ─────────────────────────────────────────────────────────────
    // Three visual regimes, picked from how many peaks the chosen LOD packs
    // into one screen pixel:
    //   * fine    (≥ ~1 peak per px)  → 1 px solid columns, step 1 → crisp
    //   * medium  (~0.5..1 peak/px)   → 1 px columns, step 2
    //   * sparse  (zoomed way out)    → 2 px columns, step 3
    // The bar count is capped so a 4k-wide clip can't blow up the layout.
    let peaks_per_pixel = (total_peaks as f32 / clip_width.max(1.0)).max(0.0001);
    let (bar_width, step) = if peaks_per_pixel >= 1.0 {
        (1.0_f32, 1.0_f32)
    } else if peaks_per_pixel >= 0.5 {
        (1.0_f32, 2.0_f32)
    } else {
        (2.0_f32, 3.0_f32)
    };
    let raw_bars = (visible_w / step).floor() as usize;
    let num_bars = raw_bars.clamp(8, MAX_BARS);

    let h = 48.0_f32;
    let center = h / 2.0;
    let mut waveform_color = color;
    waveform_color.a = 0.72;

    // Pixel → peak-index mapping. We work in the clip-relative coordinate space
    // and map each bar to a contiguous range of peaks aggregated by min/max.
    let peaks_per_full_clip = total_peaks as f32;
    let bars_to_full_ratio = peaks_per_full_clip / (clip_width / step).max(1.0);

    let bar_elements: Vec<_> = (0..num_bars)
        .filter_map(|i| {
            let bar_left = visible_start + i as f32 * step;
            if bar_left + bar_width <= visible_start { return None; }
            if bar_left >= visible_end { return None; }

            // Pixel position within the *full* clip (not just the visible slice).
            let pixel_in_clip_start = bar_left;
            let pixel_in_clip_end = bar_left + step;

            // Convert pixel range → peak index range
            let start_peak_f = (pixel_in_clip_start / step) * bars_to_full_ratio;
            let end_peak_f = (pixel_in_clip_end / step) * bars_to_full_ratio;
            let start_peak = start_peak_f.floor() as usize;
            let end_peak = (end_peak_f.ceil() as usize).max(start_peak + 1).min(total_peaks);
            if start_peak >= total_peaks { return None; }

            let WaveformPeak { min: mn, max: mx } = aggregate(&lod.peaks[start_peak..end_peak]);
            let mn = mn.max(-1.0);
            let mx = mx.min(1.0);

            // Map amplitude [-1, 1] → screen-y. y grows downward, so the
            // positive max sits *above* center (smaller y) and the negative
            // min sits *below* center (larger y).
            let top = center - mx * center;
            let bottom = center - mn * center;
            let bar_h = (bottom - top).max(1.0);

            Some(
                div()
                    .absolute()
                    .left(px(bar_left))
                    .top(px(top))
                    .w(px(bar_width))
                    .h(px(bar_h))
                    .bg(waveform_color),
            )
        })
        .collect();

    div()
        .relative()
        .size_full()
        .overflow_hidden()
        .children(bar_elements)
}

fn empty_canvas() -> gpui::Div {
    div().relative().size_full().overflow_hidden()
}

#[inline]
fn aggregate(peaks: &[WaveformPeak]) -> WaveformPeak {
    if peaks.is_empty() {
        return WaveformPeak { min: 0.0, max: 0.0 };
    }
    let mut mn = peaks[0].min;
    let mut mx = peaks[0].max;
    for p in &peaks[1..] {
        if p.min < mn { mn = p.min; }
        if p.max > mx { mx = p.max; }
    }
    WaveformPeak { min: mn, max: mx }
}

#[cfg(debug_assertions)]
#[allow(dead_code)]
fn debug_lod_pick(preview: &WaveformPreview, lod_spp: usize, samples_per_pixel: f32) {
    eprintln!(
        "[waveform-render] spp/pixel={:.1} → picked lod spp={} ({} peaks)",
        samples_per_pixel,
        lod_spp,
        preview
            .lods
            .iter()
            .find(|l| l.samples_per_peak == lod_spp)
            .map(|l| l.peaks.len())
            .unwrap_or(0),
    );
}
