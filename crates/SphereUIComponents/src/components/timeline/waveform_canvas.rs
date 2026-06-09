use super::timeline_state::{AudioImportState, ClipState, TimelineState};
use super::waveform_cache::{self, WaveformDisplayStatus, WaveformPeak};
use crate::theme::Colors;
use gpui::{
    canvas, div, fill, point, px, size, Bounds, IntoElement, ParentElement, Pixels, Styled,
};

/// One bar per visible CSS pixel column. No hard cap on number of bars —
/// the visible-range clamp naturally bounds it by viewport width.
pub fn waveform_canvas(
    clip: &ClipState,
    color: gpui::Rgba,
    state: &TimelineState,
    clip_left: f32,
    clip_width: f32,
) -> impl IntoElement {
    let _s = crate::perf::PerfScope::enter("WaveformCanvas");
    // Live recording take: draw streamed preview peaks (Part 1). Checked first
    // so the temporary preview clip never falls through to the file cache or the
    // synthetic placeholder.
    if let Some(preview) = waveform_cache::recording_preview(&clip.id) {
        return draw_preview_waveform(preview.as_ref(), color, state, clip_left, clip_width);
    }
    match clip.audio_asset_key() {
        Some(asset_key) => waveform_cache::with_file_entry(asset_key, |entry| {
            let Some(entry) = entry else {
                waveform_cache::record_timeline_render(1, 0, false);
                return import_status_canvas(&clip.audio_import, false, None);
            };
            match waveform_cache::display_status_from_entry(entry) {
                WaveformDisplayStatus::Ready { meta }
                | WaveformDisplayStatus::Partial { meta, .. } => {
                    let pixels_per_second = state.viewport.pixels_per_second;
                    draw_chunk_waveform_locked(
                        entry,
                        meta.as_ref(),
                        color,
                        clip,
                        state,
                        clip_left,
                        clip_width,
                        pixels_per_second,
                    )
                }
                WaveformDisplayStatus::Pending => {
                    waveform_cache::record_timeline_render(1, 0, false);
                    import_status_canvas(&clip.audio_import, false, None)
                }
                WaveformDisplayStatus::Error(message) => {
                    waveform_cache::record_timeline_render(1, 0, false);
                    import_status_canvas(&AudioImportState::Failed { message }, true, None)
                }
            }
        }),
        _ => {
            let preview = waveform_cache::get_or_generate_waveform(
                &clip.id,
                &clip.name,
                clip.duration_beats,
                state.bpm,
            );
            draw_preview_waveform(preview.as_ref(), color, state, clip_left, clip_width)
        }
    }
}

fn import_status_canvas(
    import: &AudioImportState,
    is_error: bool,
    _progress: Option<f32>,
) -> gpui::Div {
    let (label, show_progress) = match import {
        AudioImportState::Pending => ("Queued".to_string(), false),
        AudioImportState::Probing => ("Probing…".to_string(), true),
        AudioImportState::Decoding { .. } => ("Decoding…".to_string(), true),
        AudioImportState::GeneratingPeaks { progress } => {
            let pct = ((*progress * 100.0) as u32).min(100);
            (format!("Building waveform… {pct}%"), true)
        }
        AudioImportState::Ready => ("Ready".to_string(), false),
        AudioImportState::Failed { message } => (message.clone(), false),
    };

    let stripe = show_progress.then(|| {
        div()
            .absolute()
            .left_0()
            .right_0()
            .top(px(0.0))
            .h(px(2.0))
            .bg(Colors::with_alpha(Colors::accent_primary(), 0.55))
    });

    div()
        .relative()
        .size_full()
        .overflow_hidden()
        .bg(Colors::with_alpha(Colors::surface_base(), 0.35))
        .children(stripe)
        .child(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .rounded_sm()
                        .border(px(1.0))
                        .border_color(if is_error {
                            Colors::status_error()
                        } else {
                            Colors::border_subtle()
                        })
                        .bg(Colors::with_alpha(Colors::surface_base(), 0.72))
                        .px(px(6.0))
                        .py(px(2.0))
                        .text_size(px(9.0))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(if is_error {
                            Colors::status_error()
                        } else {
                            Colors::text_muted()
                        })
                        .child(label),
                ),
        )
}

