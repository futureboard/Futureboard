//! Runtime playback graph sent to the CPAL callback.
//!
//! The control thread builds this from an `EngineProjectSnapshot`, including
//! decoding supported media files.  The audio thread then owns a local clone of
//! the graph and can render without touching locks or parsing JSON.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use crate::audio_graph::{plan_runtime_audio_graph, GraphValidationError, RuntimeAudioGraph};
use crate::audio_source::{open_clip_audio_source, ClipAudioSource};
use crate::latency_graph::{plan_runtime_latency_graph, RuntimeLatencyGraph};
use serde_json::Value;
use sphere_audio_plugins::{canonical_plugin_id, should_rebuild_state, AudioPluginDspState};

use crate::types::{
    EngineAutomationLaneSnapshot, EngineClipSnapshot, EngineMidiClipSnapshot, EngineProjectSnapshot,
};
use crate::vst3_processor::{vst3_midi_debug_enabled, Vst3MidiEvent, Vst3RuntimeProcessor};

/// `FUTUREBOARD_MIDI_ENGINE_DEBUG=1` enables eprintln traces for MIDI runtime
/// build + per-block scheduling. Cached on first read so the audio callback
/// never touches the environment.
pub fn midi_engine_debug_enabled() -> bool {
    static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FLAG.get_or_init(|| {
        std::env::var_os("FUTUREBOARD_FORENSIC_TRACE").is_some()
            || std::env::var_os("FUTUREBOARD_MIDI_ENGINE_DEBUG").is_some()
    })
}

/// Verbose MIDI/bridge tracing (off by default — safe for realtime audio).
pub fn midi_verbose_enabled() -> bool {
    static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FLAG.get_or_init(|| {
        std::env::var_os("FUTUREBOARD_FORENSIC_TRACE").is_some()
            || std::env::var_os("FUTUREBOARD_MIDI_VERBOSE").is_some()
    })
}

#[derive(Debug, Clone)]
pub struct RuntimeTrack {
    pub id: String,
    pub track_type: String,
    pub volume: f32,
    pub pan: f32,
    pub muted: bool,
    pub solo: bool,
    pub record_armed: bool,
    pub monitor_enabled: bool,
    pub input_source: RuntimeTrackInputSource,
    pub preview_mode: RuntimePreviewMode,
    pub output_track_id: Option<String>,
    pub inserts: Vec<RuntimeInsert>,
    pub sends: Vec<RuntimeSend>,
    pub automation_lanes: Vec<RuntimeAutomationLane>,
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
    /// Per-block MIDI events for the instrument VST3 insert (Phase 2B).
    /// Cleared at the start of `schedule_midi_block`; no steady-path allocation.
    pub midi_block_events: Vec<Vst3MidiEvent>,
    /// Index into `inserts` of the first instrument-capable native VST3 insert.
    pub midi_instrument_insert_ix: Option<usize>,
    /// Sum of enabled insert latencies at build time (Phase V/W reporting).
    pub plugin_latency_samples: u32,
    /// Ring buffers for PDC on post-fader output (preallocated at build).
    pub pdc_delay_l: Vec<f32>,
    pub pdc_delay_r: Vec<f32>,
    pub pdc_write_pos: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeTrackInputSource {
    None,
    Mono { channel: usize },
    Stereo { left: usize, right: usize },
}

impl RuntimeTrackInputSource {
    fn from_channels(channels: &[u32]) -> Self {
        match channels {
            [] => Self::None,
            [channel] => Self::Mono {
                channel: *channel as usize,
            },
            [left, right, ..] => Self::Stereo {
                left: *left as usize,
                right: *right as usize,
            },
        }
    }

    #[inline]
    pub fn is_routable(&self) -> bool {
        !matches!(self, Self::None)
    }

