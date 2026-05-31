//! Immutable render snapshots built on the UI thread from [`TimelineState`].
//!
//! Renderers must treat these as read-only: no audio decode, no peak generation.

use std::sync::Arc;

use super::viewport::TimelineViewport;
use crate::components::timeline::timeline_state::{
    ClipState, ClipType, GridLineLevel, TimelineState, TrackState, TRACK_HEIGHT,
};
use crate::components::timeline::waveform_cache::{
    self, WaveformDisplayStatus, CHUNK_PEAKS, PEAK_FINE_SPP,
};
use gpui::Rgba;

/// Track rows included in this snapshot (after vertical virtualization).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibleTrackRange {
    pub start_index: usize,
    pub end_index: usize,
}

/// Beat interval visible in the lane viewport.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VisibleBeatRange {
    pub start_beat: f32,
    pub end_beat: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderClipKind {
    Audio,
    Midi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaveformReadyKind {
    Pending,
    Partial,
    Ready,
    Error,
}

/// Opaque handle to precomputed peak chunks — WGPU path binds GPU buffers from cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaveformChunkHandle {
    pub source_path: String,
    pub samples_per_peak: u32,
    pub chunk_index_start: u32,
    pub chunk_index_end: u32,
    pub peak_index_start: usize,
    pub peak_index_end: usize,
    pub ready: WaveformReadyKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderClipSnapshot {
    pub id: String,
    pub track_id: String,
    pub track_index: usize,
    pub name: String,
    pub kind: RenderClipKind,
    pub color: [f32; 4],
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub selected: bool,
    pub muted: bool,
    pub waveform: Option<WaveformChunkHandle>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderLaneSnapshot {
    pub track_index: usize,
    pub track_id: String,
    pub y: f32,
    pub height: f32,
    pub even_row: bool,
    pub selected: bool,
    pub color: [f32; 4],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridLineSnapshot {
    pub x: f32,
    pub beat: f32,
    pub level: GridLineLevel,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarShadeSnapshot {
    pub x: f32,
    pub width: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayheadSnapshot {
    pub beat: f32,
    pub x: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SelectionSnapshot {
    pub selected_track_id: Option<String>,
    pub selected_clip_ids: Vec<String>,
}

/// Immutable description of one arrangement paint pass.
#[derive(Debug, Clone, PartialEq)]
pub struct TimelineRenderSnapshot {
    pub viewport: TimelineViewport,
    pub bpm: f32,
    pub beats_per_bar: f32,
    pub visible_tracks: VisibleTrackRange,
    pub visible_beats: VisibleBeatRange,
    pub lanes: Vec<RenderLaneSnapshot>,
    pub clips: Vec<RenderClipSnapshot>,
    pub grid_lines: Vec<GridLineSnapshot>,
    pub bar_shades: Vec<BarShadeSnapshot>,
    pub playhead: PlayheadSnapshot,
    pub selection: SelectionSnapshot,
    pub track_insert_y: Option<f32>,
}

pub struct SnapshotBuildOptions {
    pub scale_factor: f32,
    pub track_overscan: usize,
}

impl Default for SnapshotBuildOptions {
    fn default() -> Self {
        Self {
            scale_factor: 1.0,
            track_overscan: 2,
        }
    }
}

impl TimelineRenderSnapshot {
    pub fn from_state(state: &TimelineState, options: SnapshotBuildOptions) -> Self {
        let grid_width = state.viewport.viewport_width.max(1.0);
        let grid_height = state.viewport.viewport_height.max(TRACK_HEIGHT);
        let seconds_per_beat = state.seconds_per_beat();
        let pixels_per_beat = state.viewport.pixels_per_second * seconds_per_beat;

        let viewport = TimelineViewport::new(
            grid_width,
            grid_height,
            options.scale_factor,
            state.viewport.scroll_x,
            state.viewport.scroll_y,
            pixels_per_beat,
            state.viewport.pixels_per_second,
            seconds_per_beat,
        );

        let visible_tracks = visible_track_range(state, options.track_overscan);
        let visible_beats = VisibleBeatRange {
            start_beat: viewport.visible_beat_range().0,
            end_beat: viewport.visible_beat_range().1,
        };

        let lanes = build_lanes(state, &visible_tracks);
        let clips = build_clips(state, &visible_tracks, &viewport);
        let grid_lines = state
            .get_arrangement_grid_lines(grid_width)
            .into_iter()
            .map(|line| GridLineSnapshot {
                x: line.x,
                beat: line.beat,
                level: line.level,
            })
            .collect();
        let bar_shades = build_bar_shades(&viewport, state.beats_per_bar());

        let playhead = PlayheadSnapshot {
            beat: state.transport.playhead_beats,
            x: viewport.beat_to_x(state.transport.playhead_beats),
        };

        let selection = SelectionSnapshot {
            selected_track_id: state.selection.selected_track_id.clone(),
            selected_clip_ids: state.selection.selected_clip_ids.clone(),
        };

        let track_insert_y = state.drag_target_index.map(|index| {
            (index as f32 * TRACK_HEIGHT - state.viewport.scroll_y)
                .clamp(0.0, grid_height.max(TRACK_HEIGHT))
        });

        Self {
            viewport,
            bpm: state.bpm,
            beats_per_bar: state.beats_per_bar(),
            visible_tracks,
            visible_beats,
            lanes,
            clips,
            grid_lines,
            bar_shades,
            playhead,
            selection,
            track_insert_y,
        }
    }
}

fn visible_track_range(state: &TimelineState, overscan: usize) -> VisibleTrackRange {
    let track_count = state.tracks.len();
    if track_count == 0 {
        return VisibleTrackRange {
            start_index: 0,
            end_index: 0,
        };
    }
    let scroll_y = state.viewport.scroll_y;
    let viewport_height = state.viewport.viewport_height;
    let first_visible = (scroll_y / TRACK_HEIGHT).floor() as usize;
    let visible_start = first_visible.saturating_sub(overscan);
    let last_visible = ((scroll_y + viewport_height) / TRACK_HEIGHT).ceil() as usize;
    let visible_end = (last_visible + overscan).min(track_count);
    VisibleTrackRange {
        start_index: visible_start,
        end_index: visible_end,
    }
}

fn build_lanes(state: &TimelineState, range: &VisibleTrackRange) -> Vec<RenderLaneSnapshot> {
    state.tracks[range.start_index..range.end_index]
        .iter()
        .enumerate()
        .map(|(rel, track)| {
            let index = range.start_index + rel;
            let y = index as f32 * TRACK_HEIGHT - state.viewport.scroll_y;
            RenderLaneSnapshot {
                track_index: index,
                track_id: track.id.clone(),
                y,
                height: TRACK_HEIGHT,
                even_row: index % 2 == 0,
                selected: state.selection.selected_track_id.as_deref() == Some(track.id.as_str()),
                color: rgba_to_array(track.color),
            }
        })
        .collect()
}

fn build_clips(
    state: &TimelineState,
    range: &VisibleTrackRange,
    viewport: &TimelineViewport,
) -> Vec<RenderClipSnapshot> {
    let mut clips = Vec::new();
    let pad = 7.0_f32;
    let clip_h = TRACK_HEIGHT - pad * 2.0;

    for (rel, track) in state.tracks[range.start_index..range.end_index]
        .iter()
        .enumerate()
    {
        let track_index = range.start_index + rel;
        for clip in &track.clips {
            let clip_left = viewport.beat_to_x(clip.start_beat);
            let clip_width =
                (clip.duration_beats * viewport.seconds_per_beat * viewport.pixels_per_second)
                    .max(10.0);
            if clip_left + clip_width < 0.0 || clip_left > viewport.width {
                continue;
            }
            let clip_y = track_index as f32 * TRACK_HEIGHT - state.viewport.scroll_y + pad;
            clips.push(build_clip_snapshot(
                clip,
                track,
                track_index,
                clip_left,
                clip_y,
                clip_width,
                clip_h,
                state,
                viewport,
            ));
        }
    }
    clips
}

fn build_clip_snapshot(
    clip: &ClipState,
    track: &TrackState,
    track_index: usize,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    state: &TimelineState,
    viewport: &TimelineViewport,
) -> RenderClipSnapshot {
    let kind = match clip.clip_type {
        ClipType::Audio { .. } => RenderClipKind::Audio,
        ClipType::Midi { .. } => RenderClipKind::Midi,
    };
    let waveform = match &clip.clip_type {
        ClipType::Audio {
            source_path: Some(path),
            ..
        } => Some(waveform_handle_for_clip(path, clip, state, viewport)),
        _ => None,
    };

    RenderClipSnapshot {
        id: clip.id.clone(),
        track_id: track.id.clone(),
        track_index,
        name: clip.name.clone(),
        kind,
        color: rgba_to_array(track.color),
        x,
        y,
        width,
        height,
        selected: state.selection.selected_clip_ids.contains(&clip.id),
        muted: clip.muted,
        waveform,
    }
}

fn waveform_handle_for_clip(
    path: &str,
    clip: &ClipState,
    state: &TimelineState,
    viewport: &TimelineViewport,
) -> WaveformChunkHandle {
    let status = waveform_cache::get_file_status(path);
    let ready = match &status {
        WaveformDisplayStatus::Ready { .. } => WaveformReadyKind::Ready,
        WaveformDisplayStatus::Partial { .. } => WaveformReadyKind::Partial,
        WaveformDisplayStatus::Pending => WaveformReadyKind::Pending,
        WaveformDisplayStatus::Error(_) => WaveformReadyKind::Error,
    };

    let (samples_per_peak, peak_count) = match status {
        WaveformDisplayStatus::Ready { meta } | WaveformDisplayStatus::Partial { meta, .. } => {
            let spp = waveform_cache::pick_best_samples_per_peak(
                viewport.pixels_per_second,
                meta.sample_rate,
            ) as u32;
            (spp, meta.peak_count)
        }
        _ => (PEAK_FINE_SPP as u32, 0),
    };

    let src_start = clip.offset_beats.max(0.0) as f64 * state.seconds_per_beat() as f64;
    let clip_dur = (clip.duration_beats as f64 * state.seconds_per_beat() as f64).max(1e-6);
    let frac0 = 0.0_f64;
    let frac1 = 1.0_f64;
    let t0 = src_start + frac0 * clip_dur;
    let t1 = src_start + frac1 * clip_dur;
    let sample_rate = waveform_cache::get_file_status(path)
        .ready_meta()
        .map(|m| m.sample_rate)
        .unwrap_or(48_000);
    let p0 = time_to_peak_index(t0, sample_rate, samples_per_peak as usize);
    let p1 = time_to_peak_index(t1, sample_rate, samples_per_peak as usize)
        .max(p0)
        .min(peak_count.saturating_sub(1));

    WaveformChunkHandle {
        source_path: path.to_string(),
        samples_per_peak,
        chunk_index_start: (p0 / CHUNK_PEAKS) as u32,
        chunk_index_end: (p1 / CHUNK_PEAKS) as u32,
        peak_index_start: p0,
        peak_index_end: p1,
        ready,
    }
}

fn time_to_peak_index(time_sec: f64, sample_rate: u32, samples_per_peak: usize) -> usize {
    let frame = (time_sec * sample_rate as f64).max(0.0) as usize;
    frame / samples_per_peak.max(1)
}

fn build_bar_shades(viewport: &TimelineViewport, beats_per_bar: f32) -> Vec<BarShadeSnapshot> {
    let bar_w = beats_per_bar * viewport.pixels_per_beat;
    if bar_w < 2.0 {
        return Vec::new();
    }
    let start_beat = viewport.scroll_x / viewport.pixels_per_beat;
    let first_bar = (start_beat / beats_per_bar).floor() as i32;
    let last_bar = ((viewport.scroll_x + viewport.width) / bar_w).ceil() as i32;
    let mut shades = Vec::new();
    for bar in first_bar..=last_bar {
        if bar % 2 == 0 {
            shades.push(BarShadeSnapshot {
                x: (bar as f32 * bar_w - viewport.scroll_x).round(),
                width: bar_w.round(),
            });
        }
    }
    shades
}

fn rgba_to_array(c: Rgba) -> [f32; 4] {
    [c.r, c.g, c.b, c.a]
}

trait WaveformStatusExt {
    fn ready_meta(&self) -> Option<&Arc<waveform_cache::WaveformFileMeta>>;
}

impl WaveformStatusExt for WaveformDisplayStatus {
    fn ready_meta(&self) -> Option<&Arc<waveform_cache::WaveformFileMeta>> {
        match self {
            WaveformDisplayStatus::Ready { meta } | WaveformDisplayStatus::Partial { meta, .. } => {
                Some(meta)
            }
            _ => None,
        }
    }
}