/// One min/max bar per visible CSS pixel column. Bars are computed once and
/// drawn from a single GPUI `canvas` element via `paint_quad` — no per-pixel
/// child elements, no `MAX_VISIBLE_WIDTH` cap.
fn draw_chunk_waveform_locked(
    entry: &waveform_cache::FileEntry,
    meta: &waveform_cache::WaveformFileMeta,
    color: gpui::Rgba,
    clip: &ClipState,
    state: &TimelineState,
    clip_left: f32,
    clip_width: f32,
    pixels_per_second: f32,
) -> gpui::Div {
    // Visible portion of the clip in clip-local pixels.
    let viewport_w = state.viewport.viewport_width.max(1.0);
    let visible_start = (-clip_left).max(0.0);
    let visible_end = clip_width
        .min((viewport_w - clip_left).max(visible_start))
        .max(visible_start);
    let visible_w = (visible_end - visible_start).max(0.0);

    if visible_w < 1.0 {
        waveform_cache::record_timeline_render(1, 0, true);
        return empty_canvas();
    }

    let num_cols = (visible_w.ceil() as usize).max(1);
    waveform_cache::record_timeline_render(1, num_cols, true);
    crate::perf::count("peak_points_drawn", num_cols as u64);

    let desired_spp =
        waveform_cache::pick_best_samples_per_peak(pixels_per_second, meta.sample_rate);
    let spp = waveform_cache::best_available_samples_per_peak_in_entry(entry, desired_spp);
    let src_start = clip.offset_beats.max(0.0) as f64 * state.seconds_per_beat() as f64;
    let clip_dur = (clip.duration_beats as f64 * state.seconds_per_beat() as f64).max(1e-6);

    let h = 48.0_f32;
    let center = h / 2.0;
    let clip_w = clip_width.max(1.0) as f64;

    // Precompute bars: (x_in_canvas, top, height).
    let mut bars: Vec<(f32, f32, f32)> = Vec::with_capacity(num_cols);
    for col in 0..num_cols {
        let x0 = visible_start + col as f32;
        let x1 = x0 + 1.0;
        let frac0 = ((x0 as f64) / clip_w).clamp(0.0, 1.0);
        let frac1 = ((x1 as f64) / clip_w).clamp(0.0, 1.0);
        let t0 = src_start + frac0 * clip_dur;
        let t1 = src_start + frac1 * clip_dur;
        let p0 = time_to_peak_index(t0, meta.sample_rate, spp);
        let p1 = time_to_peak_index(t1, meta.sample_rate, spp).max(p0);
        let WaveformPeak { min, max } =
            waveform_cache::aggregate_peak_range_in_entry(entry, spp, p0, p1 + 1);
        if min == 0.0 && max == 0.0 {
            continue;
        }
        let mn = min.max(-1.0);
        let mx = max.min(1.0);
        let top = center - mx * center;
        let bottom = center - mn * center;
        let bar_h = (bottom - top).max(1.0);
        bars.push((x0, top, bar_h));
    }

    let mut waveform_color = color;
    waveform_color.a = 0.72;

    let element = canvas(
        |_bounds, _window, _cx| {},
        move |bounds: Bounds<Pixels>, (), window, _cx| {
            for (x, top, bh) in &bars {
                let r = Bounds::new(
                    bounds.origin + point(px(*x), px(*top)),
                    size(px(1.0), px(bh.max(1.0))),
                );
                window.paint_quad(fill(r, waveform_color));
            }
        },
    )
    .absolute()
    .inset_0();

    div()
        .relative()
        .size_full()
        .overflow_hidden()
        .child(element)
}

fn time_to_peak_index(time_sec: f64, sample_rate: u32, samples_per_peak: usize) -> usize {
    let frame = (time_sec * sample_rate as f64).max(0.0) as usize;
    frame / samples_per_peak.max(1)
}

fn draw_preview_waveform(
    preview: &waveform_cache::WaveformPreview,
    color: gpui::Rgba,
    state: &TimelineState,
    clip_left: f32,
    clip_width: f32,
) -> gpui::Div {
    let viewport_w = state.viewport.viewport_width.max(1.0);
    let visible_start = (-clip_left).max(0.0);
    let visible_end = clip_width
        .min((viewport_w - clip_left).max(visible_start))
        .max(visible_start);
    let visible_w = (visible_end - visible_start).max(0.0);
    if visible_w < 1.0 {
        return empty_canvas();
    }
    let samples_per_pixel = (preview.total_frames.max(1) as f32 / clip_width.max(1.0)).max(1.0);
    let Some(lod) = waveform_cache::pick_lod(preview, samples_per_pixel) else {
        return empty_canvas();
    };

    let num_cols = (visible_w.ceil() as usize).max(1);
    let h = 48.0_f32;
    let center = h / 2.0;
    let total_peaks = lod.peaks.len().max(1);
    let clip_w = clip_width.max(1.0);

    let mut bars: Vec<(f32, f32, f32)> = Vec::with_capacity(num_cols);
    for col in 0..num_cols {
        let x0 = visible_start + col as f32;
        let x1 = x0 + 1.0;
        let frac0 = (x0 / clip_w).max(0.0);
        let frac1 = (x1 / clip_w).min(1.0);
        let p0 = (frac0 * total_peaks as f32).floor() as usize;
        let p1 = (frac1 * total_peaks as f32).ceil() as usize;
        let end = p1.min(total_peaks).max(p0 + 1);
        let agg = aggregate_slice(&lod.peaks[p0..end]);
        let top = center - agg.max.min(1.0) * center;
        let bottom = center - agg.min.max(-1.0) * center;
        bars.push((x0, top, (bottom - top).max(1.0)));
    }

    let mut waveform_color = color;
    waveform_color.a = 0.72;

    let element = canvas(
        |_b, _w, _cx| {},
        move |bounds: Bounds<Pixels>, (), window, _cx| {
            for (x, top, bh) in &bars {
                let r = Bounds::new(
                    bounds.origin + point(px(*x), px(*top)),
                    size(px(1.0), px(bh.max(1.0))),
                );
                window.paint_quad(fill(r, waveform_color));
            }
        },
    )
    .absolute()
    .inset_0();

    div()
        .relative()
        .size_full()
        .overflow_hidden()
        .child(element)
}

fn aggregate_slice(peaks: &[waveform_cache::WaveformPeak]) -> waveform_cache::WaveformPeak {
    if peaks.is_empty() {
        return waveform_cache::WaveformPeak { min: 0.0, max: 0.0 };
    }
    let mut mn = peaks[0].min;
    let mut mx = peaks[0].max;
    for p in &peaks[1..] {
        if p.min < mn {
            mn = p.min;
        }
        if p.max > mx {
            mx = p.max;
        }
    }
    waveform_cache::WaveformPeak { min: mn, max: mx }
}

fn empty_canvas() -> gpui::Div {
    div().relative().size_full().overflow_hidden()
}