    #[inline]
    pub fn sample_from_latest(&self, latest_l: f32, latest_r: f32) -> (f32, f32) {
        match self {
            Self::None => (0.0, 0.0),
            Self::Mono { channel } => {
                let mono = if *channel == 0 { latest_l } else { latest_r };
                (mono, mono)
            }
            Self::Stereo { left, right } => {
                let pick = |channel: usize| if channel == 0 { latest_l } else { latest_r };
                (pick(*left), pick(*right))
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeAutomationCurve {
    Linear,
    Hold,
    Smooth,
}

impl RuntimeAutomationCurve {
    #[inline]
    fn from_tag(tag: u8) -> Self {
        match tag {
            1 => Self::Hold,
            2 => Self::Smooth,
            _ => Self::Linear,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeAutomationTarget {
    TrackVolume,
    TrackPan,
    TrackMute,
    PluginParameter {
        insert_id: String,
        parameter_id: String,
    },
    SendGain {
        send_id: String,
    },
    Unresolved,
}

impl RuntimeAutomationTarget {
    fn from_snapshot(lane: &EngineAutomationLaneSnapshot) -> Self {
        match lane.target.tag {
            0 => Self::TrackVolume,
            1 => Self::TrackPan,
            2 => Self::TrackMute,
            3 if !lane.target.insert_id.is_empty() && !lane.target.parameter_id.is_empty() => {
                Self::PluginParameter {
                    insert_id: lane.target.insert_id.clone(),
                    parameter_id: lane.target.parameter_id.clone(),
                }
            }
            4 if !lane.target.send_id.is_empty() => Self::SendGain {
                send_id: lane.target.send_id.clone(),
            },
            _ => Self::Unresolved,
        }
    }

    #[inline]
    pub fn default_value(&self) -> f32 {
        match self {
            Self::TrackVolume => volume_db_to_norm(0.0),
            Self::TrackPan => 0.5,
            Self::TrackMute => 0.0,
            Self::PluginParameter { .. } => 0.5,
            Self::SendGain { .. } => 0.0,
            Self::Unresolved => 0.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeAutomationPoint {
    pub beat: f64,
    pub value: f32,
    pub curve: RuntimeAutomationCurve,
}

#[derive(Debug, Clone)]
pub struct RuntimeAutomationLane {
    pub id: String,
    pub name: String,
    pub target: RuntimeAutomationTarget,
    pub enabled: bool,
    pub points: Vec<RuntimeAutomationPoint>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct RuntimeTrackAutomationValues {
    pub volume: Option<f32>,
    pub pan: Option<f32>,
    pub muted: Option<bool>,
}

impl RuntimeAutomationLane {
    fn from_snapshot(lane: &EngineAutomationLaneSnapshot) -> Self {
        let mut points: Vec<RuntimeAutomationPoint> = lane
            .points
            .iter()
            .map(|point| RuntimeAutomationPoint {
                beat: point.beat.max(0.0),
                value: point.value.clamp(0.0, 1.0),
                curve: RuntimeAutomationCurve::from_tag(point.curve),
            })
            .collect();
        points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
        Self {
            id: lane.id.clone(),
            name: lane.name.clone(),
            target: RuntimeAutomationTarget::from_snapshot(lane),
            enabled: lane.enabled,
            points,
        }
    }

    #[inline]
    pub fn evaluate_normalized(&self, beat: f64) -> Option<f32> {
        if !self.enabled || matches!(self.target, RuntimeAutomationTarget::Unresolved) {
            return None;
        }
        Some(evaluate_automation_points(
            &self.points,
            beat,
            self.target.default_value(),
        ))
    }
}

impl RuntimeTrack {
    #[inline]
    pub fn automation_values_at_beat(&self, beat: f64) -> RuntimeTrackAutomationValues {
        let mut values = RuntimeTrackAutomationValues::default();
        for lane in &self.automation_lanes {
            let Some(value) = lane.evaluate_normalized(beat) else {
                continue;
            };
            match lane.target {
                RuntimeAutomationTarget::TrackVolume => {
                    values.volume = Some(volume_norm_to_linear(value));
                }
                RuntimeAutomationTarget::TrackPan => {
                    values.pan = Some((value * 2.0 - 1.0).clamp(-1.0, 1.0));
                }
                RuntimeAutomationTarget::TrackMute => {
                    values.muted = Some(value >= 0.5);
                }
                _ => {}
            }
        }
        values
    }
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
    pub(crate) fn load(&self, track_id: &str) -> RuntimeTrackMeterSnapshot {
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
    /// Clip-level mute — a muted clip is skipped entirely during render.
    pub muted: bool,
    /// Linear fade lengths in output samples, resolved from the snapshot's
    /// fade durations at build time. `0` means no fade. Clamped so
    /// `fade_in + fade_out <= duration_samples`. Curve shaping beyond linear is
    /// a placeholder (see `clip_fade_gain`).
    pub fade_in_samples: u64,
    pub fade_out_samples: u64,
    pub source: Arc<ClipAudioSource>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeMidiEventKind {
    NoteOff,
    NoteOn,
    /// MIDI controller change (CC / pitch-bend / aftertouch). Uses
    /// `cc_number` / `cc_value` rather than `pitch` / `velocity`.
    ControlChange,
}

#[derive(Debug, Clone)]
pub struct RuntimeMidiEvent {
    /// Absolute project sample at which the event fires (precomputed from the
    /// snapshot BPM at build time, mirroring how audio clips resolve to
    /// samples — keeps scheduling deterministic and lock-free in the callback).
    pub sample: u64,
    /// Absolute project beat. This is the canonical musical position; `sample`
    /// is rebuilt from it when the project tempo changes.
    pub beat: f64,
    pub kind: RuntimeMidiEventKind,
    pub pitch: u8,
    pub velocity: u8,
    pub channel: u8,
    pub note_id: u64,
    /// For `ControlChange`: VST3 controller number (`0..=127` CC, `128`
    /// aftertouch, `129` pitch bend). Unused for note events.
    pub cc_number: u16,
    /// For `ControlChange`: normalized value `0.0..=1.0`. Unused for notes.
    pub cc_value: f32,
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
    /// UI preview notes currently held independently of transport playback.
    pub preview_active: Vec<(u8, u8)>,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeProject {
    pub sample_rate: u32,
    pub tracks: Vec<RuntimeTrack>,
    pub clips: Vec<RuntimeClip>,
    pub has_solo: bool,
    /// Samples per beat at the current static BPM. MIDI event sample positions
    /// are derived from event beats whenever tempo changes.
    pub samples_per_beat: f64,
    /// Structural MIDI clips (logging / inspection).
    pub midi_clips: Vec<RuntimeMidiClip>,
    /// Per-track scheduling state driven by the audio callback.
    pub midi_tracks: Vec<RuntimeMidiTrack>,
    /// Precomputed pass order and routing validation (Phase O).
    pub audio_graph: RuntimeAudioGraph,
    /// Latency propagation and PDC delays (Phase V/W).
    pub latency_graph: RuntimeLatencyGraph,
    /// Stage 3b: realtime sinks for external plugin-host DSP output, keyed by
    /// `track_id`. Set via [`crate::command::EngineCommand::SetPluginBridgeSink`]
    /// and preserved across project reloads. Empty until the bridge installs one.
    pub plugin_bridge_sinks:
        std::collections::HashMap<String, std::sync::Arc<dyn crate::plugin_bridge::PluginBridgeSink>>,
    /// Tracks whose external-bridge plugin editor is open — keeps the audio
    /// callback rendering while stopped so VSTi internal keyboards stay audible.
    pub bridge_editor_active: std::collections::HashSet<String>,
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
        decoded_by_path: &mut HashMap<String, Arc<ClipAudioSource>>,
        mut existing_vst3: Option<&mut HashMap<String, Vst3RuntimeProcessor>>,
        pdc_enabled: bool,
    ) -> Result<Self, GraphValidationError> {
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
                        clip.id,
                        existing.frames()
                    );
                    loaded_from_cache += 1;
                    Arc::clone(existing)
                }
                None => match open_clip_audio_source(path) {
                    Ok(source) => {
                        eprintln!(
                            "[SphereAudio] clip '{}' — opened: '{path}' {} frames @ {}Hz {} ch ({})",
                            clip.id,
                            source.frames(),
                            source.sample_rate(),
                            source.channels(),
                            if source.is_streaming() {
                                "stream"
                            } else if source.is_mapped() {
                                "mmap"
                            } else {
                                "memory"
                            }
                        );
                        loaded_fresh += 1;
                        let source = Arc::new(source);
                        decoded_by_path.insert(path.to_string(), Arc::clone(&source));
                        source
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

            let midi_instrument_insert_ix = find_midi_instrument_insert_ix(&inserts, &t.track_type);

            tracks.push(RuntimeTrack {
                id: t.id.clone(),
                track_type: t.track_type.clone(),
                volume: t.volume.clamp(0.0, 2.0),
                pan: t.pan.clamp(-1.0, 1.0),
                muted: t.muted,
                solo: t.solo,
                record_armed: t.armed,
                monitor_enabled: t.input_monitor,
                input_source: RuntimeTrackInputSource::from_channels(&t.input_source.channels),
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
                automation_lanes: t
                    .automation_lanes
                    .iter()
                    .map(RuntimeAutomationLane::from_snapshot)
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
                midi_block_events: Vec::with_capacity(256),
                midi_instrument_insert_ix,
                plugin_latency_samples: 0,
                pdc_delay_l: Vec::new(),
                pdc_delay_r: Vec::new(),
                pdc_write_pos: 0,
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
                        if accepted {
                            "ACCEPT"
                        } else {
                            "REJECT(cycle-unsafe)"
                        }
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
        let (midi_clips, midi_tracks) = build_midi_runtime(&snapshot.midi_clips, samples_per_beat);

        if crate::forensic_trace::engine_midi_trace_enabled() {
            for track in &snapshot.tracks {
                let track_clip_count = snapshot
                    .midi_clips
                    .iter()
                    .filter(|c| c.track_id == track.id)
                    .count();
                crate::forensic_trace::log_runtime_midi_track_summary(&track.id, track_clip_count);
            }
            for clip in &snapshot.midi_clips {
                crate::forensic_trace::log_runtime_midi_clip(
                    &clip.track_id,
                    clip,
                    samples_per_beat,
                    |beat| beat_to_sample(beat, samples_per_beat),
                );
            }
        }

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

        let audio_graph = match plan_runtime_audio_graph(&tracks) {
            Ok(graph) => graph,
            Err(err) => {
                if let Some(map) = existing_vst3 {
                    for track in &mut tracks {
                        for insert in &mut track.inserts {
                            if let Some(vst3) = insert.vst3.take() {
                                map.insert(insert.id.clone(), vst3);
                            }
                        }
                    }
                }
                return Err(err);
            }
        };

        if std::env::var_os("FUTUREBOARD_ROUTING_DEBUG").is_some() {
            eprintln!(
                "[routing] graph nodes={} pass1={} pass2={} rejected={}",
                audio_graph.nodes.len(),
                audio_graph.pass1_source_indices.len(),
                audio_graph.pass2_routing_indices.len(),
                audio_graph.rejected_routes.len(),
            );
        }

        for (idx, track) in tracks.iter_mut().enumerate() {
            track.plugin_latency_samples =
                crate::latency_graph::strip_plugin_latency_samples(track);
            let _ = idx;
        }

        let pdc_active = pdc_enabled
            && !std::env::var_os("FUTUREBOARD_PDC").is_some_and(|v| v == "0" || v == "false");
        let latency_graph = plan_runtime_latency_graph(&tracks, &audio_graph, pdc_active);

        let pdc_buffer_frames =
            latency_graph.max_path_latency_samples.max(1) as usize + DEFAULT_AUDIO_BLOCK_CAPACITY;
        for (idx, track) in tracks.iter_mut().enumerate() {
            track.pdc_delay_l.resize(pdc_buffer_frames, 0.0);
            track.pdc_delay_r.resize(pdc_buffer_frames, 0.0);
            track.pdc_write_pos = 0;
            let _ = idx;
        }

        if std::env::var_os("FUTUREBOARD_ROUTING_DEBUG").is_some() {
            eprintln!(
                "[latency] max_path={} master_plugin={} pdc_enabled={}",
                latency_graph.max_path_latency_samples,
                latency_graph.master_plugin_latency,
                pdc_active
            );
            for (idx, track) in tracks.iter().enumerate() {
                eprintln!(
                    "[latency] track={} plugin={} output={} pdc_delay={}",
                    track.id,
                    latency_graph.track_plugin_latency[idx],
                    latency_graph.track_output_latency[idx],
                    latency_graph.track_pdc_delay[idx],
                );
            }
        }

        Ok(Self {
            sample_rate: output_sample_rate,
            tracks,
            clips,
            has_solo,
            samples_per_beat,
            midi_clips,
            midi_tracks,
            audio_graph,
            latency_graph,
            // Installed by the control thread after build; never carried in a
            // freshly built project (preserved across reloads in drain_commands).
            plugin_bridge_sinks: std::collections::HashMap::new(),
            bridge_editor_active: std::collections::HashSet::new(),
        })
    }

    /// Reposition every MIDI track's cursor to the first event at/after
    /// `position_sample` and clear active notes (emitting note-offs so the
    /// destination never gets a stuck note). Called on seek / play-from.
    pub fn reset_midi_playback(&mut self, position_sample: u64) {
        self.all_notes_off("seek/play");
        for mt in &mut self.midi_tracks {
            // Binary search: first event with sample >= position.
            mt.cursor = mt.events.partition_point(|ev| ev.sample < position_sample);
        }
        if midi_engine_debug_enabled() {
            eprintln!(
                "[DAUx MIDI] reset_midi_playback pos={}sa tracks={}",
                position_sample,
                self.midi_tracks.len()
            );
        }
    }

    /// Rebuild MIDI event sample positions from their canonical beat positions
    /// after a static BPM change. Returns the sample position that preserves the
    /// current musical playhead beat under the new tempo.
    pub fn set_static_midi_tempo(&mut self, bpm: f64, position_sample: u64) -> u64 {
        let previous_spb = self.samples_per_beat.max(1.0);
        let current_beat = position_sample as f64 / previous_spb;
        let next_spb = if bpm > 0.0 {
            self.sample_rate.max(1) as f64 * 60.0 / bpm
        } else {
            previous_spb
        };
        self.all_notes_off("tempo_change");
        self.samples_per_beat = next_spb.max(1.0);
        for clip in &mut self.midi_clips {
            for event in &mut clip.events {
                event.sample = beat_to_sample(event.beat, self.samples_per_beat);
            }
            sort_midi_events(&mut clip.events);
        }
        let next_position = beat_to_sample(current_beat, self.samples_per_beat);
        for mt in &mut self.midi_tracks {
            for event in &mut mt.events {
                event.sample = beat_to_sample(event.beat, self.samples_per_beat);
            }
            sort_midi_events(&mut mt.events);
            mt.cursor = mt.events.partition_point(|ev| ev.sample < next_position);
            mt.active.clear();
        }
        next_position
    }

    /// Emit note-off for all active notes on every MIDI track and clear the
    /// active set. Called on stop/seek to prevent stuck notes.
    pub fn all_notes_off(&mut self, reason: &str) {
        let debug = midi_engine_debug_enabled();
        if reason.contains("seek") {
            for mt in &self.midi_tracks {
                if mt.active.is_empty() {
                    continue;
                }
                if let Some(ti) = self.tracks.iter().position(|t| t.id == mt.track_id) {
                    if let Some(ix) = self.tracks[ti].midi_instrument_insert_ix {
                        if let Some(instance) = self.tracks[ti].inserts.get(ix) {
                            eprintln!(
                                "[midi-playback] seek panic old_notes={} instance={}",
                                mt.active.len(),
                                instance.id
                            );
                        }
                    }
                }
            }
        }
        let flush: Vec<(String, Vec<(u8, u8)>)> = self
            .midi_tracks
            .iter()
            .map(|mt| (mt.track_id.clone(), mt.active.clone()))
            .collect();
        for (track_id, active) in flush {
            if debug {
                eprintln!(
                    "[MidiPanic] track={} reason={} active_notes_cleared={}",
                    track_id,
                    reason,
                    active.len()
                );
            }
            push_all_notes_off_for_track(self, &track_id, &active);
        }
        for mt in &mut self.midi_tracks {
            mt.active.clear();
            mt.preview_active.clear();
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
        let verbose = crate::forensic_trace::engine_midi_verbose_enabled();
        let trace = crate::forensic_trace::engine_midi_trace_enabled();
        let vst3_debug = vst3_midi_debug_enabled();
        let spb = self.samples_per_beat.max(1.0);
        let bpm = if spb > 0.0 {
            self.sample_rate as f64 * 60.0 / spb
        } else {
            120.0
        };
        let heartbeat = trace && crate::forensic_trace::scheduler_heartbeat_due();
        for mt in &mut self.midi_tracks {
            let mut scheduled = 0u32;
            let mut _notes_on = 0u32;
            let mut _notes_off = 0u32;
            let bridged = self.plugin_bridge_sinks.contains_key(&mt.track_id);
            let instrument_instance = self
                .tracks
                .iter()
                .find(|t| t.id == mt.track_id)
                .and_then(|t| {
                    t.midi_instrument_insert_ix
                        .and_then(|ix| t.inserts.get(ix).map(|i| i.id.clone()))
                });
            let overlapping: Vec<_> = self
                .midi_clips
                .iter()
                .filter(|c| {
                    c.track_id == mt.track_id
                        && block_end > beat_to_sample(c.start_beat, spb)
                        && base_sample < beat_to_sample(c.end_beat, spb)
                })
                .collect();
            let _clips_active = overlapping.len();
            let block_has_note = overlapping.iter().any(|c| {
                c.events.iter().any(|ev| {
                    ev.sample >= base_sample
                        && ev.sample < block_end
                        && matches!(ev.kind, RuntimeMidiEventKind::NoteOn)
                })
            });
            if trace && (block_has_note || heartbeat) {
                eprintln!(
                    "[midi-scheduler] playing=true bpm={bpm:.1} sr={} block_start={base_sample} block_end={block_end}",
                    self.sample_rate
                );
            }
            if trace && !overlapping.is_empty() && (block_has_note || heartbeat) {
                for clip in &overlapping {
                    eprintln!(
                        "[midi-scheduler] track={} clip={} overlaps=true",
                        mt.track_id, clip.id
                    );
                }
            }
            if trace && bridged {
                if let Some(ref instance_id) = instrument_instance {
                    eprintln!(
                        "[instrument-route] track={} instrument_instance={}",
                        mt.track_id, instance_id
                    );
                    eprintln!("[instrument-route] plugin_instance_id={instance_id}");
                    eprintln!("[instrument-route] route_ok=true");
                }
            }
            while mt.cursor < mt.events.len() && mt.events[mt.cursor].sample < block_end {
                let ev = mt.events[mt.cursor].clone();
                mt.cursor += 1;
                if ev.sample < base_sample {
                    apply_active(&mut mt.active, &ev);
                    continue;
                }
                let offset = (ev.sample - base_sample) as u32;
                apply_active(&mut mt.active, &ev);
                if let Some(ti) = self.tracks.iter().position(|t| t.id == mt.track_id) {
                    if self.tracks[ti].midi_instrument_insert_ix.is_some() {
                        let vel = ev.velocity as f32 / 127.0;
                        let midi_ev = match ev.kind {
                            RuntimeMidiEventKind::NoteOn => {
                                Vst3MidiEvent::note_on(offset, ev.channel, ev.pitch, vel)
                            }
                            RuntimeMidiEventKind::NoteOff => {
                                Vst3MidiEvent::note_off(offset, ev.channel, ev.pitch, 0.0)
                            }
                            RuntimeMidiEventKind::ControlChange => Vst3MidiEvent::control_change(
                                offset,
                                ev.channel,
                                ev.cc_number,
                                ev.cc_value,
                            ),
                        };
                        if bridged {
                            if let Some(ref instance_id) = instrument_instance {
                                if let Some(sink) = self.plugin_bridge_sinks.get(&mt.track_id) {
                                    push_vst3_midi_event_to_sink(
                                        sink.as_ref(),
                                        &midi_ev,
                                        instance_id,
                                        verbose,
                                    );
                                }
                                match ev.kind {
                                    RuntimeMidiEventKind::NoteOn => {
                                        _notes_on += 1;
                                        if trace {
                                            let abs = base_sample + u64::from(offset);
                                            eprintln!(
                                                "[midi-scheduler] note_on pitch={} offset={offset} \
                                                 absolute_sample={abs} instance={instance_id}",
                                                ev.pitch
                                            );
                                        }
                                    }
                                    RuntimeMidiEventKind::NoteOff => {
                                        _notes_off += 1;
                                        if trace {
                                            let abs = base_sample + u64::from(offset);
                                            eprintln!(
                                                "[midi-scheduler] note_off pitch={} offset={offset} \
                                                 absolute_sample={abs} instance={instance_id}",
                                                ev.pitch
                                            );
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        } else {
                            self.tracks[ti].midi_block_events.push(midi_ev);
                        }
                    } else if vst3_debug {
                        eprintln!(
                            "[VST3 MIDI] skip track={} reason=no_instrument_insert",
                            mt.track_id
                        );
                    }
                }
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
                        RuntimeMidiEventKind::ControlChange => eprintln!(
                            "[DAUx MIDI] cc ch={} ctrl={} value={:.3} offset={}",
                            ev.channel, ev.cc_number, ev.cc_value, offset
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
                    bs,
                    be,
                    mt.track_id,
                    scheduled,
                    mt.active.len()
                );
            }
            if debug && scheduled > 0 {
                eprintln!(
                    "[DAUx MIDI] block events={} track={}",
                    scheduled, mt.track_id
                );
            }
            if vst3_debug {
                if let Some(ti) = self.tracks.iter().position(|t| t.id == mt.track_id) {
                    if let Some(ix) = self.tracks[ti].midi_instrument_insert_ix {
                        eprintln!(
                            "[VST3 MIDI] instrument insert track={} insert_ix={} block_events={}",
                            mt.track_id,
                            ix,
                            self.tracks[ti].midi_block_events.len()
                        );
                    }
                }
            }
        }
    }

    pub fn midi_preview_note_on(&mut self, track_id: &str, channel: u8, pitch: u8, velocity: u8) {
        self.bridge_preview_note_on(track_id, "", channel, pitch, velocity);
    }

    pub fn midi_preview_note_off(&mut self, track_id: &str, channel: u8, pitch: u8) {
        self.bridge_preview_note_off(track_id, "", channel, pitch);
    }

    pub fn midi_preview_all_notes_off(&mut self, track_id: &str) {
        self.bridge_preview_all_notes_off(track_id, "");
    }

    /// Push a preview note-on on the audio thread. When a bridge sink is
    /// installed, writes directly into the shared MIDI ring (sample_offset=0).
    pub fn bridge_preview_note_on(
        &mut self,
        track_id: &str,
        plugin_instance_id: &str,
        channel: u8,
        pitch: u8,
        velocity: u8,
    ) {
        let channel = channel.min(15);
        let pitch = pitch.min(127);
        let velocity = velocity.clamp(1, 127);
        let bridged = self.plugin_bridge_sinks.contains_key(track_id);
        if bridged {
            // Shared-memory path: always write into the realtime MIDI ring on the
            // audio thread. Do not rely on midi_block_events / runtime inserts.
            self.push_bridge_preview_midi(
                track_id,
                plugin_instance_id,
                0x90 | channel,
                pitch,
                velocity,
                "note_on",
            );
            self.set_preview_active(track_id, channel, pitch, true);
            return;
        }
        if self.queue_preview_event(
            track_id,
            Vst3MidiEvent::note_on(0, channel, pitch, velocity as f32 / 127.0),
            "note_on",
            channel,
            pitch,
        ) {
            self.set_preview_active(track_id, channel, pitch, true);
        }
    }

    pub fn bridge_preview_note_off(
        &mut self,
        track_id: &str,
        plugin_instance_id: &str,
        channel: u8,
        pitch: u8,
    ) {
        let channel = channel.min(15);
        let pitch = pitch.min(127);
        eprintln!(
            "[EngineMidiPreview] received note_off track={} instance={} ch={} pitch={}",
            track_id, plugin_instance_id, channel, pitch
        );
        let bridged = self.plugin_bridge_sinks.contains_key(track_id);
        if bridged {
            self.push_bridge_preview_midi(
                track_id,
                plugin_instance_id,
                0x80 | channel,
                pitch,
                0,
                "note_off",
            );
            self.set_preview_active(track_id, channel, pitch, false);
            return;
        }
        if self.queue_preview_event(
            track_id,
            Vst3MidiEvent::note_off(0, channel, pitch, 0.0),
            "note_off",
            channel,
            pitch,
        ) {
            self.set_preview_active(track_id, channel, pitch, false);
        }
    }

    pub fn bridge_preview_all_notes_off(&mut self, track_id: &str, plugin_instance_id: &str) {
        let active = self
            .midi_tracks
            .iter()
            .find(|mt| mt.track_id == track_id)
            .map(|mt| mt.preview_active.clone())
            .unwrap_or_default();
        eprintln!(
            "[EngineMidiPreview] received all_notes_off track={} instance={} active_notes={}",
            track_id, plugin_instance_id, active.len()
        );
        push_all_notes_off_for_track(self, track_id, &active);
        if let Some(sink) = self.plugin_bridge_sinks.get(track_id) {
            for &(channel, pitch) in &active {
                sink.push_midi(0x80 | (channel & 0x0F), pitch, 0, 0);
            }
            for ch in 0u8..16 {
                sink.push_midi(0xB0 | (ch & 0x0F), 64, 0, 0);
                sink.push_midi(0xB0 | (ch & 0x0F), 123, 0, 0);
                sink.push_midi(0xB0 | (ch & 0x0F), 120, 0, 0);
            }
        }
        if let Some(mt) = self
            .midi_tracks
            .iter_mut()
            .find(|mt| mt.track_id == track_id)
        {
            mt.preview_active.clear();
        }
    }

    fn push_bridge_preview_midi(
        &self,
        track_id: &str,
        plugin_instance_id: &str,
        status: u8,
        data1: u8,
        data2: u8,
        kind: &str,
    ) {
        let Some(sink) = self.plugin_bridge_sinks.get(track_id) else {
            if midi_verbose_enabled() {
                eprintln!(
                    "[plugin-dsp-midi] write skipped track={track_id} reason=no_bridge_sink keys={:?}",
                    self.plugin_bridge_sinks.keys().collect::<Vec<_>>()
                );
            }
            return;
        };
        sink.push_midi(status, data1, data2, 0);
        if crate::forensic_trace::engine_midi_verbose_enabled() {
            let instance = if plugin_instance_id.is_empty() {
                "unknown"
            } else {
                plugin_instance_id
            };
            let seq = MIDI_WRITE_SEQ.fetch_add(1, Ordering::Relaxed);
            eprintln!(
                "[plugin-dsp-midi-write] preview {kind} instance={instance} pitch={data1} offset=0"
            );
            eprintln!(
                "[plugin-dsp-midi-write] seq={seq} instance={instance} events=1"
            );
        }
    }

    pub fn has_active_midi_preview(&self) -> bool {
        self.midi_tracks
            .iter()
            .any(|mt| !mt.preview_active.is_empty())
    }

    fn queue_preview_event(
        &mut self,
        track_id: &str,
        event: Vst3MidiEvent,
        event_type: &str,
        channel: u8,
        pitch: u8,
    ) -> bool {
        let Some(ti) = self.tracks.iter().position(|t| t.id == track_id) else {
            eprintln!(
                "[InstrumentRoute] track={} no instrument plugin found reason=missing_track",
                track_id
            );
            return false;
        };
        let track = &self.tracks[ti];
        let plugins = track
            .inserts
            .iter()
            .map(|insert| {
                format!(
                    "{}:{}:{}",
                    insert.id,
                    insert.kind,
                    if insert.enabled {
                        "enabled"
                    } else {
                        "disabled"
                    }
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        eprintln!(
            "[InstrumentRoute] track={} kind={} plugins={}",
            track.id, track.track_type, plugins
        );
        let Some(insert_ix) = track.midi_instrument_insert_ix else {
            eprintln!(
                "[InstrumentRoute] track={} selected_instrument_plugin=none no instrument plugin found",
                track_id
            );
            return false;
        };
        let plugin_id = self.tracks[ti].inserts[insert_ix].id.clone();
        eprintln!(
            "[InstrumentRoute] track={} selected_instrument_plugin={}",
            track_id, plugin_id
        );
        eprintln!(
            "[PluginMidiIn] plugin={} {} ch={} pitch={} offset=0",
            plugin_id, event_type, channel, pitch
        );
        self.tracks[ti].midi_block_events.push(event);
        eprintln!(
            "[EngineMidiPreview] target plugin={} event queued",
            plugin_id
        );
        true
    }

    fn set_preview_active(&mut self, track_id: &str, channel: u8, pitch: u8, active: bool) {
        let Some(mt) = self
            .midi_tracks
            .iter_mut()
            .find(|mt| mt.track_id == track_id)
        else {
            self.midi_tracks.push(RuntimeMidiTrack {
                track_id: track_id.to_string(),
                events: Vec::new(),
                cursor: 0,
                active: Vec::with_capacity(128),
                preview_active: Vec::with_capacity(128),
            });
            let Some(mt) = self
                .midi_tracks
                .iter_mut()
                .find(|mt| mt.track_id == track_id)
            else {
                return;
            };
            if active {
                mt.preview_active.push((channel, pitch));
            }
            return;
        };
        let key = (channel, pitch);
        if active {
            if !mt.preview_active.contains(&key) {
                mt.preview_active.push(key);
            }
        } else {
            mt.preview_active.retain(|k| *k != key);
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

    /// Deliver pending `midi_block_events` to instrument VST3 inserts when the
    /// transport is stopped but stop/seek queued note-offs must still reach the
    /// plugin (prevents stuck notes).
    pub fn flush_vst3_midi_inserts(&mut self, frames: usize) {
        if frames == 0 {
            return;
        }
        for track in &mut self.tracks {
            if track.midi_block_events.is_empty() {
                continue;
            }
            let insert_ix = match track.midi_instrument_insert_ix {
                Some(ix) => ix,
                None => {
                    track.midi_block_events.clear();
                    continue;
                }
            };
            let events = std::mem::take(&mut track.midi_block_events);
            if track.block_l.len() < frames || track.block_r.len() < frames {
                continue;
            }
            track.block_l[..frames].fill(0.0);
            track.block_r[..frames].fill(0.0);
            let insert = &mut track.inserts[insert_ix];
            let Some(vst3) = insert.vst3.as_mut() else {
                continue;
            };
            if !vst3.is_processor_valid() {
                continue;
            }
            if insert.scratch_l.len() < frames {
                insert.scratch_l.resize(frames, 0.0);
                insert.scratch_r.resize(frames, 0.0);
            }
            insert.scratch_l[..frames].fill(0.0);
            insert.scratch_r[..frames].fill(0.0);
            let _ = vst3.process_stereo_block_with_midi(
                &insert.scratch_l[..frames],
                &insert.scratch_r[..frames],
                &mut track.block_l[..frames],
                &mut track.block_r[..frames],
                &events,
            );
        }
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
    pub fn accumulate_live_input_meters(&mut self, latest_l: f32, latest_r: f32) {
        if latest_l == 0.0 && latest_r == 0.0 {
            return;
        }
        for track in &mut self.tracks {
            if track.track_type != "audio" {
                continue;
            }
            if !track.record_armed && !track.monitor_enabled {
                continue;
            }
            if !track.input_source.is_routable() {
                continue;
            }
            let (l, r) = track.input_source.sample_from_latest(latest_l, latest_r);
            let abs_l = l.abs();
            let abs_r = r.abs();
            track.meter_peak_l = track.meter_peak_l.max(abs_l);
            track.meter_peak_r = track.meter_peak_r.max(abs_r);
            track.meter_sum_sq_l += l * l;
            track.meter_sum_sq_r += r * r;
        }
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

    pub fn set_bridge_editor_active(&mut self, track_id: &str, active: bool) {
        if active {
            self.bridge_editor_active.insert(track_id.to_string());
        } else {
            self.bridge_editor_active.remove(track_id);
        }
    }

    pub fn has_bridge_editor_active(&self) -> bool {
        !self.bridge_editor_active.is_empty()
    }
}

static MIDI_WRITE_SEQ: AtomicU32 = AtomicU32::new(0);

pub fn push_vst3_midi_event_to_sink(
    sink: &dyn crate::plugin_bridge::PluginBridgeSink,
    ev: &Vst3MidiEvent,
    instance_id: &str,
    verbose: bool,
) {
    let channel = ev.channel & 0x0F;
    let seq = MIDI_WRITE_SEQ.fetch_add(1, Ordering::Relaxed);
    match ev.kind {
        1 => {
            let vel = (ev.velocity.clamp(0.0, 1.0) * 127.0).round() as u8;
            if vel > 0 {
                sink.push_midi(0x90 | channel, ev.pitch, vel, ev.sample_offset);
                if verbose {
                    eprintln!(
                        "[plugin-dsp-midi-write] seq={seq} instance={instance_id} events=1"
                    );
                    eprintln!(
                        "[plugin-dsp-midi-write] note_on pitch={} offset={} ch={channel}",
                        ev.pitch, ev.sample_offset
                    );
                }
            } else {
                sink.push_midi(0x80 | channel, ev.pitch, 0, ev.sample_offset);
                if verbose {
                    eprintln!(
                        "[plugin-dsp-midi-write] seq={seq} instance={instance_id} events=1"
                    );
                    eprintln!(
                        "[plugin-dsp-midi-write] note_off pitch={} offset={} ch={channel}",
                        ev.pitch, ev.sample_offset
                    );
                }
            }
        }
        0 => {
            sink.push_midi(0x80 | channel, ev.pitch, 0, ev.sample_offset);
            if verbose {
                eprintln!(
                    "[plugin-dsp-midi-write] seq={seq} instance={instance_id} events=1"
                );
                eprintln!(
                    "[plugin-dsp-midi-write] note_off pitch={} offset={} ch={channel}",
                    ev.pitch, ev.sample_offset
                );
            }
        }
        2 => {
            let val = (ev.velocity.clamp(0.0, 1.0) * 127.0).round() as u8;
            sink.push_midi(0xB0 | channel, ev.pitch, val, ev.sample_offset);
            if verbose {
                eprintln!(
                    "[plugin-dsp-midi-write] seq={seq} instance={instance_id} events=1 cc={} val={val}",
                    ev.pitch
                );
            }
        }
        _ => {}
    }
}

/// First native VST3 insert that should receive scheduled MIDI for this track.
fn find_midi_instrument_insert_ix(inserts: &[RuntimeInsert], track_type: &str) -> Option<usize> {
    inserts.iter().enumerate().find_map(|(ix, insert)| {
        if insert_accepts_midi_events(insert, track_type) {
            Some(ix)
        } else {
            None
        }
    })
}

#[inline]
fn insert_accepts_midi_events(insert: &RuntimeInsert, track_type: &str) -> bool {
    if !insert.enabled {
        return false;
    }
    let is_bridge = insert.kind.eq_ignore_ascii_case("external-bridge-plugin");
    if !is_bridge && insert.vst3.is_none() {
        return false;
    }
    let ty = track_type.to_ascii_lowercase();
    if ty == "instrument" || ty == "midi" {
        return true;
    }
    let cat = insert
        .params
        .get("category")
        .or_else(|| insert.params.get("pluginCategory"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let cat_lc = cat.to_ascii_lowercase();
    if cat_lc.contains("instrument") || cat_lc.contains("synth") {
        return true;
    }
    insert
        .params
        .get("acceptsMidi")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn push_all_notes_off_for_track(project: &mut RuntimeProject, track_id: &str, active: &[(u8, u8)]) {
    let Some(ti) = project.tracks.iter().position(|t| t.id == track_id) else {
        return;
    };
    if project.tracks[ti].midi_instrument_insert_ix.is_none() {
        return;
    }
    let instance_id = project.tracks[ti]
        .midi_instrument_insert_ix
        .and_then(|ix| project.tracks[ti].inserts.get(ix).map(|i| i.id.clone()));
    if let Some(sink) = project.plugin_bridge_sinks.get(track_id) {
        if let Some(ref instance) = instance_id {
            eprintln!(
                "[midi-playback] transport_stop panic instance={instance} old_notes={}",
                active.len()
            );
        }
        for &(channel, pitch) in active {
            sink.push_midi(0x80 | (channel & 0x0F), pitch, 0, 0);
        }
        for ch in 0u8..16 {
            sink.push_midi(0xB0 | (ch & 0x0F), 64, 0, 0);
            sink.push_midi(0xB0 | (ch & 0x0F), 123, 0, 0);
            sink.push_midi(0xB0 | (ch & 0x0F), 120, 0, 0);
        }
        return;
    }
    for &(channel, pitch) in active {
        project.tracks[ti]
            .midi_block_events
            .push(Vst3MidiEvent::note_off(0, channel, pitch, 0.0));
    }
    for channel in 0..16 {
        project.tracks[ti]
            .midi_block_events
            .push(Vst3MidiEvent::control_change(0, channel, 64, 0.0));
        project.tracks[ti]
            .midi_block_events
            .push(Vst3MidiEvent::control_change(0, channel, 123, 0.0));
        project.tracks[ti]
            .midi_block_events
            .push(Vst3MidiEvent::control_change(0, channel, 120, 0.0));
        project.tracks[ti]
            .midi_block_events
            .push(Vst3MidiEvent::control_change(0, channel, 121, 0.0));
    }
}

#[inline]
fn beat_to_sample(beat: f64, samples_per_beat: f64) -> u64 {
    (beat.max(0.0) * samples_per_beat.max(1.0)).round().max(0.0) as u64
}

fn sort_midi_events(events: &mut [RuntimeMidiEvent]) {
    events.sort_by(|a, b| {
        a.sample
            .cmp(&b.sample)
            .then((a.kind as u8).cmp(&(b.kind as u8)))
    });
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
        // Controller changes carry no sounding-note state.
        RuntimeMidiEventKind::ControlChange => {}
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
            let on_sample = beat_to_sample(abs_start, samples_per_beat);
            let off_sample = beat_to_sample(abs_end, samples_per_beat);
            events.push(RuntimeMidiEvent {
                sample: on_sample,
                beat: abs_start,
                kind: RuntimeMidiEventKind::NoteOn,
                pitch,
                velocity,
                channel,
                note_id: note.id,
                cc_number: 0,
                cc_value: 0.0,
            });
            events.push(RuntimeMidiEvent {
                sample: off_sample,
                beat: abs_end,
                kind: RuntimeMidiEventKind::NoteOff,
                pitch,
                velocity: 0,
                channel,
                note_id: note.id,
                cc_number: 0,
                cc_value: 0.0,
            });
        }
        // Controller points → ControlChange events (block-level value).
        for lane in &clip.controllers {
            let channel = lane.channel.min(15);
            for point in &lane.points {
                let abs_beat = clip.start_beat + point.beat.max(0.0);
                let sample = beat_to_sample(abs_beat, samples_per_beat);
                events.push(RuntimeMidiEvent {
                    sample,
                    beat: abs_beat,
                    kind: RuntimeMidiEventKind::ControlChange,
                    pitch: 0,
                    velocity: 0,
                    channel,
                    note_id: 0,
                    cc_number: lane.controller,
                    cc_value: point.value.clamp(0.0, 1.0),
                });
            }
        }
        // Sort by sample; NoteOff before NoteOn at the same sample.
        sort_midi_events(&mut events);
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
            sort_midi_events(&mut events);
            let active = Vec::with_capacity(128); // bound growth out of the audio callback
            RuntimeMidiTrack {
                track_id,
                events,
                cursor: 0,
                active,
                preview_active: Vec::with_capacity(128),
            }
        })
        .collect();
    midi_tracks.sort_by(|a, b| a.track_id.cmp(&b.track_id));

    (clips, midi_tracks)
}

fn build_clip_runtime(
    clip: &EngineClipSnapshot,
    source: Arc<ClipAudioSource>,
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

    let duration_samples = seconds_to_samples(duration_seconds, output_sample_rate).max(1);

    // Resolve fade durations (seconds) → output samples. Clamp so the two
    // fades never overlap or exceed the clip length.
    let (fade_in_samples, fade_out_samples) = clip
        .fades
        .as_ref()
        .map(|f| {
            let fi = seconds_to_samples(f.in_duration.max(0.0), output_sample_rate);
            let fo = seconds_to_samples(f.out_duration.max(0.0), output_sample_rate);
            (fi, fo)
        })
        .unwrap_or((0, 0));
    let fade_in_samples = fade_in_samples.min(duration_samples);
    let fade_out_samples = fade_out_samples.min(duration_samples.saturating_sub(fade_in_samples));

    Some(RuntimeClip {
        id: clip.id.clone(),
        track_id: clip.track_id.clone(),
        start_sample: seconds_to_samples(start_seconds.max(0.0), output_sample_rate),
        duration_samples,
        offset_seconds: clip.offset_seconds.max(0.0),
        gain: clip.gain.clamp(0.0, 4.0),
        speed_ratio,
        muted: clip.muted,
        fade_in_samples,
        fade_out_samples,
        source,
    })
}

/// Evaluate a sorted automation point list without allocating. Empty lanes use
/// `default`; before/after the authored range, the nearest point is held.
pub fn evaluate_automation_points(
    points: &[RuntimeAutomationPoint],
    beat: f64,
    default: f32,
) -> f32 {
    if points.is_empty() {
        return default.clamp(0.0, 1.0);
    }
    let beat = beat.max(0.0);
    if beat <= points[0].beat {
        return points[0].value;
    }
    let last = points.len() - 1;
    if beat >= points[last].beat {
        return points[last].value;
    }

    for i in 0..last {
        let a = &points[i];
        let b = &points[i + 1];
        if beat >= a.beat && beat <= b.beat {
            return match a.curve {
                RuntimeAutomationCurve::Hold => a.value,
                RuntimeAutomationCurve::Linear | RuntimeAutomationCurve::Smooth => {
                    let span = (b.beat - a.beat).max(f64::EPSILON);
                    let t = ((beat - a.beat) / span).clamp(0.0, 1.0) as f32;
                    a.value + (b.value - a.value) * t
                }
            };
        }
    }
    points[last].value
}

pub const AUTOMATION_VOLUME_MIN_DB: f32 = -60.0;
pub const AUTOMATION_VOLUME_MAX_DB: f32 = 6.0;

#[inline]
pub fn volume_db_to_norm(db: f32) -> f32 {
    ((db - AUTOMATION_VOLUME_MIN_DB) / (AUTOMATION_VOLUME_MAX_DB - AUTOMATION_VOLUME_MIN_DB))
        .clamp(0.0, 1.0)
}

#[inline]
pub fn volume_norm_to_linear(norm: f32) -> f32 {
    let norm = norm.clamp(0.0, 1.0);
    let db =
        AUTOMATION_VOLUME_MIN_DB + norm * (AUTOMATION_VOLUME_MAX_DB - AUTOMATION_VOLUME_MIN_DB);
    if norm <= 0.001 || db <= AUTOMATION_VOLUME_MIN_DB + 0.05 {
        0.0
    } else {
        10.0_f32.powf(db / 20.0).clamp(0.0, 2.0)
    }
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
    use crate::types::{
        EngineAutomationLaneSnapshot, EngineMidiClipSnapshot, EngineMidiControllerLane,
        EngineMidiControllerPoint, EngineMidiNoteSnapshot,
    };

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
            controllers: Vec::new(),
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
        let on = evs
            .iter()
            .find(|e| e.kind == RuntimeMidiEventKind::NoteOn)
            .unwrap();
        let off = evs
            .iter()
            .find(|e| e.kind == RuntimeMidiEventKind::NoteOff)
            .unwrap();
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

    #[test]
    fn all_notes_off_clears_preview_tracker() {
        // A held preview/audition note that never received an explicit note-off
        // (e.g. deleted mid-move) must not leave the engine believing a note is
        // still sounding — the panic clears the preview tracker.
        let mut p = project_with(vec![clip_with_one_note()]);
        let track_id = p.midi_tracks[0].track_id.clone();
        p.midi_tracks[0].preview_active.push((0, 60));
        assert!(p.has_active_midi_preview());
        p.midi_preview_all_notes_off(&track_id);
        assert!(p.midi_tracks[0].preview_active.is_empty());
        assert!(!p.has_active_midi_preview());
    }

    #[test]
    fn tempo_change_reschedules_midi_samples_from_beats() {
        let mut p = project_with(vec![clip_with_one_note()]);
        let next_pos = p.set_static_midi_tempo(60.0, 96_000);
        let evs = &p.midi_tracks[0].events;
        let on = evs
            .iter()
            .find(|e| e.kind == RuntimeMidiEventKind::NoteOn)
            .unwrap();
        let off = evs
            .iter()
            .find(|e| e.kind == RuntimeMidiEventKind::NoteOff)
            .unwrap();

        // 60 BPM @ 48 kHz -> 48000 samples/beat. The note stays at beat 4..5,
        // so only its sample positions change.
        assert_eq!(on.beat, 4.0);
        assert_eq!(off.beat, 5.0);
        assert_eq!(on.sample, 192_000);
        assert_eq!(off.sample, 240_000);
        // Current sample 96000 was beat 4 at 120 BPM; preserve beat 4.
        assert_eq!(next_pos, 192_000);
    }

    #[test]
    fn controller_points_resolve_to_control_change_events() {
        let mut clip = clip_with_one_note();
        clip.controllers = vec![EngineMidiControllerLane {
            controller: 11,
            channel: 0,
            points: vec![
                EngineMidiControllerPoint {
                    beat: 0.0,
                    value: 0.25,
                },
                EngineMidiControllerPoint {
                    beat: 2.0,
                    value: 0.75,
                },
            ],
        }];
        let p = project_with(vec![clip]);
        let cc: Vec<&RuntimeMidiEvent> = p.midi_tracks[0]
            .events
            .iter()
            .filter(|e| e.kind == RuntimeMidiEventKind::ControlChange)
            .collect();
        assert_eq!(cc.len(), 2);
        // First point: abs beat 4.0 → 96000 sa, cc 11, value 0.25.
        assert_eq!(cc[0].cc_number, 11);
        assert_eq!(cc[0].sample, 96_000);
        assert!((cc[0].cc_value - 0.25).abs() < 1e-6);
        // Second point: abs beat 6.0 → 144000 sa, value 0.75.
        assert_eq!(cc[1].sample, 144_000);
        assert!((cc[1].cc_value - 0.75).abs() < 1e-6);
    }

    #[test]
    fn control_change_does_not_affect_active_notes() {
        let mut clip = clip_with_one_note();
        clip.controllers = vec![EngineMidiControllerLane {
            controller: 1,
            channel: 0,
            points: vec![EngineMidiControllerPoint {
                beat: 0.0,
                value: 0.5,
            }],
        }];
        let mut p = project_with(vec![clip]);
        p.reset_midi_playback(0);
        // Block covering the CC at abs beat 4.0 (96000) but the note also starts
        // there — active set should track only the note, not the CC.
        p.schedule_midi_block(96_000, 512);
        assert_eq!(p.midi_tracks[0].active, vec![(0u8, 60u8)]);
    }

    #[test]
    fn automation_points_are_sorted_and_clamped_for_runtime() {
        let lane = RuntimeAutomationLane::from_snapshot(&EngineAutomationLaneSnapshot {
            id: "lane-1".into(),
            name: "Volume".into(),
            target: crate::types::EngineAutomationTargetSnapshot {
                tag: 0,
                ..Default::default()
            },
            enabled: true,
            points: vec![
                crate::types::EngineAutomationPointSnapshot {
                    beat: 4.0,
                    value: 2.0,
                    curve: 0,
                },
                crate::types::EngineAutomationPointSnapshot {
                    beat: -1.0,
                    value: -0.5,
                    curve: 1,
                },
            ],
        });

        assert_eq!(lane.points[0].beat, 0.0);
        assert_eq!(lane.points[0].value, 0.0);
        assert_eq!(lane.points[1].beat, 4.0);
        assert_eq!(lane.points[1].value, 1.0);
    }

    #[test]
    fn automation_evaluator_handles_linear_and_hold_curves() {
        let points = vec![
            RuntimeAutomationPoint {
                beat: 0.0,
                value: 0.0,
                curve: RuntimeAutomationCurve::Linear,
            },
            RuntimeAutomationPoint {
                beat: 4.0,
                value: 1.0,
                curve: RuntimeAutomationCurve::Hold,
            },
            RuntimeAutomationPoint {
                beat: 8.0,
                value: 0.25,
                curve: RuntimeAutomationCurve::Linear,
            },
        ];

        assert_eq!(evaluate_automation_points(&[], 2.0, 0.75), 0.75);
        assert!((evaluate_automation_points(&points, 2.0, 0.5) - 0.5).abs() < 1e-6);
        assert!((evaluate_automation_points(&points, 6.0, 0.5) - 1.0).abs() < 1e-6);
        assert!((evaluate_automation_points(&points, 10.0, 0.5) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn disabled_or_unresolved_automation_lanes_do_not_evaluate() {
        let mut lane = RuntimeAutomationLane::from_snapshot(&EngineAutomationLaneSnapshot {
            id: "lane-1".into(),
            name: "Missing Param".into(),
            target: crate::types::EngineAutomationTargetSnapshot {
                tag: 3,
                ..Default::default()
            },
            enabled: true,
            points: vec![crate::types::EngineAutomationPointSnapshot {
                beat: 0.0,
                value: 0.25,
                curve: 0,
            }],
        });
        assert!(lane.evaluate_normalized(0.0).is_none());

        lane.target = RuntimeAutomationTarget::TrackPan;
        lane.enabled = false;
        assert!(lane.evaluate_normalized(0.0).is_none());
    }
}
