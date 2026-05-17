//! Runtime playback graph sent to the CPAL callback.
//!
//! The control thread builds this from an `EngineProjectSnapshot`, including
//! decoding supported media files.  The audio thread then owns a local clone of
//! the graph and can render without touching locks or parsing JSON.

use std::collections::HashMap;
use std::sync::Arc;

use crate::audio_file::{load_audio_file, AudioFileBuffer};
use crate::types::{EngineClipSnapshot, EngineProjectSnapshot};

#[derive(Debug, Clone)]
pub struct RuntimeTrack {
    pub id: String,
    pub volume: f32,
    pub pan: f32,
    pub muted: bool,
    pub solo: bool,
}

#[derive(Debug, Clone)]
pub struct RuntimeClip {
    pub id: String,
    pub track_id: String,
    pub start_sample: u64,
    pub duration_samples: u64,
    pub offset_seconds: f64,
    pub gain: f32,
    pub speed_ratio: f32,
    pub source: Arc<AudioFileBuffer>,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeProject {
    pub sample_rate: u32,
    pub tracks: Vec<RuntimeTrack>,
    pub clips: Vec<RuntimeClip>,
    pub has_solo: bool,
}

impl RuntimeProject {
    pub fn build(
        snapshot: &EngineProjectSnapshot,
        output_sample_rate: u32,
        decoded_by_path: &mut HashMap<String, Arc<AudioFileBuffer>>,
    ) -> Self {
        let output_sample_rate = output_sample_rate.max(1);
        let beats_per_second = snapshot.bpm.max(1.0) / 60.0;
        let mut clips = Vec::new();
        let mut skipped_no_path = 0u32;
        let mut skipped_decode_err = 0u32;
        let mut loaded_from_cache = 0u32;
        let mut loaded_fresh = 0u32;

        for clip in &snapshot.clips {
            let Some(path) = clip.media_path.as_deref().filter(|p| !p.trim().is_empty()) else {
                eprintln!(
                    "[SphereAudio] clip '{}' (track={}) — no mediaPath, skipping",
                    clip.id, clip.track_id
                );
                skipped_no_path += 1;
                continue;
            };

            let source = match decoded_by_path.get(path) {
                Some(existing) => {
                    eprintln!(
                        "[SphereAudio] clip '{}' — cache hit: '{path}' ({} frames)",
                        clip.id, existing.frames
                    );
                    loaded_from_cache += 1;
                    Arc::clone(existing)
                }
                None => match load_audio_file(path) {
                    Ok(buffer) => {
                        eprintln!(
                            "[SphereAudio] clip '{}' — decoded: '{path}' {} frames @ {}Hz {} ch",
                            clip.id, buffer.frames, buffer.sample_rate, buffer.channels
                        );
                        loaded_fresh += 1;
                        let buffer = Arc::new(buffer);
                        decoded_by_path.insert(path.to_string(), Arc::clone(&buffer));
                        buffer
                    }
                    Err(e) => {
                        skipped_decode_err += 1;
                        eprintln!("[SphereAudio] clip '{}' — decode FAILED '{path}': {e}", clip.id);
                        continue;
                    }
                },
            };

            let Some(runtime_clip) = build_clip_runtime(
                clip,
                Arc::clone(&source),
                beats_per_second,
                output_sample_rate,
            ) else {
                skipped_decode_err += 1;
                continue;
            };
            clips.push(runtime_clip);
        }

        if skipped_no_path > 0 || skipped_decode_err > 0 || loaded_fresh > 0 {
            eprintln!(
                "[SphereAudio] RuntimeProject built: {} clips ready ({} cached, {} decoded), \
                 {} skipped (no path), {} decode errors",
                clips.len(),
                loaded_from_cache,
                loaded_fresh,
                skipped_no_path,
                skipped_decode_err,
            );
        }

        let tracks: Vec<RuntimeTrack> = snapshot
            .tracks
            .iter()
            .map(|t| RuntimeTrack {
                id: t.id.clone(),
                volume: t.volume.clamp(0.0, 2.0),
                pan: t.pan.clamp(-1.0, 1.0),
                muted: t.muted,
                solo: t.solo,
            })
            .collect();
        let has_solo = tracks.iter().any(|t| t.solo);

        Self {
            sample_rate: output_sample_rate,
            tracks,
            clips,
            has_solo,
        }
    }

    #[inline]
    pub fn active_clip_count_at_sample(&self, project_sample: u64) -> usize {
        self.clips
            .iter()
            .filter(|clip| {
                project_sample >= clip.start_sample
                    && project_sample < clip.start_sample.saturating_add(clip.duration_samples)
            })
            .count()
    }

    #[inline]
    pub fn update_track_volume(&mut self, track_id: &str, volume: f32) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            track.volume = volume.clamp(0.0, 2.0);
        }
    }

    #[inline]
    pub fn update_track_pan(&mut self, track_id: &str, pan: f32) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            track.pan = pan.clamp(-1.0, 1.0);
        }
    }

    #[inline]
    pub fn update_track_mute(&mut self, track_id: &str, muted: bool) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            track.muted = muted;
        }
    }

    #[inline]
    pub fn update_track_solo(&mut self, track_id: &str, solo: bool) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            track.solo = solo;
            self.has_solo = self.tracks.iter().any(|t| t.solo);
        }
    }
}

fn build_clip_runtime(
    clip: &EngineClipSnapshot,
    source: Arc<AudioFileBuffer>,
    beats_per_second: f64,
    output_sample_rate: u32,
) -> Option<RuntimeClip> {
    if beats_per_second <= 0.0 || output_sample_rate == 0 {
        return None;
    }

    let start_seconds = clip.start_beat / beats_per_second;
    let duration_seconds = clip.duration_beats / beats_per_second;
    if duration_seconds <= 0.0 {
        return None;
    }

    let speed_ratio = clip
        .audio_process
        .as_ref()
        .map(|p| p.speed_ratio as f32)
        .unwrap_or(1.0)
        .clamp(0.01, 16.0);

    Some(RuntimeClip {
        id: clip.id.clone(),
        track_id: clip.track_id.clone(),
        start_sample: seconds_to_samples(start_seconds.max(0.0), output_sample_rate),
        duration_samples: seconds_to_samples(duration_seconds, output_sample_rate).max(1),
        offset_seconds: clip.offset_seconds.max(0.0),
        gain: clip.gain.clamp(0.0, 4.0),
        speed_ratio,
        source,
    })
}

#[inline]
fn seconds_to_samples(seconds: f64, sample_rate: u32) -> u64 {
    (seconds * sample_rate as f64).round().max(0.0) as u64
}
