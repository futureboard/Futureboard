//! Forensic MIDI/plugin trace instrumentation for the audio engine.
//!
//! Enable with `FUTUREBOARD_FORENSIC_TRACE=1` or per-area flags
//! (`FUTUREBOARD_MIDI_ENGINE_DEBUG`, `FUTUREBOARD_MIDI_VERBOSE`, …).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::runtime::{midi_engine_debug_enabled, midi_verbose_enabled};
use crate::types::{EngineMidiClipSnapshot, EngineProjectSnapshot};
use crate::vst3_processor::vst3_midi_debug_enabled;

/// Master switch for the forensic hop chain.
pub fn forensic_trace_enabled() -> bool {
    static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FLAG.get_or_init(|| std::env::var_os("FUTUREBOARD_FORENSIC_TRACE").is_some())
}

pub fn engine_midi_trace_enabled() -> bool {
    forensic_trace_enabled() || midi_engine_debug_enabled()
}

pub fn engine_midi_verbose_enabled() -> bool {
    forensic_trace_enabled() || midi_verbose_enabled()
}

pub fn vst3_trace_enabled() -> bool {
    forensic_trace_enabled() || vst3_midi_debug_enabled()
}

static SCHEDULER_HEARTBEAT_MS: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Rate-limit scheduler heartbeat logs to ~1 Hz.
pub fn scheduler_heartbeat_due() -> bool {
    let now = now_ms();
    let prev = SCHEDULER_HEARTBEAT_MS.load(Ordering::Relaxed);
    if now.saturating_sub(prev) >= 1000 {
        SCHEDULER_HEARTBEAT_MS.store(now, Ordering::Relaxed);
        true
    } else {
        false
    }
}

/// Hop 2: engine snapshot MIDI detail.
pub fn log_engine_sync_midi(snapshot: &EngineProjectSnapshot) {
    if !engine_midi_trace_enabled() {
        return;
    }
    for track in &snapshot.tracks {
        let track_clips: Vec<_> = snapshot
            .midi_clips
            .iter()
            .filter(|c| c.track_id == track.id)
            .collect();
        if track_clips.is_empty() {
            continue;
        }
        eprintln!(
            "[engine-sync-midi] track={} clips={}",
            track.id,
            track_clips.len()
        );
        for clip in track_clips {
            eprintln!(
                "[engine-sync-midi] clip={} start_beats={:.3} length_beats={:.3} notes={}",
                clip.id,
                clip.start_beat,
                clip.length_beats,
                clip.notes.len()
            );
            for note in &clip.notes {
                eprintln!(
                    "[engine-sync-midi] note pitch={} start_beats={:.3} length_beats={:.3} velocity={}",
                    note.pitch,
                    note.start_beat,
                    note.length_beats,
                    note.velocity
                );
            }
        }
    }
}

/// Hop 3: runtime build MIDI detail (sample coordinates).
pub fn log_runtime_midi_clip(
    track_id: &str,
    clip: &EngineMidiClipSnapshot,
    samples_per_beat: f64,
    beat_to_sample: impl Fn(f64) -> u64,
) {
    if !engine_midi_trace_enabled() {
        return;
    }
    let start_samples = beat_to_sample(clip.start_beat);
    let end_samples = beat_to_sample(clip.start_beat + clip.length_beats);
    eprintln!(
        "[runtime-midi] clip={} start_samples={start_samples} end_samples={end_samples} notes={}",
        clip.id,
        clip.notes.len()
    );
    for note in &clip.notes {
        let abs_start = clip.start_beat + note.start_beat;
        let abs_end = abs_start + note.length_beats;
        let on_sample = beat_to_sample(abs_start);
        let off_sample = beat_to_sample(abs_end);
        eprintln!(
            "[runtime-midi] note pitch={} start_sample={on_sample} end_sample={off_sample} velocity={} track={track_id}",
            note.pitch,
            note.velocity
        );
        let _ = samples_per_beat; // used by caller context
    }
}

pub fn log_runtime_midi_track_summary(track_id: &str, runtime_clips: usize) {
    if !engine_midi_trace_enabled() {
        return;
    }
    eprintln!(
        "[runtime-midi] track={track_id} runtime_clips={runtime_clips}"
    );
}
