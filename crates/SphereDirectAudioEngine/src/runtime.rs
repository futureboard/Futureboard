//! Runtime playback graph sent to the CPAL callback.
//!
//! The control thread builds this from an `EngineProjectSnapshot`, including
//! decoding supported media files.  The audio thread then owns a local clone of
//! the graph and can render without touching locks or parsing JSON.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use crate::audio_file::{load_audio_file, AudioFileBuffer};
use serde_json::Value;
use sphere_audio_plugins::{canonical_plugin_id, should_rebuild_state, AudioPluginDspState};

use crate::types::{EngineClipSnapshot, EngineMidiClipSnapshot, EngineProjectSnapshot};
use crate::vst3_processor::Vst3RuntimeProcessor;

/// `FUTUREBOARD_MIDI_ENGINE_DEBUG=1` enables eprintln traces for MIDI runtime
/// build + per-block scheduling. Cached on first read so the audio callback
/// never touches the environment.
pub fn midi_engine_debug_enabled() -> bool {
    static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FLAG.get_or_init(|| std::env::var_os("FUTUREBOARD_MIDI_ENGINE_DEBUG").is_some())
}

#[derive(Debug, Clone)]
pub struct RuntimeTrack {
    pub id: String,
    pub track_type: String,
    pub volume: f32,
    pub pan: f32,
    pub muted: bool,
    pub solo: bool,
    pub preview_mode: RuntimePreviewMode,
    pub output_track_id: Option<String>,
    pub inserts: Vec<RuntimeInsert>,
    pub sends: Vec<RuntimeSend>,
    pub meter: Arc<RuntimeTrackMeter>,
    pub meter_peak_l: f32,
    pub meter_peak_r: f32,
    pub meter_sum_sq_l: f32,
    pub meter_sum_sq_r: f32,
    pub callback_insert_log_done: bool,
    pub callback_clip_route_log_done: bool,
    pub block_l: Vec<f32>,
    pub block_r: Vec<f32>,
    /// Send-receive accumulation buffers (Phase 3). Sends from other tracks
    /// sum into these; routing tracks (bus/return) then process this as their
    /// input. Preallocated alongside `block_*` so the audio callback never
    /// allocates. Zeroed at the top of each render block.
    pub recv_l: Vec<f32>,
    pub recv_r: Vec<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimePreviewMode {
    Stereo,
    Mono,
    Mid,
    Side,
}

impl RuntimePreviewMode {
    #[inline]
    pub fn from_str(value: &str) -> Self {
        match value {
            "mono" => Self::Mono,
            "mid" => Self::Mid,
            "side" => Self::Side,
            _ => Self::Stereo,
        }
    }

    #[inline]
    pub fn from_code(value: f32) -> Self {
        match value as i32 {
            1 => Self::Mono,
            2 => Self::Mid,
            3 => Self::Side,
            _ => Self::Stereo,
        }
    }
}

#[derive(Debug, Default)]
pub struct RuntimeTrackMeter {
    peak_l: AtomicU32,
    peak_r: AtomicU32,
    rms_l: AtomicU32,
    rms_r: AtomicU32,
}

#[derive(Debug, Clone)]
pub struct RuntimeTrackMeterSnapshot {
    pub track_id: String,
    pub peak_l: f32,
    pub peak_r: f32,
    pub rms_l: f32,
    pub rms_r: f32,
}

impl RuntimeTrackMeter {
    #[inline]
    fn store(&self, peak_l: f32, peak_r: f32, rms_l: f32, rms_r: f32) {
        self.peak_l.store(f32_store(peak_l), Ordering::Relaxed);
        self.peak_r.store(f32_store(peak_r), Ordering::Relaxed);
        self.rms_l.store(f32_store(rms_l), Ordering::Relaxed);
        self.rms_r.store(f32_store(rms_r), Ordering::Relaxed);
    }

