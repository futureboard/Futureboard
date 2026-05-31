//! Builds audio editor view models from timeline + peak cache (no PCM).

use sphere_audio_editor::{WaveformColumn, WaveformViewModel};

use crate::components::timeline::timeline_state::{
    AudioImportState, ClipState, ClipType, TimelineState,
};
use crate::components::timeline::waveform_cache::{self, WaveformDisplayStatus, WaveformPeak};
use crate::theme::Colors;

const MAX_EDITOR_COLUMNS: usize = 2048;

pub fn import_status_label(import: &AudioImportState) -> (String, bool, bool) {
    match import {
        AudioImportState::Pending => ("Queued".to_string(), false, true),
        AudioImportState::Probing => ("Probing metadata…".to_string(), false, true),
        AudioImportState::Decoding { .. } => ("Preparing waveform…".to_string(), false, true),
        AudioImportState::GeneratingPeaks { progress } => {
            let pct = ((*progress * 100.0) as u32).min(100);
            (format!("Building waveform… {pct}%"), false, true)
        }
        AudioImportState::Ready => ("Ready".to_string(), false, false),
        AudioImportState::Failed { message } => (message.clone(), true, false),
    }
}

pub fn build_waveform_view_model(
    clip: &ClipState,
    state: &TimelineState,
    pixels_per_beat: f32,
    scroll_x: f32,
    viewport_width: f32,
) -> WaveformViewModel {
    let (label, is_error, show_progress) = import_status_label(&clip.audio_import);

    let ClipType::Audio {
        source_path: Some(path),
        ..
    } = &clip.clip_type
    else {
        return WaveformViewModel::loading("No source file");
    };

    waveform_cache::with_file_entry(path, |entry| {
        let Some(entry) = entry else {
            return WaveformViewModel {
                columns: Vec::new(),
                ready: false,
                status_label: label.clone(),
                is_error,
                show_progress,
            };
        };

        match waveform_cache::display_status_from_entry(entry) {
            WaveformDisplayStatus::Ready { meta } | WaveformDisplayStatus::Partial { meta, .. } => {
                let columns = build_peak_columns(
                    entry,
                    meta.as_ref(),
                    clip,
                    state,
                    pixels_per_beat,
                    scroll_x,
                    viewport_width,
                );
                let ready = !columns.is_empty();
                WaveformViewModel {
                    ready,
                    columns,
                    status_label: if ready { String::new() } else { label },
                    is_error: false,
                    show_progress: !ready && show_progress,
                }
            }
            WaveformDisplayStatus::Pending => WaveformViewModel {
                columns: Vec::new(),
                ready: false,
                status_label: label,
                is_error: false,
                show_progress: true,
            },
            WaveformDisplayStatus::Error(message) => WaveformViewModel::error(message),
        }
    })
}

fn build_peak_columns(
    entry: &waveform_cache::FileEntry,
    meta: &waveform_cache::WaveformFileMeta,
    clip: &ClipState,
    state: &TimelineState,
    pixels_per_beat: f32,
    scroll_x: f32,
    viewport_width: f32,
) -> Vec<WaveformColumn> {
    let clip_width_px = (clip.duration_beats * pixels_per_beat).max(1.0);
    let visible_start = scroll_x.max(0.0);
    let visible_end = (scroll_x + viewport_width).min(clip_width_px);
    let visible_w = (visible_end - visible_start).max(1.0);
    let num_cols = (visible_w.ceil() as usize).clamp(16, MAX_EDITOR_COLUMNS);

    let seconds_per_beat = state.seconds_per_beat();
    let src_start = clip.offset_beats.max(0.0) as f64 * seconds_per_beat as f64;
    let clip_dur = (clip.duration_beats as f64 * seconds_per_beat as f64).max(1e-6);

    let pixels_per_second = pixels_per_beat / seconds_per_beat.max(1e-6);
    let desired_spp =
        waveform_cache::pick_best_samples_per_peak(pixels_per_second, meta.sample_rate);
    let spp = waveform_cache::best_available_samples_per_peak_in_entry(entry, desired_spp);

    (0..num_cols)
        .filter_map(|col| {
            let x0 = visible_start + (col as f32 / num_cols as f32) * visible_w;
            let x1 = visible_start + ((col + 1) as f32 / num_cols as f32) * visible_w;
            let frac0 = (x0 / clip_width_px).clamp(0.0, 1.0) as f64;
            let frac1 = (x1 / clip_width_px).clamp(0.0, 1.0) as f64;
            let t0 = src_start + frac0 * clip_dur;
            let t1 = src_start + frac1 * clip_dur;
            let p0 = time_to_peak_index(t0, meta.sample_rate, spp);
            let p1 = time_to_peak_index(t1, meta.sample_rate, spp).max(p0);
            let WaveformPeak { min, max } =
                waveform_cache::aggregate_peak_range_in_entry(entry, spp, p0, p1 + 1);
            if min == 0.0 && max == 0.0 {
                return None;
            }
            Some(WaveformColumn {
                x: x0 - scroll_x,
                min,
                max,
            })
        })
        .collect()
}

fn time_to_peak_index(time_sec: f64, sample_rate: u32, samples_per_peak: usize) -> usize {
    let frame = (time_sec * sample_rate as f64).max(0.0) as usize;
    frame / samples_per_peak.max(1)
}

pub fn audio_editor_theme() -> sphere_audio_editor::AudioEditorTheme {
    sphere_audio_editor::AudioEditorTheme {
        surface_base: Colors::surface_base(),
        surface_panel: Colors::surface_panel(),
        text_primary: Colors::text_primary(),
        text_secondary: Colors::text_secondary(),
        text_muted: Colors::text_muted(),
        border_subtle: Colors::border_subtle(),
        accent: Colors::accent_primary(),
        playhead: Colors::timeline_playhead(),
        error: Colors::status_error(),
        selection: Colors::accent_primary(),
    }
}

pub fn selected_audio_clip<'a>(
    state: &'a TimelineState,
) -> Option<(
    &'a crate::components::timeline::timeline_state::TrackState,
    &'a ClipState,
)> {
    let clip_id = state.selection.selected_clip_ids.first()?;
    let (track, clip) = state.find_clip(clip_id)?;
    match clip.clip_type {
        ClipType::Audio { .. } => Some((track, clip)),
        ClipType::Midi { .. } => None,
    }
}

pub fn clip_type_hint_for_selection(
    state: &TimelineState,
) -> Option<sphere_audio_editor::ClipTypeHint> {
    let clip_id = state.selection.selected_clip_ids.first()?;
    let (_, clip) = state.find_clip(clip_id)?;
    match clip.clip_type {
        ClipType::Audio { .. } => Some(sphere_audio_editor::ClipTypeHint::Audio),
        ClipType::Midi { .. } => Some(sphere_audio_editor::ClipTypeHint::Midi),
    }
}
