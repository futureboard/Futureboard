//! Runtime playback graph sent to the CPAL callback.
//!
//! The control thread builds this from an `EngineProjectSnapshot`, including
//! decoding supported media files.  The audio thread then owns a local clone of
//! the graph and can render without touching locks or parsing JSON.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use crate::audio_graph::{plan_runtime_audio_graph, GraphValidationError, RuntimeAudioGraph};
use crate::audio_source::{open_clip_audio_source, ClipAudioSource};
use crate::latency_graph::{plan_runtime_latency_graph, RuntimeLatencyGraph};
use serde_json::Value;
use sphere_audio_plugins::{canonical_plugin_id, should_rebuild_state, AudioPluginDspState};
use SphereAudioProcessor::{
    create_stretch_processor, effective_pitch_ratio, effective_time_ratio, resolve_backend,
    source_read_rate_for_repitch, stretched_duration_samples, StretchAlgorithm, StretchBackend,
    StretchMode, StretchParams, StretchProcessor,
};

use crate::tempo_map::{RuntimeTempoMapSnapshot, TempoMap, TempoPoint};
use crate::types::{
    EngineAutomationLaneSnapshot, EngineClipAudioProcess, EngineClipSnapshot,
    EngineMidiClipSnapshot, EngineProjectSnapshot,
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
    /// [`Self::output_track_id`] resolved at build time
    /// ([`RuntimeProject::resolve_indices`]): `Some(index)` only when the id
    /// names an existing non-master track. The render path must never do a
    /// per-block id lookup.
    pub output_track_index: Option<usize>,
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

/// `RuntimeInsert::kind` resolved to a compact tag at build time so the render
/// path never does per-block string compares (realtime rules).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeInsertKind {
    /// In-process VST3 (`kind == "native-plugin"`).
    NativePlugin,
    /// Out-of-process bridged plugin (`kind == "external-bridge-plugin"`).
    ExternalBridge,
    /// Built-in DSP insert (everything else).
    BuiltIn,
}

impl RuntimeInsertKind {
    pub fn from_kind(kind: &str) -> Self {
        if kind.eq_ignore_ascii_case("native-plugin") {
            Self::NativePlugin
        } else if kind.eq_ignore_ascii_case("external-bridge-plugin") {
            Self::ExternalBridge
        } else {
            Self::BuiltIn
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeInsert {
    pub id: String,
    pub kind: String,
    /// [`Self::kind`] resolved at build time — the audio callback branches on
    /// this tag, never on the string.
    pub kind_tag: RuntimeInsertKind,
    pub enabled: bool,
    pub params: HashMap<String, Value>,
    /// For [`RuntimeInsertKind::ExternalBridge`]: `params["role"] == "effect"`,
    /// resolved at build time so the block path never reads the params map.
    pub bridge_is_effect: bool,
    /// For [`RuntimeInsertKind::ExternalBridge`]: the installed realtime sink.
    /// Cached from [`RuntimeProject::plugin_bridge_sinks`] by
    /// [`RuntimeProject::resolve_bridge_sinks`] (LoadProject /
    /// SetPluginBridgeSink) so the block path never does a `HashMap<String, _>`
    /// lookup.
    pub bridge_sink: Option<crate::plugin_bridge::SharedPluginBridgeSink>,
    pub dsp: InsertDspState,
    pub vst3: Option<Vst3RuntimeProcessor>,
    pub callback_process_log_done: bool,
    pub silent_process_blocks: u32,
    /// Consecutive blocks the external plugin host failed to deliver on time
    /// (its `read_output` returned 0). Drives the throttled missed-deadline /
    /// recovered logs in `apply_external_bridge_insert_block`.
    pub bridge_missed_blocks: u32,
    pub scratch_l: Vec<f32>,
    pub scratch_r: Vec<f32>,
}

pub type InsertDspState = AudioPluginDspState;

const DEFAULT_AUDIO_BLOCK_CAPACITY: usize = 8192;

#[derive(Debug, Clone)]
pub struct RuntimeSend {
    pub id: String,
    pub return_track_id: String,
    /// [`Self::return_track_id`] resolved to a track index at build time
    /// ([`RuntimeProject::resolve_indices`]) — `None` when the target track
    /// does not exist. The render path must never do a per-block id lookup.
    pub return_track_index: Option<usize>,
    pub level: f32,
    pub enabled: bool,
    /// Pre-fader tap (Phase 3). See [`EngineSendSnapshot::pre_fader`].
    pub pre_fader: bool,
}

pub struct RuntimeClip {
    pub id: String,
    pub track_id: String,
    /// [`Self::track_id`] resolved to a track index at build time
    /// ([`RuntimeProject::resolve_indices`]); `None` when the track is missing.
    pub track_index: Option<usize>,
    pub start_sample: u64,
    pub duration_samples: u64,
    pub offset_seconds: f64,
    pub gain: f32,
    /// Immutable stretch parameters copied from the project snapshot for the
    /// audio thread. SphereAudioProcessor is the source of truth for all derived
    /// ratios/backend decisions.
    pub stretch: StretchParams,
    pub speed_ratio: f32,
    pub source_read_rate: f32,
    pub effective_time_ratio: f32,
    pub pitch_ratio: f32,
    pub stretch_backend: StretchBackend,
    pub source_start_samples: u64,
    pub source_end_samples: u64,
    pub warp_markers: Vec<RuntimeWarpMarker>,
    pub processor: ClipDspProcessor,
    /// Play the source window backwards (resolved from the snapshot's
    /// `audio_process.reverse`). The render maps output → source from the clip
    /// end instead of the start; `speed_ratio` is unchanged.
    pub reverse: bool,
    /// Clip-level mute — a muted clip is skipped entirely during render.
    pub muted: bool,
    /// Linear fade lengths in output samples, resolved from the snapshot's
    /// fade durations at build time. `0` means no fade. Clamped so
    /// `fade_in + fade_out <= duration_samples`. Curve shaping beyond linear is
    /// a placeholder (see `clip_fade_gain`).
    pub fade_in_samples: u64,
    pub fade_out_samples: u64,
    pub source: Arc<ClipAudioSource>,
    /// Cached preserve-pitch processor for this runtime clip/voice. Created on
    /// the control thread while building/cloning the runtime graph; the audio
    /// thread only calls `reset`/`process_stereo` on it.
    pub stretch_processor: Option<Box<dyn StretchProcessor + Send>>,
    pub stretch_input_l: Vec<f32>,
    pub stretch_input_r: Vec<f32>,
    pub stretch_output_l: Vec<f32>,
    pub stretch_output_r: Vec<f32>,
    /// Pre-roll scratch fed to `StretchProcessor::output_seek` to latency-align
    /// the stretcher output to the timeline on (re)start. Grows lazily to the
    /// largest seek length seen, then stays stable (no steady-state alloc).
    pub stretch_prime_l: Vec<f32>,
    pub stretch_prime_r: Vec<f32>,
    pub stretch_next_project_sample: Option<u64>,
}

impl std::fmt::Debug for RuntimeClip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeClip")
            .field("id", &self.id)
            .field("track_id", &self.track_id)
            .field("track_index", &self.track_index)
            .field("start_sample", &self.start_sample)
            .field("duration_samples", &self.duration_samples)
            .field("offset_seconds", &self.offset_seconds)
            .field("gain", &self.gain)
            .field("stretch", &self.stretch)
            .field("source_read_rate", &self.source_read_rate)
            .field("effective_time_ratio", &self.effective_time_ratio)
            .field("pitch_ratio", &self.pitch_ratio)
            .field("stretch_backend", &self.stretch_backend)
            .field("processor", &self.processor)
            .field("reverse", &self.reverse)
            .field("muted", &self.muted)
            .field("has_stretch_processor", &self.stretch_processor.is_some())
            .finish()
    }
}

impl Clone for RuntimeClip {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            track_id: self.track_id.clone(),
            track_index: self.track_index,
            start_sample: self.start_sample,
            duration_samples: self.duration_samples,
            offset_seconds: self.offset_seconds,
            gain: self.gain,
            stretch: self.stretch.clone(),
            speed_ratio: self.speed_ratio,
            source_read_rate: self.source_read_rate,
            effective_time_ratio: self.effective_time_ratio,
            pitch_ratio: self.pitch_ratio,
            stretch_backend: self.stretch_backend,
            source_start_samples: self.source_start_samples,
            source_end_samples: self.source_end_samples,
            warp_markers: self.warp_markers.clone(),
            processor: self.processor,
            reverse: self.reverse,
            muted: self.muted,
            fade_in_samples: self.fade_in_samples,
            fade_out_samples: self.fade_out_samples,
            source: Arc::clone(&self.source),
            stretch_processor: create_runtime_stretch_processor(
                self.stretch_backend,
                self.source.sample_rate(),
                &self.stretch,
            ),
            stretch_input_l: vec![0.0; DEFAULT_AUDIO_BLOCK_CAPACITY],
            stretch_input_r: vec![0.0; DEFAULT_AUDIO_BLOCK_CAPACITY],
            stretch_output_l: vec![0.0; DEFAULT_AUDIO_BLOCK_CAPACITY],
            stretch_output_r: vec![0.0; DEFAULT_AUDIO_BLOCK_CAPACITY],
            stretch_prime_l: vec![0.0; self.stretch_prime_l.len()],
            stretch_prime_r: vec![0.0; self.stretch_prime_r.len()],
            stretch_next_project_sample: None,
        }
    }
}