    #[inline]
    fn load(&self, track_id: &str) -> RuntimeTrackMeterSnapshot {
        RuntimeTrackMeterSnapshot {
            track_id: track_id.to_string(),
            peak_l: f32_load(self.peak_l.load(Ordering::Relaxed)),
            peak_r: f32_load(self.peak_r.load(Ordering::Relaxed)),
            rms_l: f32_load(self.rms_l.load(Ordering::Relaxed)),
            rms_r: f32_load(self.rms_r.load(Ordering::Relaxed)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeInsert {
    pub id: String,
    pub kind: String,
    pub enabled: bool,
    pub params: HashMap<String, Value>,
    pub dsp: InsertDspState,
    pub vst3: Option<Vst3RuntimeProcessor>,
    pub callback_process_log_done: bool,
    pub silent_process_blocks: u32,
    pub scratch_l: Vec<f32>,
    pub scratch_r: Vec<f32>,
}

pub type InsertDspState = AudioPluginDspState;

const DEFAULT_AUDIO_BLOCK_CAPACITY: usize = 8192;

#[derive(Debug, Clone)]
pub struct RuntimeSend {
    pub id: String,
    pub return_track_id: String,
    pub level: f32,
    pub enabled: bool,
    /// Pre-fader tap (Phase 3). See [`EngineSendSnapshot::pre_fader`].
    pub pre_fader: bool,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeMidiEventKind {
    NoteOff,
    NoteOn,
}

#[derive(Debug, Clone)]
pub struct RuntimeMidiEvent {
    /// Absolute project sample at which the event fires (precomputed from the
    /// snapshot BPM at build time, mirroring how audio clips resolve to
    /// samples — keeps scheduling deterministic and lock-free in the callback).
    pub sample: u64,
    /// Absolute project beat (kept for debug logging only).
    pub beat: f64,
    pub kind: RuntimeMidiEventKind,
    pub pitch: u8,
    pub velocity: u8,
    pub channel: u8,
    pub note_id: u64,
}

/// Structural per-clip representation, retained for logging / future reuse.
#[derive(Debug, Clone)]
pub struct RuntimeMidiClip {
    pub id: String,
    pub track_id: String,
    pub start_beat: f64,
    pub end_beat: f64,
    pub events: Vec<RuntimeMidiEvent>,
}

/// Per-track merged + sorted event list with a playback cursor and active-note
/// set. Scheduling reads `events[cursor..]` each block; `cursor` is repositioned
/// on seek/play. `active` prevents stuck notes across stop/seek.
#[derive(Debug, Clone, Default)]
pub struct RuntimeMidiTrack {
    pub track_id: String,
    pub events: Vec<RuntimeMidiEvent>,
    pub cursor: usize,
    /// Currently-sounding (channel, pitch) pairs since the last NoteOn.
    pub active: Vec<(u8, u8)>,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeProject {
    pub sample_rate: u32,
    pub tracks: Vec<RuntimeTrack>,
    pub clips: Vec<RuntimeClip>,
    pub has_solo: bool,
    /// Samples per beat at the snapshot BPM (constant-tempo; tempo automation
    /// is a TODO — see midi-phase2-engine-playback.md).
    pub samples_per_beat: f64,
    /// Structural MIDI clips (logging / inspection).
    pub midi_clips: Vec<RuntimeMidiClip>,
    /// Per-track scheduling state driven by the audio callback.
    pub midi_tracks: Vec<RuntimeMidiTrack>,
}

impl RuntimeProject {
    /// Build a RuntimeProject from a snapshot.
    ///
    /// `existing_vst3` — if provided, VST3 processors from a previous runtime
    /// whose insert ID + plugin path + class_id + sample_rate still match are
    /// REUSED (taken out of the map) rather than recreated.  This keeps the
    /// same C++ processor alive across project reloads so editor windows stay
    /// valid.  Any entries left in the map after build were not matched and will
    /// be dropped by the caller (triggering `sphere_daux_vst3_destroy`).
    pub fn build(
        snapshot: &EngineProjectSnapshot,
        output_sample_rate: u32,
        decoded_by_path: &mut HashMap<String, Arc<AudioFileBuffer>>,
        mut existing_vst3: Option<&mut HashMap<String, Vst3RuntimeProcessor>>,
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
                        eprintln!(
                            "[SphereAudio] clip '{}' — decode FAILED '{path}': {e}",
                            clip.id
                        );
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

        // Use an explicit loop so we can mutably borrow existing_vst3 on each insert.
        let mut tracks: Vec<RuntimeTrack> = Vec::with_capacity(snapshot.tracks.len());
        for t in &snapshot.tracks {
            let mut inserts: Vec<RuntimeInsert> = Vec::with_capacity(t.inserts.len());
            for insert in &t.inserts {
                let is_native_vst3 = insert.kind.eq_ignore_ascii_case("native-plugin")
                    && insert
                        .params
                        .get("format")
                        .and_then(Value::as_str)
                        .map(|f| f.eq_ignore_ascii_case("VST3"))
                        .unwrap_or(false);

                let vst3 = if is_native_vst3 {
                    let new_path = insert
                        .params
                        .get("modulePath")
                        .or_else(|| insert.params.get("path"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    let new_class_id = insert
                        .params
                        .get("classId")
                        .or_else(|| insert.params.get("class_id"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim()
                        .to_string();

                    // Try to reuse an existing processor matching insert ID +
                    // plugin path + class_id + sample_rate.
                    let reused: Option<Vst3RuntimeProcessor> =
                        if let Some(ref mut map) = existing_vst3 {
                            let can_reuse = map
                                .get(&insert.id)
                                .map(|e| {
                                    e.plugin_path()
                                        .map(|p| p == new_path.as_str())
                                        .unwrap_or(false)
                                        && e.class_id()
                                            .map(|c| c == new_class_id.as_str())
                                            .unwrap_or(false)
                                        && e.sample_rate() == output_sample_rate
                                        && e.is_ready()
                                })
                                .unwrap_or(false);
                            if can_reuse {
                                map.remove(&insert.id)
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                    let reused_flag = reused.is_some();
                    let processor = reused.or_else(|| {
                        Vst3RuntimeProcessor::from_params(&insert.params, output_sample_rate)
                    });
                    let processor_handle =
                        processor.as_ref().map(|p| p.handle_value()).unwrap_or(0);
                    eprintln!(
                        "[SphereAudio] native VST3 insert track='{}' insert='{}' pluginInstanceId='{}' reused={} ready={} processorHandle=0x{:x} path='{}'",
                        t.id,
                        insert.id,
                        insert.params.get("pluginInstanceId").and_then(Value::as_str).unwrap_or(&insert.id),
                        reused_flag,
                        processor.as_ref().map(|p| p.is_ready()).unwrap_or(false),
                        processor_handle,
                        insert.params.get("path").and_then(Value::as_str).unwrap_or(""),
                    );
                    processor
                } else {
                    None
                };

                inserts.push(RuntimeInsert {
                    id: insert.id.clone(),
                    kind: insert.kind.clone(),
                    enabled: insert.enabled,
                    params: insert.params.clone(),
                    dsp: InsertDspState::new(
                        canonical_plugin_id(&insert.kind),
                        &insert.params,
                        output_sample_rate,
                    ),
                    vst3,
                    callback_process_log_done: false,
                    silent_process_blocks: 0,
                    scratch_l: vec![0.0; DEFAULT_AUDIO_BLOCK_CAPACITY],
                    scratch_r: vec![0.0; DEFAULT_AUDIO_BLOCK_CAPACITY],
                });
            }

            tracks.push(RuntimeTrack {
                id: t.id.clone(),
                track_type: t.track_type.clone(),
                volume: t.volume.clamp(0.0, 2.0),
                pan: t.pan.clamp(-1.0, 1.0),
                muted: t.muted,
                solo: t.solo,
                preview_mode: RuntimePreviewMode::from_str(&t.preview_mode),
                output_track_id: t.output_track_id.clone(),
                inserts,
                sends: t
                    .sends
                    .iter()
                    .map(|send| RuntimeSend {
                        id: send.id.clone(),
                        return_track_id: send.return_track_id.clone(),
                        level: send.level.clamp(0.0, 2.0),
                        enabled: send.enabled,
                        pre_fader: send.pre_fader,
                    })
                    .collect(),
                meter: Arc::new(RuntimeTrackMeter::default()),
                meter_peak_l: 0.0,
                meter_peak_r: 0.0,
                meter_sum_sq_l: 0.0,
                meter_sum_sq_r: 0.0,
                callback_insert_log_done: false,
                callback_clip_route_log_done: false,
                block_l: vec![0.0; DEFAULT_AUDIO_BLOCK_CAPACITY],
                block_r: vec![0.0; DEFAULT_AUDIO_BLOCK_CAPACITY],
                recv_l: vec![0.0; DEFAULT_AUDIO_BLOCK_CAPACITY],
                recv_r: vec![0.0; DEFAULT_AUDIO_BLOCK_CAPACITY],
            });
        }
        let has_solo = tracks.iter().any(|t| t.solo);
        let master_insert_count = tracks
            .iter()
            .find(|track| track.track_type == "master")
            .map(|track| track.inserts.len())
            .unwrap_or(0);
        eprintln!("[SphereAudio] RuntimeMaster inserts={master_insert_count}");
        for track in &tracks {
            let track_clips = clips
                .iter()
                .filter(|clip| clip.track_id == track.id)
                .count();
            eprintln!(
                "[SphereAudio] RuntimeTrack track={} clips={} inserts={}",
                track.id,
                track_clips,
                track.inserts.len()
            );
            if !track.inserts.is_empty() {
                for insert in &track.inserts {
                    let format = insert
                        .params
                        .get("format")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let path = insert
                        .params
                        .get("modulePath")
                        .or_else(|| insert.params.get("path"))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let class_id = insert
                        .params
                        .get("classId")
                        .or_else(|| insert.params.get("class_id"))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    eprintln!(
                        "[SphereAudio] RuntimeInsert id={} format={} path={} classId={} bypass={}",
                        insert.id, format, path, class_id, !insert.enabled
                    );
                }
            }
        }

        // Phase 3 routing graph trace. Logged here on the build (worker)
        // thread — never in the audio callback. Reports node kinds, each
        // track's sends, and any sends that will be rejected at render time
        // (cycle-safe rule: source→routing only, routing→later-routing only).
        if std::env::var_os("FUTUREBOARD_ROUTING_DEBUG").is_some() {
            let is_routing = |ty: &str| ty == "bus" || ty == "return";
            eprintln!("[routing] graph nodes={}", tracks.len());
            for (idx, track) in tracks.iter().enumerate() {
                eprintln!(
                    "[routing] node[{idx}] track={} type={} sends={}",
                    track.id,
                    track.track_type,
                    track.sends.len()
                );
                for send in &track.sends {
                    let target_idx = tracks.iter().position(|t| t.id == send.return_track_id);
                    let target_routing = target_idx
                        .map(|t| is_routing(&tracks[t].track_type))
                        .unwrap_or(false);
                    let source_routing = is_routing(&track.track_type);
                    // Accepted when: target is a routing track, AND if the
                    // source is itself routing the target must come later in
                    // the array (forward-only) to stay acyclic.
                    let accepted = target_routing
                        && match (source_routing, target_idx) {
                            (true, Some(t)) => t > idx,
                            (false, Some(_)) => true,
                            _ => false,
                        };
                    eprintln!(
                        "[routing]   send id={} -> {} target_idx={:?} level={:.3} enabled={} {}",
                        send.id,
                        send.return_track_id,
                        target_idx,
                        send.level,
                        send.enabled,
                        if accepted { "ACCEPT" } else { "REJECT(cycle-unsafe)" }
                    );
                }
            }
        }

        // ── MIDI runtime build (Phase 2) ────────────────────────────────────
        let samples_per_beat = if snapshot.bpm > 0.0 {
            output_sample_rate as f64 * 60.0 / snapshot.bpm
        } else {
            0.0
        };
        let (midi_clips, midi_tracks) =
            build_midi_runtime(&snapshot.midi_clips, samples_per_beat);

        if midi_engine_debug_enabled() {
            let total_events: usize = midi_clips.iter().map(|c| c.events.len()).sum();
            for c in &midi_clips {
                eprintln!(
                    "[DAUx MIDI] RuntimeMidiClip id={} track={} notes={} events={} beats={:.3}..{:.3}",
                    c.id,
                    c.track_id,
                    c.events.len() / 2,
                    c.events.len(),
                    c.start_beat,
                    c.end_beat
                );
            }
            eprintln!(
                "[DAUx MIDI] RuntimeProject midi_clips={} midi_events={} midi_tracks={} samples_per_beat={:.2}",
                midi_clips.len(),
                total_events,
                midi_tracks.len(),
                samples_per_beat
            );
        }

        Self {
            sample_rate: output_sample_rate,
            tracks,
            clips,
            has_solo,
            samples_per_beat,
            midi_clips,
            midi_tracks,
        }
    }

    /// Reposition every MIDI track's cursor to the first event at/after
    /// `position_sample` and clear active notes (emitting note-offs so the
    /// destination never gets a stuck note). Called on seek / play-from.
    pub fn reset_midi_playback(&mut self, position_sample: u64) {
        self.all_notes_off("seek/play");
        for mt in &mut self.midi_tracks {
            // Binary search: first event with sample >= position.
            mt.cursor = mt
                .events
                .partition_point(|ev| ev.sample < position_sample);
        }
        if midi_engine_debug_enabled() {
            eprintln!(
                "[DAUx MIDI] reset_midi_playback pos={}sa tracks={}",
                position_sample,
                self.midi_tracks.len()
            );
        }
    }

    /// Emit note-off for all active notes on every MIDI track and clear the
    /// active set. Called on stop/seek to prevent stuck notes.
    pub fn all_notes_off(&mut self, reason: &str) {
        let debug = midi_engine_debug_enabled();
        for mt in &mut self.midi_tracks {
            if mt.active.is_empty() {
                continue;
            }
            if debug {
                eprintln!(
                    "[DAUx MIDI] all notes off track={} active={} reason={}",
                    mt.track_id,
                    mt.active.len(),
                    reason
                );
            }
            // TODO (Phase 2B): forward note-off to the track's instrument insert
            // (VST3 event input) before clearing.
            mt.active.clear();
        }
    }

    /// Schedule the MIDI events that fall inside `[base_sample, base_sample +
    /// frames)`. Runs once per audio block from the callback. No heap
    /// allocation on the steady-state path (event lists are preallocated; the
    /// active-note Vec is reserved at build time).
    pub fn schedule_midi_block(&mut self, base_sample: u64, frames: u64) {
        if self.midi_tracks.is_empty() || frames == 0 {
            return;
        }
        let block_end = base_sample.saturating_add(frames);
        let debug = midi_engine_debug_enabled();
        let spb = self.samples_per_beat.max(1.0);
        for mt in &mut self.midi_tracks {
            let mut scheduled = 0u32;
            while mt.cursor < mt.events.len() && mt.events[mt.cursor].sample < block_end {
                let ev = mt.events[mt.cursor].clone();
                mt.cursor += 1;
                if ev.sample < base_sample {
                    // Stale (e.g. just after a seek landing mid-block) — still
                    // apply active-note bookkeeping so state stays consistent.
                    apply_active(&mut mt.active, &ev);
                    continue;
                }
                let offset = (ev.sample - base_sample) as usize;
                apply_active(&mut mt.active, &ev);
                // TODO (Phase 2B): route `ev` to the track's instrument insert
                // (VST3 IEventList) at `offset` samples into the block.
                if debug {
                    match ev.kind {
                        RuntimeMidiEventKind::NoteOn => eprintln!(
                            "[DAUx MIDI] note_on ch={} pitch={} vel={} offset={}",
                            ev.channel, ev.pitch, ev.velocity, offset
                        ),
                        RuntimeMidiEventKind::NoteOff => eprintln!(
                            "[DAUx MIDI] note_off ch={} pitch={} offset={}",
                            ev.channel, ev.pitch, offset
                        ),
                    }
                }
                scheduled += 1;
            }
            if debug && scheduled > 0 {
                let bs = base_sample as f64 / spb;
                let be = block_end as f64 / spb;
                eprintln!(
                    "[DAUx MIDI] block beat={:.3}..{:.3} track={} events={} active={}",
                    bs, be, mt.track_id, scheduled, mt.active.len()
                );
            }
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
    pub fn begin_meter_block(&mut self) {
        for track in &mut self.tracks {
            track.meter_peak_l = 0.0;
            track.meter_peak_r = 0.0;
            track.meter_sum_sq_l = 0.0;
            track.meter_sum_sq_r = 0.0;
        }
    }

    #[inline]
    pub fn accumulate_track_meter(&mut self, track_index: usize, l: f32, r: f32) {
        let Some(track) = self.tracks.get_mut(track_index) else {
            return;
        };
        let abs_l = l.abs();
        let abs_r = r.abs();
        track.meter_peak_l = track.meter_peak_l.max(abs_l);
        track.meter_peak_r = track.meter_peak_r.max(abs_r);
        track.meter_sum_sq_l += l * l;
        track.meter_sum_sq_r += r * r;
    }

    #[inline]
    pub fn end_meter_block(&mut self, frames: u64) {
        let frame_count = frames.max(1) as f32;
        for track in &mut self.tracks {
            let rms_l = (track.meter_sum_sq_l / frame_count).sqrt();
            let rms_r = (track.meter_sum_sq_r / frame_count).sqrt();
            track
                .meter
                .store(track.meter_peak_l, track.meter_peak_r, rms_l, rms_r);
        }
    }

    pub fn meter_snapshots(&self) -> Vec<RuntimeTrackMeterSnapshot> {
        self.tracks
            .iter()
            .map(|track| track.meter.load(&track.id))
            .collect()
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

    #[inline]
    pub fn update_track_preview_mode(&mut self, track_id: &str, mode: RuntimePreviewMode) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            track.preview_mode = mode;
        }
    }

    #[inline]
    pub fn update_insert_param(
        &mut self,
        track_id: &str,
        insert_id: &str,
        param_id: &str,
        value: f32,
    ) {
        let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) else {
            return;
        };
        let Some(insert) = track.inserts.iter_mut().find(|i| i.id == insert_id) else {
            return;
        };

        // "enabled" toggles bypass for all insert types.
        if param_id == "enabled" {
            insert.enabled = value >= 0.5;
            return;
        }

        // For native VST3 inserts: forward numeric param IDs to the C++ processor.
        // The web UI sends VST3 ParamIDs as decimal strings ("12345"), and values
        // are normalized (0..1) as required by IParameterChanges.
        if let Some(vst3) = insert.vst3.as_mut() {
            if let Ok(vst3_param_id) = param_id.parse::<u32>() {
                vst3.set_param(vst3_param_id, value as f64);
                insert.callback_process_log_done = false;
                insert.silent_process_blocks = 0;
                // Also persist in params map for snapshot/recall, then return —
                // built-in DSP state rebuild is not applicable to VST3 inserts.
                insert
                    .params
                    .insert(param_id.to_string(), Value::from(value as f64));
                return;
            }
        }

        // Built-in plugin insert: update params map and rebuild DSP state if needed.
        insert
            .params
            .insert(param_id.to_string(), Value::from(value as f64));
        let plugin_id = canonical_plugin_id(&insert.kind);
        if should_rebuild_state(plugin_id, param_id) {
            insert
                .dsp
                .rebuild(plugin_id, &insert.params, self.sample_rate);
        }
    }
}

/// Apply a note event to the active-note set (NoteOn inserts, NoteOff removes).
#[inline]
fn apply_active(active: &mut Vec<(u8, u8)>, ev: &RuntimeMidiEvent) {
    let key = (ev.channel, ev.pitch);
    match ev.kind {
        RuntimeMidiEventKind::NoteOn => {
            if !active.contains(&key) {
                active.push(key);
            }
        }
        RuntimeMidiEventKind::NoteOff => {
            active.retain(|k| *k != key);
        }
    }
}

/// Convert snapshot MIDI clips into structural [`RuntimeMidiClip`]s and merged
/// per-track [`RuntimeMidiTrack`] schedules. Note starts are clip-relative and
/// converted to absolute project beats/samples here (outside the audio
/// callback). Events are sorted by sample, with NoteOff before NoteOn at the
/// same sample to avoid retrigger glitches / stuck notes.
fn build_midi_runtime(
    snapshot_clips: &[EngineMidiClipSnapshot],
    samples_per_beat: f64,
) -> (Vec<RuntimeMidiClip>, Vec<RuntimeMidiTrack>) {
    let mut clips: Vec<RuntimeMidiClip> = Vec::with_capacity(snapshot_clips.len());
    let mut by_track: HashMap<String, Vec<RuntimeMidiEvent>> = HashMap::new();

    for clip in snapshot_clips {
        let mut events: Vec<RuntimeMidiEvent> = Vec::with_capacity(clip.notes.len() * 2);
        for note in &clip.notes {
            if note.length_beats <= 0.0 {
                continue; // skip zero/negative-length notes
            }
            let pitch = note.pitch.min(127);
            let velocity = note.velocity.clamp(1, 127);
            let channel = note.channel.min(15);
            let abs_start = clip.start_beat + note.start_beat.max(0.0);
            let abs_end = abs_start + note.length_beats;
            let on_sample = (abs_start * samples_per_beat).round().max(0.0) as u64;
            let off_sample = (abs_end * samples_per_beat).round().max(0.0) as u64;
            events.push(RuntimeMidiEvent {
                sample: on_sample,
                beat: abs_start,
                kind: RuntimeMidiEventKind::NoteOn,
                pitch,
                velocity,
                channel,
                note_id: note.id,
            });
            events.push(RuntimeMidiEvent {
                sample: off_sample,
                beat: abs_end,
                kind: RuntimeMidiEventKind::NoteOff,
                pitch,
                velocity: 0,
                channel,
                note_id: note.id,
            });
        }
        // Sort by sample; NoteOff before NoteOn at the same sample.
        events.sort_by(|a, b| {
            a.sample
                .cmp(&b.sample)
                .then((a.kind as u8).cmp(&(b.kind as u8)))
        });
        let end_beat = clip.start_beat + clip.length_beats.max(0.0);
        by_track
            .entry(clip.track_id.clone())
            .or_default()
            .extend(events.iter().cloned());
        clips.push(RuntimeMidiClip {
            id: clip.id.clone(),
            track_id: clip.track_id.clone(),
            start_beat: clip.start_beat,
            end_beat,
            events,
        });
    }

    let mut midi_tracks: Vec<RuntimeMidiTrack> = by_track
        .into_iter()
        .map(|(track_id, mut events)| {
            events.sort_by(|a, b| {
                a.sample
                    .cmp(&b.sample)
                    .then((a.kind as u8).cmp(&(b.kind as u8)))
            });
            let mut active = Vec::new();
            active.reserve(128); // bound growth out of the audio callback
            RuntimeMidiTrack {
                track_id,
                events,
                cursor: 0,
                active,
            }
        })
        .collect();
    midi_tracks.sort_by(|a, b| a.track_id.cmp(&b.track_id));

    (clips, midi_tracks)
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

#[inline]
fn f32_store(v: f32) -> u32 {
    v.to_bits()
}

#[inline]
fn f32_load(v: u32) -> f32 {
    f32::from_bits(v)
}

#[cfg(test)]
mod midi_tests {
    use super::*;
    use crate::types::{EngineMidiClipSnapshot, EngineMidiNoteSnapshot};

    // 120 BPM @ 48 kHz → 24000 samples per beat.
    const SPB: f64 = 24000.0;

    fn clip_with_one_note() -> EngineMidiClipSnapshot {
        EngineMidiClipSnapshot {
            id: "mc1".into(),
            track_id: "track-1".into(),
            start_beat: 4.0, // bar 2 in 4/4
            length_beats: 4.0,
            notes: vec![EngineMidiNoteSnapshot {
                id: 1,
                pitch: 60, // C4
                start_beat: 0.0,
                length_beats: 1.0,
                velocity: 100,
                channel: 0,
            }],
        }
    }

    fn project_with(clips: Vec<EngineMidiClipSnapshot>) -> RuntimeProject {
        let (midi_clips, midi_tracks) = build_midi_runtime(&clips, SPB);
        RuntimeProject {
            sample_rate: 48_000,
            samples_per_beat: SPB,
            midi_clips,
            midi_tracks,
            ..Default::default()
        }
    }

    #[test]
    fn note_resolves_to_absolute_samples_with_off_before_on() {
        let p = project_with(vec![clip_with_one_note()]);
        let evs = &p.midi_tracks[0].events;
        assert_eq!(evs.len(), 2);
        // absolute start beat = 4 + 0 = 4 → 96000 sa; end beat 5 → 120000 sa.
        let on = evs.iter().find(|e| e.kind == RuntimeMidiEventKind::NoteOn).unwrap();
        let off = evs.iter().find(|e| e.kind == RuntimeMidiEventKind::NoteOff).unwrap();
        assert_eq!(on.sample, 96_000);
        assert_eq!(off.sample, 120_000);
        assert_eq!(on.pitch, 60);
        assert_eq!(on.velocity, 100);
    }

    #[test]
    fn zero_length_note_is_skipped() {
        let mut clip = clip_with_one_note();
        clip.notes[0].length_beats = 0.0;
        let p = project_with(vec![clip]);
        assert!(p.midi_tracks.is_empty() || p.midi_tracks[0].events.is_empty());
    }

    #[test]
    fn schedule_fires_note_on_then_off_and_tracks_active() {
        let mut p = project_with(vec![clip_with_one_note()]);
        p.reset_midi_playback(0);
        // Block before the note: nothing active.
        p.schedule_midi_block(0, 512);
        assert_eq!(p.midi_tracks[0].active.len(), 0);
        // Block covering the NoteOn (96000).
        p.schedule_midi_block(96_000, 512);
        assert_eq!(p.midi_tracks[0].active, vec![(0u8, 60u8)]);
        // Block covering the NoteOff (120000).
        p.schedule_midi_block(120_000, 512);
        assert!(p.midi_tracks[0].active.is_empty());
    }

    #[test]
    fn seek_before_note_then_play_fires_it() {
        let mut p = project_with(vec![clip_with_one_note()]);
        p.reset_midi_playback(95_000); // just before the NoteOn
        p.schedule_midi_block(95_000, 2048); // covers 95000..97048 → fires NoteOn
        assert_eq!(p.midi_tracks[0].active, vec![(0u8, 60u8)]);
    }

    #[test]
    fn seek_after_note_does_not_fire_old_note() {
        let mut p = project_with(vec![clip_with_one_note()]);
        p.reset_midi_playback(200_000); // well past the note
        p.schedule_midi_block(200_000, 512);
        assert!(p.midi_tracks[0].active.is_empty());
        assert_eq!(p.midi_tracks[0].cursor, p.midi_tracks[0].events.len());
    }

    #[test]
    fn all_notes_off_clears_active() {
        let mut p = project_with(vec![clip_with_one_note()]);
        p.reset_midi_playback(96_000);
        p.schedule_midi_block(96_000, 512);
        assert_eq!(p.midi_tracks[0].active.len(), 1);
        p.all_notes_off("stop");
        assert!(p.midi_tracks[0].active.is_empty());
    }
}