pub type AudioClip = EngineClipSnapshot;

#[derive(Debug, Clone)]
pub struct RuntimeWarpMarker {
    pub id: u64,
    pub source_sample: u64,
    pub timeline_beat: f64,
    pub locked: bool,
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
    /// [`Self::track_id`] resolved to a track index at build time
    /// ([`RuntimeProject::resolve_indices`]); `None` when the track is missing.
    pub track_index: Option<usize>,
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
    /// Authoritative hold-mode tempo map for beat/time/sample conversion.
    pub tempo_map: RuntimeTempoMapSnapshot,
    /// Structural MIDI clips (logging / inspection).
    pub midi_clips: Vec<RuntimeMidiClip>,
    /// Per-track scheduling state driven by the audio callback.
    pub midi_tracks: Vec<RuntimeMidiTrack>,
    /// Precomputed pass order and routing validation (Phase O).
    pub audio_graph: RuntimeAudioGraph,
    /// Latency propagation and PDC delays (Phase V/W).
    pub latency_graph: RuntimeLatencyGraph,
    /// Effective playback PDC flag used when this runtime graph was built.
    pub pdc_enabled: bool,
    /// Stage 3b: realtime sinks for external plugin-host DSP output, keyed by
    /// insert `id` (one region + handshake per insert). Set via
    /// [`crate::command::EngineCommand::SetPluginBridgeSink`]
    /// and preserved across project reloads. Empty until the bridge installs one.
    pub plugin_bridge_sinks: std::collections::HashMap<
        String,
        std::sync::Arc<dyn crate::plugin_bridge::PluginBridgeSink>,
    >,
    /// Tracks whose external-bridge plugin editor is open — keeps the audio
    /// callback rendering while stopped so VSTi internal keyboards stay audible.
    pub bridge_editor_active: std::collections::HashSet<String>,
    /// Samples of post-panic processing still owed to bridged plugin hosts.
    /// Set whenever panic MIDI (note-offs + CC 64/123/120) is pushed into a
    /// bridge sink's ring: the host only drains that ring while blocks are
    /// being requested, so the callback must keep the handshake alive for this
    /// long after a stop/seek/mute panic or the VSTi's voices stay stuck until
    /// the next play. Counted down by the callback while the transport is
    /// stopped; transient, never persisted.
    pub bridge_panic_flush_samples: u64,
}

impl RuntimeProject {
    /// Resolve every cross-entity id reference (clip→track, send→track,
    /// track→output, MIDI track→track) to an index. Called once at build time
    /// on the worker thread; track order is fixed for the life of a runtime
    /// snapshot, so the audio callback only ever reads the precomputed indices.
    pub fn resolve_indices(&mut self) {
        for i in 0..self.clips.len() {
            let ix = {
                let id = &self.clips[i].track_id;
                self.tracks.iter().position(|t| &t.id == id)
            };
            self.clips[i].track_index = ix;
        }
        for i in 0..self.midi_tracks.len() {
            let ix = {
                let id = &self.midi_tracks[i].track_id;
                self.tracks.iter().position(|t| &t.id == id)
            };
            self.midi_tracks[i].track_index = ix;
        }
        for i in 0..self.tracks.len() {
            let out_ix = self.tracks[i]
                .output_track_id
                .as_deref()
                .filter(|id| !crate::engine::is_master_output(id))
                .and_then(|id| self.tracks.iter().position(|t| t.id == id));
            self.tracks[i].output_track_index = out_ix;
            for s in 0..self.tracks[i].sends.len() {
                let target_ix = {
                    let id = &self.tracks[i].sends[s].return_track_id;
                    self.tracks.iter().position(|t| &t.id == id)
                };
                self.tracks[i].sends[s].return_track_index = target_ix;
            }
        }
        self.resolve_bridge_sinks();
    }

    /// Cache each external-bridge insert's realtime sink from
    /// [`Self::plugin_bridge_sinks`] onto the insert itself, so the block path
    /// reads `insert.bridge_sink` instead of doing a `HashMap<String, _>`
    /// lookup. Re-run whenever the sink map changes (LoadProject preserves the
    /// map across graph swaps; SetPluginBridgeSink installs/removes entries).
    /// Arc clones only — no allocation.
    pub fn resolve_bridge_sinks(&mut self) {
        let sinks = &self.plugin_bridge_sinks;
        for track in &mut self.tracks {
            for insert in &mut track.inserts {
                if insert.kind_tag == RuntimeInsertKind::ExternalBridge {
                    insert.bridge_sink = sinks.get(&insert.id).cloned();
                }
            }
        }
    }

    #[inline]
    fn track_insert_latency_samples(&self, track: &RuntimeTrack, bridge_block_frames: u32) -> u32 {
        if track.inserts.is_empty() {
            return track.plugin_latency_samples;
        }
        let mut samples = 0u32;
        for insert in &track.inserts {
            if !insert.enabled {
                continue;
            }
            if insert.kind_tag == RuntimeInsertKind::ExternalBridge {
                if let Some(sink) = insert.bridge_sink.as_ref() {
                    samples = samples
                        .saturating_add(bridge_block_frames)
                        .saturating_add(sink.reported_latency_samples());
                }
                continue;
            }
            if let Some(vst3) = insert.vst3.as_ref().filter(|vst3| vst3.is_ready()) {
                samples = samples.saturating_add(vst3.get_latency_samples().max(0) as u32);
            }
        }
        samples
    }

    fn ensure_pdc_delay_capacity(&mut self) {
        let pdc_buffer_frames = self.latency_graph.max_path_latency_samples.max(1) as usize
            + DEFAULT_AUDIO_BLOCK_CAPACITY;
        for track in &mut self.tracks {
            track.pdc_delay_l.resize(pdc_buffer_frames, 0.0);
            track.pdc_delay_r.resize(pdc_buffer_frames, 0.0);
            track.pdc_write_pos = 0;
        }
    }

    /// Refresh PDC planning when runtime-only bridge latency changes. The
    /// steady-state path scans existing tracks/inserts without allocation; graph
    /// rebuild + delay-line resize only happens when an observed latency differs
    /// from the active plan.
    pub fn refresh_runtime_latency_graph(&mut self, bridge_block_frames: u32) -> bool {
        let changed = self.tracks.iter().enumerate().any(|(idx, track)| {
            let observed = self.track_insert_latency_samples(track, bridge_block_frames);
            self.latency_graph
                .track_plugin_latency
                .get(idx)
                .copied()
                .unwrap_or(0)
                != observed
        });
        if !changed {
            return false;
        }

        let observed: Vec<u32> = self
            .tracks
            .iter()
            .map(|track| self.track_insert_latency_samples(track, bridge_block_frames))
            .collect();
        for (track, samples) in self.tracks.iter_mut().zip(observed) {
            track.plugin_latency_samples = samples;
        }
        self.latency_graph =
            plan_runtime_latency_graph(&self.tracks, &self.audio_graph, self.pdc_enabled);
        self.ensure_pdc_delay_capacity();

        if std::env::var_os("FUTUREBOARD_PDC_DEBUG").is_some()
            || std::env::var_os("FUTUREBOARD_ROUTING_DEBUG").is_some()
        {
            eprintln!(
                "[pdc] refreshed max_path={} pdc_enabled={}",
                self.latency_graph.max_path_latency_samples, self.pdc_enabled
            );
            for (idx, track) in self.tracks.iter().enumerate() {
                eprintln!(
                    "[pdc] track={} plugin={} output={} delay={}",
                    track.id,
                    self.latency_graph.track_plugin_latency[idx],
                    self.latency_graph.track_output_latency[idx],
                    self.latency_graph.track_pdc_delay[idx],
                );
            }
        }
        true
    }

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
                    kind_tag: RuntimeInsertKind::from_kind(&insert.kind),
                    enabled: insert.enabled,
                    bridge_is_effect: insert
                        .params
                        .get("role")
                        .and_then(Value::as_str)
                        .map(|role| role.eq_ignore_ascii_case("effect"))
                        .unwrap_or(false),
                    bridge_sink: None,
                    params: insert.params.clone(),
                    dsp: InsertDspState::new(
                        canonical_plugin_id(&insert.kind),
                        &insert.params,
                        output_sample_rate,
                    ),
                    vst3,
                    callback_process_log_done: false,
                    silent_process_blocks: 0,
                    bridge_missed_blocks: 0,
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
                output_track_index: None, // resolved below in resolve_indices
                inserts,
                sends: t
                    .sends
                    .iter()
                    .map(|send| RuntimeSend {
                        id: send.id.clone(),
                        return_track_id: send.return_track_id.clone(),
                        return_track_index: None, // resolved below in resolve_indices
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
        let tempo_map = build_project_tempo_map(snapshot);
        let (midi_clips, midi_tracks) =
            build_midi_runtime(&snapshot.midi_clips, &tempo_map, output_sample_rate);
        let samples_per_beat = if snapshot.bpm > 0.0 {
            output_sample_rate as f64 * 60.0 / snapshot.bpm
        } else {
            0.0
        };

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
                    |beat| tempo_map.samples_at_beat(beat, output_sample_rate as f64),
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

        let mut project = Self {
            sample_rate: output_sample_rate,
            tracks,
            clips,
            has_solo,
            tempo_map,
            midi_clips,
            midi_tracks,
            audio_graph,
            latency_graph,
            pdc_enabled: pdc_active,
            // Installed by the control thread after build; never carried in a
            // freshly built project (preserved across reloads in drain_commands).
            plugin_bridge_sinks: std::collections::HashMap::new(),
            bridge_editor_active: std::collections::HashSet::new(),
            bridge_panic_flush_samples: 0,
        };
        // Resolve cross-entity indices once, on this worker thread, so the
        // audio callback never does an id lookup per block.
        project.resolve_indices();
        Ok(project)
    }

    /// Reposition every MIDI track's cursor to the first event at/after
    /// `position_sample` and clear active notes (emitting note-offs so the
    /// destination never gets a stuck note). Called on seek / play-from.
    pub fn reset_midi_playback(&mut self, position_sample: u64) {
        self.reset_midi_playback_with_offset(position_sample, 0);
    }

    /// Like [`Self::reset_midi_playback`], but note-off panic events are placed
    /// at `sample_offset` within the current callback block. Used when the
    /// render kernel wraps a loop in the middle of a device block.
    pub fn reset_midi_playback_with_offset(&mut self, position_sample: u64, sample_offset: u32) {
        self.all_notes_off_with_offset("seek/play", sample_offset);
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

    /// Rebuild MIDI event sample positions from canonical beat positions after a
    /// tempo-map change. Returns the sample position that preserves the current
    /// musical playhead beat under the new map.
    pub fn apply_tempo_map(
        &mut self,
        tempo_map: RuntimeTempoMapSnapshot,
        position_sample: u64,
    ) -> u64 {
        let sr = self.sample_rate.max(1) as f64;
        let current_beat = self.tempo_map.beat_at_samples(position_sample, sr);
        self.all_notes_off("tempo_change");
        self.tempo_map = tempo_map;
        for clip in &mut self.midi_clips {
            for event in &mut clip.events {
                event.sample = self.tempo_map.samples_at_beat(event.beat, sr);
            }
            sort_midi_events(&mut clip.events);
        }
        let next_position = self.tempo_map.samples_at_beat(current_beat, sr);
        for mt in &mut self.midi_tracks {
            for event in &mut mt.events {
                event.sample = self.tempo_map.samples_at_beat(event.beat, sr);
            }
            sort_midi_events(&mut mt.events);
            mt.cursor = mt.events.partition_point(|ev| ev.sample < next_position);
            mt.active.clear();
        }
        next_position
    }

    /// Static-tempo shortcut used by legacy `SetBpm` commands.
    pub fn set_static_midi_tempo(&mut self, bpm: f64, position_sample: u64) -> u64 {
        self.apply_tempo_map(RuntimeTempoMapSnapshot::static_tempo(bpm), position_sample)
    }

    /// Emit note-off for all active notes on every MIDI track and clear the
    /// active set. Called on stop/seek to prevent stuck notes.
    pub fn all_notes_off(&mut self, reason: &str) {
        self.all_notes_off_with_offset(reason, 0);
    }

    fn all_notes_off_with_offset(&mut self, reason: &str, sample_offset: u32) {
        let debug = midi_engine_debug_enabled();
        if debug && reason.contains("seek") {
            for mt in &self.midi_tracks {
                if mt.active.is_empty() {
                    continue;
                }
                if let Some(ti) = mt.track_index {
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
        // Runs on the audio thread (stop/seek/mute/solo/graph swap): take each
        // active list out by swap (no clone, capacity preserved) and hand the
        // note-offs to the track's instrument route.
        for mt_ix in 0..self.midi_tracks.len() {
            let mut active = std::mem::take(&mut self.midi_tracks[mt_ix].active);
            if debug {
                eprintln!(
                    "[MidiPanic] track={} reason={} active_notes_cleared={}",
                    self.midi_tracks[mt_ix].track_id,
                    reason,
                    active.len()
                );
            }
            let track_index = self.midi_tracks[mt_ix].track_index;
            push_all_notes_off_for_track(self, track_index, &active, sample_offset);
            active.clear();
            self.midi_tracks[mt_ix].active = active;
            self.midi_tracks[mt_ix].preview_active.clear();
        }
    }

    /// Schedule the MIDI events that fall inside `[base_sample, base_sample +
    /// frames)`. Runs once per audio block from the callback. No heap
    /// allocation on the steady-state path (event lists are preallocated; the
    /// active-note Vec is reserved at build time).
    pub fn schedule_midi_block(&mut self, base_sample: u64, frames: u64) {
        self.schedule_midi_block_with_offset(base_sample, frames, 0);
    }

    pub fn schedule_midi_block_with_offset(
        &mut self,
        base_sample: u64,
        frames: u64,
        callback_offset: u32,
    ) {
        if self.midi_tracks.is_empty() || frames == 0 {
            return;
        }
        let block_end = base_sample.saturating_add(frames);
        let debug = midi_engine_debug_enabled();
        let verbose = crate::forensic_trace::engine_midi_verbose_enabled();
        let trace = crate::forensic_trace::engine_midi_trace_enabled();
        let vst3_debug = vst3_midi_debug_enabled();
        let sr = self.sample_rate.max(1) as f64;
        let heartbeat = trace && crate::forensic_trace::scheduler_heartbeat_due();
        for mt in &mut self.midi_tracks {
            let mut scheduled = 0u32;
            // Instrument route from build-time indices + the cached bridge
            // sink — no id lookups, String clones, or Vec collects per block.
            let track_ix = mt.track_index.filter(|&ti| ti < self.tracks.len());
            let instrument_ix = track_ix.and_then(|ti| self.tracks[ti].midi_instrument_insert_ix);
            let bridge_sink = track_ix.zip(instrument_ix).and_then(|(ti, ix)| {
                self.tracks[ti]
                    .inserts
                    .get(ix)
                    .and_then(|insert| insert.bridge_sink.clone())
            });
            if trace {
                // Trace-only diagnostics. The clip-overlap scan (and its Vec)
                // is allowed here because the flag is off in production.
                let bpm = self
                    .tempo_map
                    .bpm_at_beat(self.tempo_map.beat_at_samples(base_sample, sr));
                let overlapping: Vec<_> = self
                    .midi_clips
                    .iter()
                    .filter(|c| {
                        c.track_id == mt.track_id
                            && block_end > self.tempo_map.samples_at_beat(c.start_beat, sr)
                            && base_sample < self.tempo_map.samples_at_beat(c.end_beat, sr)
                    })
                    .collect();
                let block_has_note = overlapping.iter().any(|c| {
                    c.events.iter().any(|ev| {
                        ev.sample >= base_sample
                            && ev.sample < block_end
                            && matches!(ev.kind, RuntimeMidiEventKind::NoteOn)
                    })
                });
                if block_has_note || heartbeat {
                    eprintln!(
                        "[midi-scheduler] playing=true bpm={bpm:.1} sr={} block_start={base_sample} block_end={block_end}",
                        self.sample_rate
                    );
                    for clip in &overlapping {
                        eprintln!(
                            "[midi-scheduler] track={} clip={} overlaps=true",
                            mt.track_id, clip.id
                        );
                    }
                }
                if bridge_sink.is_some() {
                    if let Some((ti, ix)) = track_ix.zip(instrument_ix) {
                        let instance_id = &self.tracks[ti].inserts[ix].id;
                        eprintln!(
                            "[instrument-route] track={} instrument_instance={}",
                            mt.track_id, instance_id
                        );
                        eprintln!("[instrument-route] plugin_instance_id={instance_id}");
                        eprintln!("[instrument-route] route_ok=true");
                    }
                }
            }
            while mt.cursor < mt.events.len() && mt.events[mt.cursor].sample < block_end {
                let ev = mt.events[mt.cursor].clone();
                mt.cursor += 1;
                if ev.sample < base_sample {
                    apply_active(&mut mt.active, &ev);
                    continue;
                }
                let offset = callback_offset.saturating_add((ev.sample - base_sample) as u32);
                apply_active(&mut mt.active, &ev);
                if let Some(ti) = track_ix {
                    if let Some(ix) = instrument_ix {
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
                        if let Some(sink) = bridge_sink.as_deref() {
                            push_vst3_midi_event_to_sink(
                                sink,
                                &midi_ev,
                                &self.tracks[ti].inserts[ix].id,
                                verbose,
                            );
                            if trace {
                                let abs = ev.sample;
                                let instance_id = &self.tracks[ti].inserts[ix].id;
                                match ev.kind {
                                    RuntimeMidiEventKind::NoteOn => eprintln!(
                                        "[midi-scheduler] note_on pitch={} offset={offset} \
                                         absolute_sample={abs} instance={instance_id}",
                                        ev.pitch
                                    ),
                                    RuntimeMidiEventKind::NoteOff => eprintln!(
                                        "[midi-scheduler] note_off pitch={} offset={offset} \
                                         absolute_sample={abs} instance={instance_id}",
                                        ev.pitch
                                    ),
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
                let bs = self.tempo_map.beat_at_samples(base_sample, sr);
                let be = self.tempo_map.beat_at_samples(block_end, sr);
                eprintln!(
                    "[DAUx MIDI] block beat={:.3}..{:.3} track={} events={} active={}",
                    bs,
                    be,
                    mt.track_id,
                    scheduled,
                    mt.active.len()
                );
                eprintln!(
                    "[DAUx MIDI] block events={} track={}",
                    scheduled, mt.track_id
                );
            }
            if vst3_debug {
                if let Some(ti) = track_ix {
                    if let Some(ix) = instrument_ix {
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

    pub fn midi_preview_control_change(
        &mut self,
        track_id: &str,
        channel: u8,
        controller: u8,
        value: u8,
    ) {
        self.bridge_preview_control_change(track_id, "", channel, controller, value);
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
        let bridged = self.plugin_bridge_sinks.contains_key(plugin_instance_id);
        if bridged {
            // Shared-memory path: always write into the realtime MIDI ring on the
            // audio thread. Do not rely on midi_block_events / runtime inserts.
            self.push_bridge_preview_midi(
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
        if crate::forensic_trace::engine_midi_verbose_enabled() {
            eprintln!(
                "[EngineMidiPreview] received note_off track={} instance={} ch={} pitch={}",
                track_id, plugin_instance_id, channel, pitch
            );
        }
        let bridged = self.plugin_bridge_sinks.contains_key(plugin_instance_id);
        if bridged {
            self.push_bridge_preview_midi(plugin_instance_id, 0x80 | channel, pitch, 0, "note_off");
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

    pub fn bridge_preview_control_change(
        &mut self,
        track_id: &str,
        plugin_instance_id: &str,
        channel: u8,
        controller: u8,
        value: u8,
    ) {
        let channel = channel.min(15);
        let controller = controller.min(127);
        let value = value.min(127);
        if self.plugin_bridge_sinks.contains_key(plugin_instance_id) {
            self.push_bridge_preview_midi(
                plugin_instance_id,
                0xB0 | channel,
                controller,
                value,
                "control_change",
            );
            return;
        }
        let _ = self.queue_preview_event(
            track_id,
            Vst3MidiEvent::control_change(0, channel, controller as u16, value as f32 / 127.0),
            "control_change",
            channel,
            controller,
        );
    }

    pub fn bridge_preview_all_notes_off(&mut self, track_id: &str, plugin_instance_id: &str) {
        let (active, track_index) = self
            .midi_tracks
            .iter()
            .find(|mt| mt.track_id == track_id)
            .map(|mt| (mt.preview_active.clone(), mt.track_index))
            .unwrap_or_default();
        // Command path (not per-block): fall back to an id lookup so the panic
        // CCs still reach a track that never had a MIDI schedule entry.
        let track_index = track_index.or_else(|| self.tracks.iter().position(|t| t.id == track_id));
        if crate::forensic_trace::engine_midi_verbose_enabled() {
            eprintln!(
                "[EngineMidiPreview] received all_notes_off track={} instance={} active_notes={}",
                track_id,
                plugin_instance_id,
                active.len()
            );
        }
        push_all_notes_off_for_track(self, track_index, &active, 0);
        if self.plugin_bridge_sinks.contains_key(plugin_instance_id) {
            if let Some(sink) = self.plugin_bridge_sinks.get(plugin_instance_id) {
                for &(channel, pitch) in &active {
                    sink.push_midi(0x80 | (channel & 0x0F), pitch, 0, 0);
                }
                for ch in 0u8..16 {
                    sink.push_midi(0xB0 | (ch & 0x0F), 64, 0, 0);
                    sink.push_midi(0xB0 | (ch & 0x0F), 123, 0, 0);
                    sink.push_midi(0xB0 | (ch & 0x0F), 120, 0, 0);
                }
            }
            self.arm_bridge_panic_flush();
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
        plugin_instance_id: &str,
        status: u8,
        data1: u8,
        data2: u8,
        kind: &str,
    ) {
        let Some(sink) = self.plugin_bridge_sinks.get(plugin_instance_id) else {
            if midi_verbose_enabled() {
                eprintln!(
                    "[plugin-dsp-midi] write skipped instance={plugin_instance_id} reason=no_bridge_sink keys={:?}",
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
            eprintln!("[plugin-dsp-midi-write] seq={seq} instance={instance} events=1");
        }
    }

    pub fn has_active_midi_preview(&self) -> bool {
        self.midi_tracks
            .iter()
            .any(|mt| !mt.preview_active.is_empty())
    }

    /// Arm the post-panic flush window after panic MIDI was pushed into a
    /// bridge sink. ~250 ms of kept-alive block requests is plenty: the host
    /// drains the ring on the first requested block and CC 120 (All Sound Off)
    /// silences voices immediately; the rest of the window absorbs a host that
    /// is momentarily stalled behind an editor open/close.
    pub fn arm_bridge_panic_flush(&mut self) {
        self.bridge_panic_flush_samples = (self.sample_rate.max(1) as u64) / 4;
    }

    fn queue_preview_event(
        &mut self,
        track_id: &str,
        event: Vst3MidiEvent,
        event_type: &str,
        channel: u8,
        pitch: u8,
    ) -> bool {
        // Runs on the audio thread per preview event — route diagnostics (with
        // their String formatting) only exist under the verbose trace flag.
        let verbose = crate::forensic_trace::engine_midi_verbose_enabled();
        let Some(ti) = self.tracks.iter().position(|t| t.id == track_id) else {
            if verbose {
                eprintln!(
                    "[InstrumentRoute] track={} no instrument plugin found reason=missing_track",
                    track_id
                );
            }
            return false;
        };
        let track = &self.tracks[ti];
        if verbose {
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
        }
        let Some(insert_ix) = track.midi_instrument_insert_ix else {
            if verbose {
                eprintln!(
                    "[InstrumentRoute] track={} selected_instrument_plugin=none no instrument plugin found",
                    track_id
                );
            }
            return false;
        };
        if verbose {
            let plugin_id = &self.tracks[ti].inserts[insert_ix].id;
            eprintln!(
                "[InstrumentRoute] track={} selected_instrument_plugin={}",
                track_id, plugin_id
            );
            eprintln!(
                "[PluginMidiIn] plugin={} {} ch={} pitch={} offset=0",
                plugin_id, event_type, channel, pitch
            );
            eprintln!(
                "[EngineMidiPreview] target plugin={} event queued",
                plugin_id
            );
        }
        self.tracks[ti].midi_block_events.push(event);
        true
    }

    fn set_preview_active(&mut self, track_id: &str, channel: u8, pitch: u8, active: bool) {
        let Some(mt) = self
            .midi_tracks
            .iter_mut()
            .find(|mt| mt.track_id == track_id)
        else {
            let track_index = self.tracks.iter().position(|t| t.id == track_id);
            self.midi_tracks.push(RuntimeMidiTrack {
                track_id: track_id.to_string(),
                track_index,
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

        // Bridged plugin (runs in the host process): forward the numeric VST3
        // ParamID + normalized value to the host through the shared param ring,
        // and persist it in the params map for snapshot/recall. Previously this
        // fell through to the built-in branch and was silently dropped.
        if insert.kind_tag == RuntimeInsertKind::ExternalBridge {
            if let Ok(vst3_param_id) = param_id.parse::<u32>() {
                insert
                    .params
                    .insert(param_id.to_string(), Value::from(value as f64));
                if let Some(sink) = insert.bridge_sink.as_ref() {
                    sink.push_param(vst3_param_id, value.clamp(0.0, 1.0), 0);
                }
            }
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
                    eprintln!("[plugin-dsp-midi-write] seq={seq} instance={instance_id} events=1");
                    eprintln!(
                        "[plugin-dsp-midi-write] note_on pitch={} offset={} ch={channel}",
                        ev.pitch, ev.sample_offset
                    );
                }
            } else {
                sink.push_midi(0x80 | channel, ev.pitch, 0, ev.sample_offset);
                if verbose {
                    eprintln!("[plugin-dsp-midi-write] seq={seq} instance={instance_id} events=1");
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
                eprintln!("[plugin-dsp-midi-write] seq={seq} instance={instance_id} events=1");
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

fn push_all_notes_off_for_track(
    project: &mut RuntimeProject,
    track_index: Option<usize>,
    active: &[(u8, u8)],
    sample_offset: u32,
) {
    let Some(ti) = track_index.filter(|&ti| ti < project.tracks.len()) else {
        return;
    };
    let Some(ix) = project.tracks[ti].midi_instrument_insert_ix else {
        return;
    };
    let sink = project.tracks[ti]
        .inserts
        .get(ix)
        .and_then(|insert| insert.bridge_sink.clone());
    if let Some(sink) = sink {
        if midi_engine_debug_enabled() {
            eprintln!(
                "[midi-playback] transport_stop panic instance={} old_notes={}",
                project.tracks[ti].inserts[ix].id,
                active.len()
            );
        }
        for &(channel, pitch) in active {
            sink.push_midi(0x80 | (channel & 0x0F), pitch, 0, sample_offset);
        }
        for ch in 0u8..16 {
            sink.push_midi(0xB0 | (ch & 0x0F), 64, 0, sample_offset);
            sink.push_midi(0xB0 | (ch & 0x0F), 123, 0, sample_offset);
            sink.push_midi(0xB0 | (ch & 0x0F), 120, 0, sample_offset);
        }
        // The host only drains the ring while blocks are requested — keep the
        // handshake alive past this panic so it is actually delivered.
        project.arm_bridge_panic_flush();
        return;
    }
    for &(channel, pitch) in active {
        project.tracks[ti]
            .midi_block_events
            .push(Vst3MidiEvent::note_off(sample_offset, channel, pitch, 0.0));
    }
    for channel in 0..16 {
        project.tracks[ti]
            .midi_block_events
            .push(Vst3MidiEvent::control_change(
                sample_offset,
                channel,
                64,
                0.0,
            ));
        project.tracks[ti]
            .midi_block_events
            .push(Vst3MidiEvent::control_change(
                sample_offset,
                channel,
                123,
                0.0,
            ));
        project.tracks[ti]
            .midi_block_events
            .push(Vst3MidiEvent::control_change(
                sample_offset,
                channel,
                120,
                0.0,
            ));
        project.tracks[ti]
            .midi_block_events
            .push(Vst3MidiEvent::control_change(
                sample_offset,
                channel,
                121,
                0.0,
            ));
    }
}

pub fn build_tempo_map_from_points(
    default_bpm: f64,
    points: &[crate::types::EngineTempoPointSnapshot],
) -> RuntimeTempoMapSnapshot {
    if points.is_empty() {
        RuntimeTempoMapSnapshot::static_tempo(default_bpm)
    } else {
        TempoMap::from_points(
            default_bpm,
            points
                .iter()
                .map(|p| TempoPoint {
                    beat: p.beat,
                    bpm: p.bpm,
                })
                .collect(),
        )
        .into_snapshot()
    }
}

pub fn build_project_tempo_map(snapshot: &EngineProjectSnapshot) -> RuntimeTempoMapSnapshot {
    build_tempo_map_from_points(snapshot.bpm, &snapshot.tempo_points)
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
    tempo_map: &RuntimeTempoMapSnapshot,
    sample_rate: u32,
) -> (Vec<RuntimeMidiClip>, Vec<RuntimeMidiTrack>) {
    let sr = sample_rate.max(1) as f64;
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
            let on_sample = tempo_map.samples_at_beat(abs_start, sr);
            let off_sample = tempo_map.samples_at_beat(abs_end, sr);
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
                let sample = tempo_map.samples_at_beat(abs_beat, sr);
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
                track_index: None, // resolved by RuntimeProject::resolve_indices
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

/// `FUTUREBOARD_CLIP_DSP_DEBUG=1` enables a one-line-per-clip diagnostic of the
/// resolved stretch DSP path, printed once at graph-build time (never from the
/// audio callback). Cached on first read.
pub fn clip_dsp_debug_enabled() -> bool {
    static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FLAG.get_or_init(|| std::env::var_os("FUTUREBOARD_CLIP_DSP_DEBUG").is_some())
}

/// The clip-stretch DSP path resolved for a clip. `PhaseVocoderBasic` is a
/// basic streaming OLA/granular stretcher today; the enum keeps processor
/// selection explicit and leaves room for a higher-quality phase vocoder without
/// changing snapshot/runtime wiring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipDspProcessor {
    NoStretch,
    Resample,
    PhaseVocoderBasic,
}

/// Resolve the DSP path from the snapshot's `mode` key (see
/// `engine_snapshot::stretch_mode_key`) and `preserve_pitch` flag.
pub fn resolve_clip_processor(mode: &str, preserve_pitch: bool) -> ClipDspProcessor {
    match mode {
        "off" | "none" => ClipDspProcessor::NoStretch,
        "resample" => ClipDspProcessor::Resample,
        "manual" | "temposync" => {
            if preserve_pitch {
                ClipDspProcessor::PhaseVocoderBasic
            } else {
                ClipDspProcessor::Resample
            }
        }
        "warp" => ClipDspProcessor::PhaseVocoderBasic,
        _ => ClipDspProcessor::Resample,
    }
}

fn resolved_clip_stretch_params(clip: &EngineClipSnapshot) -> StretchParams {
    if clip.stretch != StretchParams::default() {
        return clip.stretch.clone();
    }

    clip.audio_process
        .as_ref()
        .map(legacy_process_stretch_params)
        .unwrap_or_default()
}

fn legacy_process_stretch_params(process: &EngineClipAudioProcess) -> StretchParams {
    legacy_audio_process_to_stretch(
        &process.mode,
        process.preserve_pitch,
        process.speed_ratio,
        process.effective_time_ratio,
        process.pitch_semitones,
        &process.quality,
    )
}

fn legacy_audio_process_to_stretch(
    mode: &str,
    preserve_pitch: bool,
    speed_ratio: f64,
    effective_time_ratio: f64,
    pitch_semitones: f64,
    quality: &str,
) -> StretchParams {
    let mut params = StretchParams::default();
    let legacy_time_ratio = if effective_time_ratio.is_finite() && effective_time_ratio > 0.0 {
        effective_time_ratio as f32
    } else if speed_ratio.is_finite() && speed_ratio > 0.0 {
        (1.0 / speed_ratio) as f32
    } else {
        1.0
    };

    let mode_key = mode.to_ascii_lowercase();
    let force_repitch = mode_key == "resample";
    params.mode = match mode_key.as_str() {
        "off" | "none" => StretchMode::Off,
        "temposync" | "tempo_sync" | "tempo-sync" => StretchMode::TempoSync,
        "warp" => StretchMode::Warp,
        "manual" | "resample" => {
            if (legacy_time_ratio - 1.0).abs() > f32::EPSILON || preserve_pitch || force_repitch {
                StretchMode::Manual
            } else {
                StretchMode::Off
            }
        }
        _ => {
            if (legacy_time_ratio - 1.0).abs() > f32::EPSILON || preserve_pitch {
                StretchMode::Manual
            } else {
                StretchMode::Off
            }
        }
    };
    params.algorithm = if params.mode == StretchMode::Off {
        StretchAlgorithm::Off
    } else if preserve_pitch && !force_repitch {
        StretchAlgorithm::PreservePitch
    } else {
        StretchAlgorithm::RePitch
    };
    params.time_ratio = legacy_time_ratio;
    params.pitch_ratio = if pitch_semitones.is_finite() {
        2.0_f32.powf(pitch_semitones as f32 / 12.0)
    } else {
        1.0
    };
    params.preserve_pitch = preserve_pitch && !force_repitch && params.mode != StretchMode::Off;
    params.quality = match quality {
        "draft" => 0.35,
        "high" => 1.0,
        _ => 0.75,
    };
    params
}

pub fn resolve_clip_processor_from_stretch(params: &StretchParams) -> ClipDspProcessor {
    if params.mode == StretchMode::Off || params.algorithm == StretchAlgorithm::Off {
        return ClipDspProcessor::NoStretch;
    }
    match resolve_backend(params) {
        StretchBackend::InternalRePitch => ClipDspProcessor::Resample,
        StretchBackend::Signalsmith => ClipDspProcessor::PhaseVocoderBasic,
    }
}

#[cfg(test)]
mod stretch_runtime_tests {
    use super::*;
    use crate::audio_file::AudioFileBuffer;
    use crate::types::EngineFadeSnapshot;

    fn test_source(frames: u64) -> Arc<ClipAudioSource> {
        Arc::new(ClipAudioSource::InMemory(Arc::new(AudioFileBuffer {
            sample_rate: 48_000,
            channels: 2,
            frames: frames as usize,
            samples: vec![0.0; frames as usize * 2],
        })))
    }

    fn test_clip(stretch: StretchParams) -> EngineClipSnapshot {
        EngineClipSnapshot {
            id: "clip".to_string(),
            track_id: "track".to_string(),
            asset_id: "asset".to_string(),
            media_path: Some("test.wav".to_string()),
            start_beat: 0.0,
            duration_beats: 1.0,
            offset_seconds: 0.0,
            gain: 1.0,
            muted: false,
            fades: Some(EngineFadeSnapshot {
                in_duration: 0.0,
                out_duration: 0.0,
                in_curve: "linear".to_string(),
                out_curve: "linear".to_string(),
            }),
            stretch,
            audio_process: None,
        }
    }

    #[test]
    fn engine_clip_stretch_serializes_roundtrip() {
        let stretch = StretchParams {
            mode: StretchMode::Manual,
            algorithm: StretchAlgorithm::PreservePitch,
            time_ratio: 2.0,
            pitch_ratio: 1.25,
            preserve_pitch: true,
            ..StretchParams::default()
        };
        let clip = test_clip(stretch.clone());
        let json = serde_json::to_string(&clip).expect("serialize clip");
        let loaded: EngineClipSnapshot = serde_json::from_str(&json).expect("deserialize clip");
        assert_eq!(loaded.stretch, stretch);
    }

    #[test]
    fn missing_stretch_defaults_to_off() {
        let json = r#"{
            "id":"clip","trackId":"track","assetId":"asset","mediaPath":"test.wav",
            "startBeat":0.0,"durationBeats":1.0,"offsetSeconds":0.0,"gain":1.0
        }"#;
        let loaded: EngineClipSnapshot =
            serde_json::from_str(json).expect("deserialize legacy clip");
        assert_eq!(loaded.stretch, StretchParams::default());
        assert_eq!(
            resolved_clip_stretch_params(&loaded),
            StretchParams::default()
        );
    }

    #[test]
    fn legacy_audio_process_migrates_to_stretch_params() {
        let mut clip = test_clip(StretchParams::default());
        clip.audio_process = Some(EngineClipAudioProcess {
            speed_ratio: 0.5,
            effective_time_ratio: 2.0,
            pitch_ratio: 1.0,
            pitch_semitones: 0.0,
            preserve_pitch: true,
            mode: "manual".to_string(),
            quality: "balanced".to_string(),
            source_start_samples: 0,
            source_end_samples: 48_000,
            warp_markers: Vec::new(),
            reverse: false,
        });
        let migrated = resolved_clip_stretch_params(&clip);
        assert_eq!(migrated.mode, StretchMode::Manual);
        assert_eq!(migrated.algorithm, StretchAlgorithm::PreservePitch);
        assert!(migrated.preserve_pitch);
        assert!((migrated.time_ratio - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn build_runtime_uses_stretched_duration_samples() {
        let stretch = StretchParams {
            mode: StretchMode::Manual,
            algorithm: StretchAlgorithm::RePitch,
            time_ratio: 2.0,
            preserve_pitch: false,
            ..StretchParams::default()
        };
        let clip = test_clip(stretch.clone());
        let runtime_clip =
            build_clip_runtime(&clip, test_source(48_000), 2.0, 48_000).expect("runtime clip");
        assert_eq!(runtime_clip.duration_samples, 96_000);
        assert_eq!(runtime_clip.stretch, stretch);
        assert!((runtime_clip.source_read_rate - 0.5).abs() < f32::EPSILON);

        let stretch = StretchParams {
            time_ratio: 0.5,
            ..stretch
        };
        let clip = test_clip(stretch);
        let runtime_clip =
            build_clip_runtime(&clip, test_source(48_000), 2.0, 48_000).expect("runtime clip");
        assert_eq!(runtime_clip.duration_samples, 24_000);
        assert!((runtime_clip.source_read_rate - 2.0).abs() < f32::EPSILON);
    }
}

pub fn describe_clip_dsp_state(
    clip: &AudioClip,
    process: &EngineClipAudioProcess,
    project_bpm: f64,
) -> String {
    let stretch = legacy_process_stretch_params(process);
    let processor = resolve_clip_processor_from_stretch(&stretch);
    let pending = if matches!(processor, ClipDspProcessor::PhaseVocoderBasic)
        && process.pitch_semitones.abs() > f64::EPSILON
    {
        " pitch_shift=pending"
    } else {
        ""
    };
    let duration_samples = process
        .source_end_samples
        .saturating_sub(process.source_start_samples) as f64
        * process.effective_time_ratio.max(0.0);
    format!(
        "Clip DSP Snapshot: clip_id={} name={} mode={} ratio={:.6} percent={:.2} algorithm={} effective_time_ratio={:.6} pitch_ratio={:.6} speed_ratio={:.6} preserve_pitch={} reverse={} duration_samples={} source_start={} source_end={} processor={:?}{} warp_markers={} project_bpm={:.3}",
        clip.id,
        clip.asset_id,
        process.mode,
        effective_time_ratio(&stretch, Some(project_bpm as f32)),
        effective_time_ratio(&stretch, Some(project_bpm as f32)) * 100.0,
        process.quality,
        effective_time_ratio(&stretch, Some(project_bpm as f32)),
        effective_pitch_ratio(&stretch),
        source_read_rate_for_repitch(&stretch, Some(project_bpm as f32)),
        process.preserve_pitch,
        process.reverse,
        duration_samples.round() as u64,
        process.source_start_samples,
        process.source_end_samples,
        processor,
        pending,
        process.warp_markers.len(),
        project_bpm,
    )
}

/// Switch for routing preserve-pitch clips through the real Signalsmith backend
/// in the realtime render. **Default-on**: the Signalsmith default preset reports
/// ~5760 samples (≈120 ms @ 48 kHz) of algorithmic latency, which is now
/// compensated per-clip via `output_seek` pre-roll priming in
/// `render_signalsmith_clip_segment` (the next `process` output is aligned to the
/// playback position on every (re)start), so stretched clips stay in sync without
/// the crude zero-latency `PhaseVocoderBasic` fallback. Set
/// `FUTUREBOARD_STRETCH_SIGNALSMITH=0` to force the fallback for A/B comparison.
fn signalsmith_enabled() -> bool {
    static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FLAG.get_or_init(|| match std::env::var("FUTUREBOARD_STRETCH_SIGNALSMITH") {
        Ok(value) => {
            let v = value.trim();
            !(v == "0" || v.eq_ignore_ascii_case("false") || v.eq_ignore_ascii_case("off"))
        }
        Err(_) => true,
    })
}

/// Build the per-clip preserve-pitch stretch processor for the realtime render.
///
/// Only the Signalsmith backend uses a cached `StretchProcessor`; resample /
/// no-stretch clips are sampled inline. The bridge is now an allocation-free
/// pass-through (`render_signalsmith_clip_segment` feeds it exactly the source
/// samples it consumes per block), so it is safe for the audio callback. Created
/// on the control thread; the audio thread only calls `reset`/`process_stereo`.
fn create_runtime_stretch_processor(
    backend: StretchBackend,
    sample_rate: u32,
    stretch: &StretchParams,
) -> Option<Box<dyn StretchProcessor + Send>> {
    if backend != StretchBackend::Signalsmith || !signalsmith_enabled() {
        return None;
    }
    match create_stretch_processor(backend, sample_rate as f32, 2, stretch.clone()) {
        Ok(processor) => {
            if std::env::var_os("FUTUREBOARD_AUDIO_DEBUG").is_some() {
                eprintln!(
                    "[clip-stretch] signalsmith processor created sample_rate={sample_rate} latency_samples={} time_ratio={:.4} pitch_ratio={:.4}",
                    processor.latency_samples(),
                    effective_time_ratio(stretch, None),
                    effective_pitch_ratio(stretch),
                );
            }
            Some(processor)
        }
        Err(err) => {
            static WARNED: AtomicBool = AtomicBool::new(false);
            if !WARNED.swap(true, Ordering::Relaxed) {
                eprintln!("[clip-stretch] signalsmith processor unavailable, using fallback: {err}");
            }
            None
        }
    }
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

    let project_bpm = Some((beats_per_second * 60.0) as f32);
    let stretch = resolved_clip_stretch_params(clip);
    let speed_ratio = source_read_rate_for_repitch(&stretch, project_bpm).clamp(0.01, 16.0);
    let source_read_rate = speed_ratio;
    let effective_time_ratio = effective_time_ratio(&stretch, project_bpm).clamp(0.01, 20.0);
    let pitch_ratio = effective_pitch_ratio(&stretch).clamp(0.01, 16.0);
    let mut stretch_backend = resolve_backend(&stretch);
    if stretch_backend == StretchBackend::Signalsmith
        && !SphereAudioProcessor::signalsmith_stretch_available()
    {
        static WARNED_SIGNALS_MISSING: AtomicBool = AtomicBool::new(false);
        if !WARNED_SIGNALS_MISSING.swap(true, Ordering::Relaxed) {
            eprintln!(
                "Signalsmith Stretch unavailable; falling back to InternalRePitch for clip {}",
                clip.id
            );
        }
        stretch_backend = StretchBackend::InternalRePitch;
    }
    let processor = match stretch_backend {
        StretchBackend::InternalRePitch => {
            if stretch.mode == StretchMode::Off || stretch.algorithm == StretchAlgorithm::Off {
                ClipDspProcessor::NoStretch
            } else {
                ClipDspProcessor::Resample
            }
        }
        StretchBackend::Signalsmith => ClipDspProcessor::PhaseVocoderBasic,
    };
    let reverse = clip
        .audio_process
        .as_ref()
        .map(|p| p.reverse)
        .unwrap_or(false);
    let source_start_samples = clip
        .audio_process
        .as_ref()
        .map(|p| p.source_start_samples)
        .unwrap_or(0);
    let source_end_samples = clip
        .audio_process
        .as_ref()
        .map(|p| p.source_end_samples)
        .unwrap_or(0);
    let mut warp_markers: Vec<RuntimeWarpMarker> = clip
        .audio_process
        .as_ref()
        .map(|p| {
            p.warp_markers
                .iter()
                .map(|m| RuntimeWarpMarker {
                    id: m.id,
                    source_sample: m.source_sample,
                    timeline_beat: m.timeline_beat,
                    locked: m.locked,
                })
                .collect()
        })
        .unwrap_or_default();
    warp_markers.sort_by(|a, b| a.timeline_beat.total_cmp(&b.timeline_beat));

    // One-time, control-thread-only diagnostic of the resolved clip DSP path
    // (never logs from the audio callback). Gated behind a debug flag.
    if clip_dsp_debug_enabled() {
        if let Some(p) = clip.audio_process.as_ref() {
            eprintln!(
                "[clip-dsp] {}",
                describe_clip_dsp_state(clip, p, beats_per_second * 60.0)
            );
        }
    }

    let base_duration_samples = seconds_to_samples(duration_seconds, output_sample_rate).max(1);
    let stretch_is_authoritative =
        clip.audio_process.is_some() || clip.stretch != StretchParams::default();
    let duration_samples = if stretch_is_authoritative {
        let trim_start = source_start_samples.min(source.frames() as u64);
        let trim_end = if source_end_samples > trim_start {
            source_end_samples.min(source.frames() as u64)
        } else {
            source.frames() as u64
        };
        let trimmed_source_frames = trim_end.saturating_sub(trim_start).max(1);
        let source_frames_at_output_rate = ((trimmed_source_frames as f64
            / source.sample_rate().max(1) as f64)
            * output_sample_rate.max(1) as f64)
            .round()
            .max(1.0) as u64;
        stretched_duration_samples(source_frames_at_output_rate, &stretch, project_bpm).max(1)
    } else {
        base_duration_samples
    };

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

    let stretch_processor =
        create_runtime_stretch_processor(stretch_backend, source.sample_rate(), &stretch);

    // Preallocate the latency-priming pre-roll buffer on the control thread so
    // the audio thread never grows it on first use. Sized for this clip's
    // playback rate (`1 / time_ratio` input-per-output); `0` for zero-latency
    // backends / no processor.
    let stretch_prime_len = stretch_processor
        .as_ref()
        .map(|p| p.seek_input_len(1.0 / effective_time_ratio.max(0.01)))
        .unwrap_or(0);

    Some(RuntimeClip {
        id: clip.id.clone(),
        track_id: clip.track_id.clone(),
        track_index: None, // resolved by RuntimeProject::resolve_indices
        start_sample: seconds_to_samples(start_seconds.max(0.0), output_sample_rate),
        duration_samples,
        offset_seconds: clip.offset_seconds.max(0.0),
        gain: clip.gain.clamp(0.0, 4.0),
        stretch,
        speed_ratio,
        source_read_rate,
        effective_time_ratio,
        pitch_ratio,
        stretch_backend,
        source_start_samples,
        source_end_samples,
        warp_markers,
        processor,
        reverse,
        muted: clip.muted,
        fade_in_samples,
        fade_out_samples,
        source,
        stretch_processor,
        stretch_input_l: vec![0.0; DEFAULT_AUDIO_BLOCK_CAPACITY],
        stretch_input_r: vec![0.0; DEFAULT_AUDIO_BLOCK_CAPACITY],
        stretch_output_l: vec![0.0; DEFAULT_AUDIO_BLOCK_CAPACITY],
        stretch_output_r: vec![0.0; DEFAULT_AUDIO_BLOCK_CAPACITY],
        stretch_prime_l: vec![0.0; stretch_prime_len],
        stretch_prime_r: vec![0.0; stretch_prime_len],
        stretch_next_project_sample: None,
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
        let tempo_map = RuntimeTempoMapSnapshot::static_tempo(120.0);
        let (midi_clips, midi_tracks) = build_midi_runtime(&clips, &tempo_map, 48_000);
        RuntimeProject {
            sample_rate: 48_000,
            tempo_map,
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

    // ── VSTi bridge MIDI tests ───────────────────────────────────────────────

    /// Test sink recording every `push_midi` call as (status, data1, data2, offset).
    #[derive(Debug, Default)]
    struct RecordingSink {
        events: std::sync::Mutex<Vec<(u8, u8, u8, u32)>>,
    }

    impl RecordingSink {
        fn take(&self) -> Vec<(u8, u8, u8, u32)> {
            std::mem::take(&mut *self.events.lock().unwrap())
        }
    }

    impl crate::plugin_bridge::PluginBridgeSink for RecordingSink {
        fn dsp_ready(&self) -> bool {
            true
        }
        fn read_output(&self, _out_l: &mut [f32], _out_r: &mut [f32], _frames: usize) -> usize {
            0
        }
        fn push_midi(&self, status: u8, data1: u8, data2: u8, sample_offset: u32) {
            self.events
                .lock()
                .unwrap()
                .push((status, data1, data2, sample_offset));
        }
        fn write_input(&self, _in_l: &[f32], _in_r: &[f32], _frames: usize) {}
        fn request_block(&self, _frames: u32) {}
    }

    fn bridged_instrument_track(id: &str) -> RuntimeTrack {
        RuntimeTrack {
            id: id.to_string(),
            track_type: "midi".to_string(),
            volume: 1.0,
            pan: 0.0,
            muted: false,
            solo: false,
            record_armed: false,
            monitor_enabled: false,
            input_source: RuntimeTrackInputSource::None,
            preview_mode: RuntimePreviewMode::Stereo,
            output_track_id: None,
            output_track_index: None,
            inserts: vec![RuntimeInsert {
                id: "insert-1".to_string(),
                kind: "external-bridge-plugin".to_string(),
                kind_tag: RuntimeInsertKind::ExternalBridge,
                enabled: true,
                params: HashMap::new(),
                bridge_is_effect: false,
                bridge_sink: None,
                dsp: InsertDspState::default(),
                vst3: None,
                callback_process_log_done: false,
                silent_process_blocks: 0,
                bridge_missed_blocks: 0,
                scratch_l: vec![0.0; 64],
                scratch_r: vec![0.0; 64],
            }],
            sends: Vec::new(),
            automation_lanes: Vec::new(),
            meter: Arc::new(Default::default()),
            meter_peak_l: 0.0,
            meter_peak_r: 0.0,
            meter_sum_sq_l: 0.0,
            meter_sum_sq_r: 0.0,
            callback_insert_log_done: false,
            callback_clip_route_log_done: false,
            block_l: vec![0.0; 64],
            block_r: vec![0.0; 64],
            recv_l: vec![0.0; 64],
            recv_r: vec![0.0; 64],
            midi_block_events: Vec::new(),
            midi_instrument_insert_ix: Some(0),
            plugin_latency_samples: 0,
            pdc_delay_l: Vec::new(),
            pdc_delay_r: Vec::new(),
            pdc_write_pos: 0,
        }
    }

    /// Project with the one-note clip on a bridged instrument track plus a
    /// recording sink installed as its plugin-bridge sink.
    fn bridged_project() -> (RuntimeProject, Arc<RecordingSink>) {
        let mut p = project_with(vec![clip_with_one_note()]);
        p.tracks.push(bridged_instrument_track("track-1"));
        let sink = Arc::new(RecordingSink::default());
        p.plugin_bridge_sinks
            .insert("insert-1".to_string(), sink.clone());
        // Mirror the engine: indices + cached bridge sinks are resolved before
        // the block path runs.
        p.resolve_indices();
        (p, sink)
    }

    #[test]
    fn scheduled_bridge_events_carry_offset_velocity_and_channel() {
        let (mut p, sink) = bridged_project();
        p.reset_midi_playback(95_880);
        sink.take(); // discard the seek panic CCs

        // NoteOn at absolute sample 96_000 inside block 95_880..96_392.
        p.schedule_midi_block(95_880, 512);
        assert_eq!(sink.take(), vec![(0x90, 60, 100, 120)]);

        // NoteOff at 120_000 inside block 119_900..120_412.
        p.schedule_midi_block(119_900, 512);
        assert_eq!(sink.take(), vec![(0x80, 60, 0, 100)]);
    }

    #[test]
    fn loop_wrap_bridge_events_keep_callback_offset() {
        let (mut p, sink) = bridged_project();
        p.reset_midi_playback(119_900);
        sink.take(); // discard the seek panic CCs

        let end_reset = crate::engine::schedule_midi_render_block(
            &mut p,
            119_900,
            300,
            Some(crate::transport::LoopBounds {
                start: 96_000,
                end: 120_000,
            }),
        );

        assert!(end_reset.is_none());
        let events = sink.take();
        assert!(
            events.contains(&(0x90, 60, 100, 100)),
            "wrapped NoteOn should land 100 samples into the callback: {events:?}"
        );
    }

    #[test]
    fn stop_panic_pushes_note_offs_and_ccs_and_arms_bridge_flush() {
        let (mut p, sink) = bridged_project();
        p.reset_midi_playback(0);
        p.schedule_midi_block(96_000, 512); // fires the NoteOn
        assert_eq!(p.midi_tracks[0].active, vec![(0u8, 60u8)]);
        sink.take();

        p.all_notes_off("stop");

        let events = sink.take();
        // The tracked active note is released explicitly, first.
        assert_eq!(events[0], (0x80, 60, 0, 0));
        // Then Sustain Off / All Notes Off / All Sound Off on every channel.
        for ch in 0u8..16 {
            assert!(
                events.contains(&(0xB0 | ch, 64, 0, 0)),
                "sustain off ch={ch}"
            );
            assert!(
                events.contains(&(0xB0 | ch, 123, 0, 0)),
                "all notes off ch={ch}"
            );
            assert!(
                events.contains(&(0xB0 | ch, 120, 0, 0)),
                "all sound off ch={ch}"
            );
        }
        assert!(p.midi_tracks[0].active.is_empty());
        // The callback must keep requesting bridge blocks so the host actually
        // drains this panic while the transport is stopped.
        assert!(p.bridge_panic_flush_samples > 0);
    }

    #[test]
    fn repeated_bridge_preview_cycle_leaves_no_stuck_notes() {
        let (mut p, sink) = bridged_project();
        for _ in 0..2 {
            p.bridge_preview_note_on("track-1", "insert-1", 0, 64, 110);
            assert!(p.has_active_midi_preview());
            p.bridge_preview_note_off("track-1", "insert-1", 0, 64);
            assert!(!p.has_active_midi_preview());
        }
        assert_eq!(
            sink.take(),
            vec![
                (0x90, 64, 110, 0),
                (0x80, 64, 0, 0),
                (0x90, 64, 110, 0),
                (0x80, 64, 0, 0),
            ]
        );
    }

    #[test]
    fn preview_all_notes_off_releases_held_notes_and_arms_flush() {
        let (mut p, sink) = bridged_project();
        p.bridge_preview_note_on("track-1", "insert-1", 0, 72, 90);
        sink.take();

        p.bridge_preview_all_notes_off("track-1", "insert-1");

        let events = sink.take();
        assert_eq!(events[0], (0x80, 72, 0, 0));
        assert!(events.contains(&(0xB0, 123, 0, 0)));
        assert!(!p.has_active_midi_preview());
        assert!(p.bridge_panic_flush_samples > 0);
    }
}
