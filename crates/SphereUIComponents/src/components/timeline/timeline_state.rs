/// `FUTUREBOARD_PLUGIN_DEBUG=1` enables eprintln traces for insert
/// mutations. Cached on first read.
fn plugin_debug_enabled() -> bool {
    static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FLAG.get_or_init(|| std::env::var_os("FUTUREBOARD_PLUGIN_DEBUG").is_some())
}

/// `FUTUREBOARD_ROUTING_DEBUG=1` enables eprintln traces for send/routing
/// mutations (mirrors the DAUx-side flag). Cached on first read.
fn routing_debug_enabled() -> bool {
    static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FLAG.get_or_init(|| std::env::var_os("FUTUREBOARD_ROUTING_DEBUG").is_some())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackType {
    Audio,
    Midi,
    Instrument,
    /// Sub-mix bus — other tracks route their output here for grouped
    /// processing before the master. Phase 3.
    Bus,
    /// FX return — receives sends from other tracks (aux/reverb returns).
    /// Phase 3.
    Return,
    Master,
}

impl TrackType {
    /// `true` for routing tracks (Bus/Return) that receive audio from other
    /// tracks rather than hosting clips directly.
    pub fn is_routing(self) -> bool {
        matches!(self, TrackType::Bus | TrackType::Return)
    }
}

/// `FUTUREBOARD_MIDI_DEBUG=1` enables eprintln traces for MIDI clip/note
/// mutations (mirrors the plugin/routing debug flags). Cached on first read.
pub fn midi_debug_enabled() -> bool {
    static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FLAG.get_or_init(|| std::env::var_os("FUTUREBOARD_MIDI_DEBUG").is_some())
}

/// Smallest allowed note length, in beats (1/32 note). Mirrors the WebUI
/// `MIN_DUR` guard so a note can never collapse to zero width.
pub const MIN_NOTE_BEATS: f32 = 1.0 / 32.0;

/// Monotonic source of transient note identities. Note ids are NOT persisted —
/// they exist only so the piano-roll editor can track selection / drag targets
/// across edits. Fresh ids are minted on create and on project load.
fn next_midi_note_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

#[derive(Debug, Clone, PartialEq)]
pub struct MidiNoteState {
    /// Transient identity (not serialized). Used by the piano-roll editor to
    /// track selection and in-flight drag targets.
    pub id: u64,
    pub pitch: u8,
    pub start: f32,    // beats relative to clip start
    pub duration: f32, // beats
    /// MIDI velocity in 1..=127.
    pub velocity: u8,
}

impl MidiNoteState {
    /// Construct a note with a freshly minted transient id. `pitch` is clamped
    /// to 0..=127, `velocity` to 1..=127, and `duration` to at least
    /// [`MIN_NOTE_BEATS`].
    pub fn new(pitch: u8, start: f32, duration: f32, velocity: u8) -> Self {
        Self {
            id: next_midi_note_id(),
            pitch: pitch.min(127),
            start: start.max(0.0),
            duration: duration.max(MIN_NOTE_BEATS),
            velocity: velocity.clamp(1, 127),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClipType {
    Audio {
        file_id: String,
        /// Absolute path to the decoded source file, if this clip was created
        /// by importing a real audio file. Used as the waveform cache key.
        source_path: Option<String>,
    },
    Midi {
        notes: Vec<MidiNoteState>,
    },
}

/// Background import/decode state for a real audio file (waveform + engine).
#[derive(Debug, Clone, PartialEq)]
pub enum AudioImportState {
    Pending,
    Probing,
    Decoding { progress: f32 },
    GeneratingPeaks { progress: f32 },
    Ready,
    Failed { message: String },
}

impl Default for AudioImportState {
    fn default() -> Self {
        Self::Pending
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClipState {
    pub id: String,
    pub name: String,
    pub start_beat: f32,
    pub duration_beats: f32,
    pub source_duration_seconds: Option<f64>,
    pub offset_beats: f32,
    pub gain: f32,
    pub clip_type: ClipType,
    pub muted: bool,
    /// Populated for imported audio clips; drives clip chrome + waveform UI.
    pub audio_import: AudioImportState,
}

#[derive(Debug, Clone)]
pub struct ClipDragItem {
    pub clip_id: String,
    pub source_track_id: String,
    pub start_beat: f32,
}

#[derive(Debug, Clone)]
pub struct TrackDragItem {
    pub track_id: String,
    pub origin_index: usize,
    pub name: String,
    pub color: gpui::Rgba,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AutomationPoint {
    pub beat: f32,
    pub value: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AutomationLaneState {
    pub id: String,
    pub name: String,
    pub visible: bool,
    pub points: Vec<AutomationPoint>,
}

/// Plugin format identifier mirrored from `project::PluginFormat`. Kept
/// in the UI state so we can render an icon/badge without depending on
/// the project crate from render code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertPluginFormat {
    Vst3,
    Clap,
    Au,
    Lv2,
    Unknown,
}

impl InsertPluginFormat {
    pub fn label(self) -> &'static str {
        match self {
            InsertPluginFormat::Vst3 => "VST3",
            InsertPluginFormat::Clap => "CLAP",
            InsertPluginFormat::Au => "AU",
            InsertPluginFormat::Lv2 => "LV2",
            InsertPluginFormat::Unknown => "?",
        }
    }
}

/// Load progress of an insert slot. Drives the chip color / label.
/// `Loading` is reserved for Phase 2 when actual plugin instantiation
/// runs on a worker thread. Phase 1 transitions Empty → Ready directly
/// because the runtime doesn't yet instantiate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsertLoadStatus {
    Empty,
    Loading,
    Ready,
    Failed(String),
    Disabled,
}

impl Default for InsertLoadStatus {
    fn default() -> Self {
        InsertLoadStatus::Empty
    }
}

/// Read-only parameter snapshot — populated in Phase 5 by the param
/// event drain pump. Phase 1 keeps the vec empty.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginParameterState {
    pub id: u32,
    pub name: String,
    pub value_normalized: f32,
}

/// UI-side mirror of `project::ProjectInsert`. The runtime owns the
/// actual plugin processor; this struct only stores descriptor +
/// transient UI state (bypass, load status, last-seen parameters).
#[derive(Debug, Clone, PartialEq)]
pub struct InsertSlotState {
    pub id: String,
    /// Stable plugin identifier (`plugin_uid` / classId) — primary key
    /// against the plugin registry. `None` while the slot is empty.
    pub plugin_id: Option<String>,
    pub plugin_path: Option<std::path::PathBuf>,
    pub plugin_format: Option<InsertPluginFormat>,
    /// Display label shown on the mixer strip. "Empty" when no plugin
    /// is loaded; the plugin's `display_name` otherwise.
    pub display_name: String,
    pub enabled: bool,
    pub bypassed: bool,
    pub load_status: InsertLoadStatus,
    pub parameters: Vec<PluginParameterState>,
}

impl InsertSlotState {
    pub fn empty(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            plugin_id: None,
            plugin_path: None,
            plugin_format: None,
            display_name: "Empty".to_string(),
            enabled: true,
            bypassed: false,
            load_status: InsertLoadStatus::Empty,
            parameters: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.plugin_id.is_none()
    }
}

/// A single aux send from this track to a Bus/Return track (Phase 3). The
/// runtime sums `gain_db`-scaled signal into the target's input. UI stores the
/// descriptor; DAUx owns the realtime accumulation.
#[derive(Debug, Clone, PartialEq)]
pub struct SendSlotState {
    pub id: String,
    /// Id of the destination Bus/Return track.
    pub target_track_id: String,
    /// Display label for the destination (resolved at edit time; refreshed
    /// from the track list on render).
    pub target_name: String,
    pub enabled: bool,
    /// `true` = tap before the source track fader; `false` = post-fader.
    /// Realtime currently honours post-fader only (pre-fader is a refinement).
    pub pre_fader: bool,
    pub gain_db: f32,
}

impl SendSlotState {
    /// Linear send gain from `gain_db` (clamped to a sane range).
    pub fn gain_linear(&self) -> f32 {
        10f32.powf(self.gain_db.clamp(-60.0, 6.0) / 20.0)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrackState {
    pub id: String,
    pub name: String,
    pub track_type: TrackType,
    pub color: gpui::Rgba,
    /// Normalized fader position in `0.0..=1.0`. `1.0` is the top of the fader
    /// (≈ +6 dB) and `0.0` is the bottom (≈ -60 dB). See `Volume::norm_to_db`.
    pub volume: f32,
    /// Pan position in `-1.0..=1.0`. `-1.0` is hard left, `+1.0` is hard right.
    pub pan: f32,
    pub muted: bool,
    pub solo: bool,
    pub armed: bool,
    /// Whether the track is monitoring its input. UI-only for now (no audio).
    pub input_monitor: bool,
    /// Latest peak meter levels in `0.0..=1.0`. Currently a static placeholder
    /// per track; will be driven by the audio engine when that lands.
    pub meter_level_l: f32,
    pub meter_level_r: f32,
    pub clips: Vec<ClipState>,
    pub automation_lanes: Vec<AutomationLaneState>,
    /// Insert (effect) plugin chain — ordered. Audio flows through these
    /// in order before volume/pan/sends in the runtime. The UI stores
    /// only descriptor + transient state; the runtime owns the actual
    /// plugin processor.
    pub inserts: Vec<InsertSlotState>,
    /// Aux sends to Bus/Return tracks (Phase 3). Empty for most tracks.
    pub sends: Vec<SendSlotState>,
}

#[derive(Debug, Clone)]
pub struct CreateTrackOptions {
    pub track_type: TrackType,
    pub name: String,
    pub color: gpui::Rgba,
    pub volume: f32,
    pub pan: f32,
    pub armed: bool,
    pub input_monitor: bool,
}

/// Volume / dB mapping helpers. Linear in dB between the soft floor and a
/// little headroom above unity.
pub mod volume {
    pub const MIN_DB: f32 = -60.0;
    pub const MAX_DB: f32 = 6.0;

    pub fn norm_to_db(norm: f32) -> f32 {
        let n = norm.clamp(0.0, 1.0);
        MIN_DB + n * (MAX_DB - MIN_DB)
    }

    pub fn db_to_norm(db: f32) -> f32 {
        ((db - MIN_DB) / (MAX_DB - MIN_DB)).clamp(0.0, 1.0)
    }

    pub fn format_db(norm: f32) -> String {
        let db = norm_to_db(norm);
        if norm <= 0.001 || db <= MIN_DB + 0.05 {
            "-∞".to_string()
        } else if db >= 0.0 {
            format!("+{:.1}", db)
        } else {
            format!("{:.1}", db)
        }
    }
}

/// Playback follow / auto-scroll behavior for the timeline viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoScrollMode {
    /// Never auto-scroll. User controls the viewport entirely.
    Off,
    /// When the playhead reaches the right edge, page forward by ~90% of
    /// the viewport width so the playhead lands near the left side again.
    /// Cheap, predictable, and friendly to low-end GPUs.
    Page,
    /// Keep the playhead at a roughly fixed fraction of the viewport while
    /// playing. Smoother, but more scroll churn — left as an opt-in.
    Continuous,
}

impl Default for AutoScrollMode {
    fn default() -> Self {
        AutoScrollMode::Page
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineTool {
    Pointer,
    Pen,
    Cut,
    Glue,
    Mute,
    Time,
    Automation,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineViewport {
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub target_scroll_x: f32,
    pub target_scroll_y: f32,
    pub pixels_per_second: f32,
    pub pixels_per_beat: f32,
    pub viewport_width: f32,
    pub viewport_height: f32,
    pub track_area_height: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TransportState {
    pub playing: bool,
    pub recording: bool,
    pub metronome_enabled: bool,
    pub playhead_beats: f32,
    pub loop_enabled: bool,
    pub loop_start_beats: f32,
    pub loop_end_beats: f32,
    pub last_engine_frame: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineSelection {
    pub selected_track_id: Option<String>,
    pub selected_clip_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapDivision {
    Auto,
    Off,
    Bar1,
    Div1_1,
    Div1_2,
    Div1_4,
    Div1_8,
    Div1_16,
    Div1_32,
    Div1_64,
}

impl SnapDivision {
    pub fn label(&self) -> &'static str {
        match self {
            SnapDivision::Auto => "Auto",
            SnapDivision::Off => "Off",
            SnapDivision::Bar1 => "1 bar",
            SnapDivision::Div1_1 => "1/1",
            SnapDivision::Div1_2 => "1/2",
            SnapDivision::Div1_4 => "1/4",
            SnapDivision::Div1_8 => "1/8",
            SnapDivision::Div1_16 => "1/16",
            SnapDivision::Div1_32 => "1/32",
            SnapDivision::Div1_64 => "1/64",
        }
    }

    pub fn step_beats(&self, bpb: f32) -> f32 {
        match self {
            SnapDivision::Auto => 0.0,
            SnapDivision::Off => 0.0,
            SnapDivision::Bar1 => bpb,
            SnapDivision::Div1_1 => 4.0,
            SnapDivision::Div1_2 => 2.0,
            SnapDivision::Div1_4 => 1.0,
            SnapDivision::Div1_8 => 0.5,
            SnapDivision::Div1_16 => 0.25,
            SnapDivision::Div1_32 => 0.125,
            SnapDivision::Div1_64 => 0.0625,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridLineLevel {
    Bar,
    Beat,
    Sub,
}

pub struct GridLine {
    pub x: f32,
    pub beat: f32,
    pub level: GridLineLevel,
    pub show_label: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MasterBusState {
    pub volume: f32,
    pub meter_level_l: f32,
    pub meter_level_r: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineState {
    pub bpm: f32,
    pub time_signature_num: u32,
    pub time_signature_den: u32,
    pub viewport: TimelineViewport,
    pub transport: TransportState,
    pub tracks: Vec<TrackState>,
    pub master: MasterBusState,
    pub selection: TimelineSelection,
    pub active_tool: TimelineTool,
    pub snap_to_grid: bool,
    pub grid_division: SnapDivision,
    pub dragging_track_id: Option<TrackId>,
    pub drag_origin_index: Option<usize>,
    pub drag_current_y: f32,
    pub drag_target_index: Option<usize>,
    /// True when the timeline viewport should follow the playhead during
    /// playback. Toggled off temporarily when the user manually scrolls or
    /// drags the viewport; can be re-enabled from the Follow button.
    pub follow_playhead: bool,
    pub auto_scroll_mode: AutoScrollMode,
}

impl Default for TimelineState {
    /// Clean, empty project. No tracks, no clips, no MIDI — the real runtime
    /// startup state. Use [`TimelineState::demo_project`] when you explicitly
    /// want the seeded demo content (development / screenshots).
    fn default() -> Self {
        Self {
            bpm: 120.0,
            time_signature_num: 4,
            time_signature_den: 4,
            viewport: TimelineViewport {
                scroll_x: 0.0,
                scroll_y: 0.0,
                target_scroll_x: 0.0,
                target_scroll_y: 0.0,
                pixels_per_second: 150.0,
                pixels_per_beat: 75.0,
                viewport_width: 0.0,
                viewport_height: 500.0,
                track_area_height: 500.0,
            },
            transport: TransportState {
                playing: false,
                recording: false,
                metronome_enabled: false,
                playhead_beats: 0.0,
                loop_enabled: false,
                loop_start_beats: 0.0,
                loop_end_beats: 16.0,
                last_engine_frame: 0,
            },
            tracks: Vec::new(),
            master: MasterBusState {
                volume: volume::db_to_norm(0.0),
                meter_level_l: 0.0,
                meter_level_r: 0.0,
            },
            selection: TimelineSelection {
                selected_track_id: None,
                selected_clip_ids: Vec::new(),
            },
            active_tool: TimelineTool::Pointer,
            snap_to_grid: true,
            grid_division: SnapDivision::Div1_16,
            dragging_track_id: None,
            drag_origin_index: None,
            drag_current_y: 0.0,
            drag_target_index: None,
            follow_playhead: true,
            auto_scroll_mode: AutoScrollMode::Page,
        }
    }
}

impl TimelineState {
    /// Seeded demo project — three tracks with synthetic audio/MIDI clips.
    /// Intended for development, screenshots, and the demo seed flag in the
    /// app entry point; never used by the real runtime default.
    pub fn demo_project() -> Self {
        let track1 = TrackState {
            id: "track-1".to_string(),
            name: "Audio 1".to_string(),
            track_type: TrackType::Audio,
            color: crate::theme::Colors::track_color_for_index(0),
            volume: volume::db_to_norm(-3.0),
            pan: 0.0,
            muted: false,
            solo: false,
            armed: false,
            input_monitor: false,
            meter_level_l: 0.62,
            meter_level_r: 0.68,
            clips: vec![
                ClipState {
                    id: "clip-1".to_string(),
                    name: "vocals_dry.wav".to_string(),
                    start_beat: 1.0,
                    duration_beats: 8.0,
                    source_duration_seconds: None,
                    offset_beats: 0.0,
                    gain: 1.0,
                    clip_type: ClipType::Audio {
                        file_id: "file-vocals-dry".to_string(),
                        source_path: None,
                    },
                    muted: false,
                    audio_import: AudioImportState::Ready,
                },
                ClipState {
                    id: "clip-2".to_string(),
                    name: "vocals_harmony.wav".to_string(),
                    start_beat: 10.0,
                    duration_beats: 6.0,
                    source_duration_seconds: None,
                    offset_beats: 0.0,
                    gain: 1.0,
                    clip_type: ClipType::Audio {
                        file_id: "file-vocals-harmony".to_string(),
                        source_path: None,
                    },
                    muted: false,
                    audio_import: AudioImportState::Ready,
                },
            ],
            automation_lanes: vec![AutomationLaneState {
                id: "lane-1".to_string(),
                name: "Volume".to_string(),
                visible: false,
                points: vec![
                    AutomationPoint {
                        beat: 0.0,
                        value: 0.8,
                    },
                    AutomationPoint {
                        beat: 4.0,
                        value: 0.5,
                    },
                    AutomationPoint {
                        beat: 8.0,
                        value: 0.8,
                    },
                ],
            }],
            inserts: Vec::new(),
            sends: Vec::new(),
        };

        let track2 = TrackState {
            id: "track-2".to_string(),
            name: "Audio 2".to_string(),
            track_type: TrackType::Audio,
            color: crate::theme::Colors::track_color_for_index(1),
            volume: volume::db_to_norm(-6.0),
            pan: -0.2,
            muted: false,
            solo: false,
            armed: false,
            input_monitor: false,
            meter_level_l: 0.42,
            meter_level_r: 0.48,
            clips: vec![ClipState {
                id: "clip-3".to_string(),
                name: "drums_loop_120.wav".to_string(),
                start_beat: 0.0,
                duration_beats: 16.0,
                source_duration_seconds: None,
                offset_beats: 0.0,
                gain: 1.0,
                clip_type: ClipType::Audio {
                    file_id: "file-drums-loop".to_string(),
                    source_path: None,
                },
                muted: false,
                audio_import: AudioImportState::Ready,
            }],
            automation_lanes: vec![],
            inserts: Vec::new(),
            sends: Vec::new(),
        };

        let track3 = TrackState {
            id: "track-3".to_string(),
            name: "Synth 3".to_string(),
            track_type: TrackType::Midi,
            color: crate::theme::Colors::track_color_for_index(2),
            volume: volume::db_to_norm(-1.5),
            pan: 0.3,
            muted: false,
            solo: false,
            armed: false,
            input_monitor: false,
            meter_level_l: 0.15,
            meter_level_r: 0.12,
            clips: vec![ClipState {
                id: "clip-4".to_string(),
                name: "synth_lead.mid".to_string(),
                start_beat: 4.0,
                duration_beats: 8.0,
                source_duration_seconds: None,
                offset_beats: 0.0,
                gain: 1.0,
                clip_type: ClipType::Midi {
                    notes: vec![
                        MidiNoteState::new(60, 0.0, 1.0, 100),
                        MidiNoteState::new(64, 1.0, 1.0, 100),
                        MidiNoteState::new(67, 2.0, 1.0, 100),
                        MidiNoteState::new(72, 3.0, 2.0, 110),
                        MidiNoteState::new(67, 5.0, 1.0, 90),
                        MidiNoteState::new(64, 6.0, 1.0, 90),
                        MidiNoteState::new(60, 7.0, 1.0, 80),
                    ],
                },
                muted: false,
                audio_import: AudioImportState::default(),
            }],
            automation_lanes: vec![],
            inserts: Vec::new(),
            sends: Vec::new(),
        };

        Self {
            bpm: 120.0,
            time_signature_num: 4,
            time_signature_den: 4,
            viewport: TimelineViewport {
                scroll_x: 0.0,
                scroll_y: 0.0,
                target_scroll_x: 0.0,
                target_scroll_y: 0.0,
                pixels_per_second: 150.0, // Default zoom level in Web UI
                pixels_per_beat: 75.0,
                viewport_width: 0.0,
                viewport_height: 500.0,
                track_area_height: 500.0,
            },
            transport: TransportState {
                playing: false,
                recording: false,
                metronome_enabled: false,
                playhead_beats: 2.0,
                loop_enabled: true,
                loop_start_beats: 0.0,
                loop_end_beats: 16.0,
                last_engine_frame: 0,
            },
            tracks: vec![track1, track2, track3],
            master: MasterBusState {
                volume: volume::db_to_norm(0.0),
                meter_level_l: 0.0,
                meter_level_r: 0.0,
            },
            selection: TimelineSelection {
                selected_track_id: Some("track-1".to_string()),
                selected_clip_ids: vec![],
            },
            active_tool: TimelineTool::Pointer,
            snap_to_grid: true,
            grid_division: SnapDivision::Div1_16,
            dragging_track_id: None,
            drag_origin_index: None,
            drag_current_y: 0.0,
            drag_target_index: None,
            follow_playhead: true,
            auto_scroll_mode: AutoScrollMode::Page,
        }
    }

    /// Mutating variant of [`TimelineState::demo_project`]. Seeds the given
    /// state with demo content in-place.
    pub fn seed_demo_content(&mut self) {
        *self = Self::demo_project();
    }
}

// ── Time conversions and coordinate helpers ───────────────────────────────────────

pub const HEADER_WIDTH: f32 = 320.0; // Keep it slightly wider for native controls
pub const TRACK_HEIGHT: f32 = 76.0;
pub const RULER_HEIGHT: f32 = 30.0;
pub type TrackId = String;

impl TimelineState {
    pub fn seconds_per_beat(&self) -> f32 {
        60.0 / self.bpm.max(1.0)
    }

    pub fn seconds_to_beats(&self, seconds: f64) -> f32 {
        (seconds * self.bpm.max(1.0) as f64 / 60.0) as f32
    }

    pub fn beats_to_seconds(&self, beats: f32) -> f32 {
        beats * self.seconds_per_beat()
    }

    pub fn pixels_per_beat(&self) -> f32 {
        self.viewport.pixels_per_second * self.seconds_per_beat()
    }

    fn sync_pixels_per_beat(&mut self) {
        self.viewport.pixels_per_beat = self.pixels_per_beat();
    }

    pub fn beats_per_bar(&self) -> f32 {
        self.time_signature_num as f32 * (4.0 / self.time_signature_den as f32)
    }

    pub fn time_to_content_x(&self, time_sec: f32) -> f32 {
        (time_sec * self.viewport.pixels_per_second - self.viewport.scroll_x).round()
    }

    pub fn content_x_to_time(&self, x: f32) -> f32 {
        ((x + self.viewport.scroll_x) / self.viewport.pixels_per_second).max(0.0)
    }

    pub fn beats_to_x(&self, beats: f32) -> f32 {
        self.time_to_content_x(beats * self.seconds_per_beat())
    }

    pub fn x_to_beats(&self, x: f32) -> f32 {
        self.seconds_to_beats(self.content_x_to_time(x) as f64)
    }

    pub fn beat_to_x(&self, beat: f32) -> f32 {
        self.beats_to_x(beat)
    }

    pub fn x_to_beat(&self, x: f32) -> f64 {
        self.x_to_beats(x) as f64
    }

    pub fn lane_y_to_track_id(&self, y: f32) -> Option<TrackId> {
        self.track_index_at_y(y)
            .and_then(|index| self.tracks.get(index))
            .map(|track| track.id.clone())
    }

    pub fn update_viewport_size(&mut self, width: f32, height: f32) {
        self.viewport.viewport_width = width.max(0.0);
        self.viewport.viewport_height = height.max(0.0);
        self.viewport.track_area_height = height.max(0.0);
        self.sync_pixels_per_beat();
    }

    pub fn get_visible_beat_range(&self, width: f32) -> (f32, f32) {
        let start = self.x_to_beats(0.0);
        let end = self.x_to_beats(width);
        (start, end)
    }

    pub fn visible_beat_range(&self, viewport_width: f32) -> (f32, f32) {
        self.get_visible_beat_range(viewport_width)
    }

    pub fn build_interval_list(&self) -> Vec<f32> {
        let bpb = self.beats_per_bar();
        let mut result = Vec::new();
        for &sub in &[
            1.0 / 32.0,
            1.0 / 16.0,
            1.0 / 8.0,
            1.0 / 4.0,
            1.0 / 2.0,
            1.0,
            2.0,
        ] {
            if sub < bpb {
                result.push(sub);
            }
        }
        for &mult in &[1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0] {
            result.push(bpb * mult);
        }
        result
    }

    pub fn get_grid_interval_beats(&self, ppb: f32) -> f32 {
        let min_beats = 100.0 / ppb.max(1.0);
        let intervals = self.build_interval_list();
        for &n in &intervals {
            if n >= min_beats {
                return n;
            }
        }
        *intervals.last().unwrap_or(&4.0)
    }

    pub fn get_grid_sub_beats(&self, ppb: f32) -> f32 {
        let _bpb = self.beats_per_bar();
        let interval = self.get_grid_interval_beats(ppb);
        let intervals = self.build_interval_list();
        if let Some(idx) = intervals.iter().position(|&x| x == interval) {
            if idx > 0 {
                return intervals[idx - 1];
            }
        }
        interval
    }

    pub fn snap_time(&self, seconds: f32) -> f32 {
        if !self.snap_to_grid || self.grid_division == SnapDivision::Off {
            return seconds;
        }
        let ppb = self.viewport.pixels_per_second * self.seconds_per_beat();
        let bpb = self.beats_per_bar();
        let sub_div = match self.grid_division {
            SnapDivision::Auto => self.get_grid_sub_beats(ppb),
            SnapDivision::Bar1 => bpb,
            _ => self.grid_division.step_beats(bpb),
        };
        if sub_div <= 0.0 {
            return seconds;
        }
        let spb = self.seconds_per_beat();
        let total_beats = seconds / spb;
        let snapped = (total_beats / sub_div).round() * sub_div;
        (snapped * spb).max(0.0)
    }

    pub fn get_arrangement_grid_lines(&self, viewport_width: f32) -> Vec<GridLine> {
        let power = crate::perf::power_mode();
        const MIN_LINE_SPACING_PX_BASE: f32 = 8.0;
        // Force sub lines further apart on low-end so we draw fewer of them.
        let min_line_spacing_px = if matches!(power, crate::perf::PowerMode::LowEnd) {
            16.0
        } else {
            MIN_LINE_SPACING_PX_BASE
        };
        const MIN_LABEL_SPACING_PX: f32 = 46.0;
        const MAX_GRID_LINES_BASE: usize = 1200;
        let max_grid_lines =
            (MAX_GRID_LINES_BASE as f32 * power.grid_line_budget_scale()) as usize;

        let ppb = self.pixels_per_beat().max(0.0001);
        let bpb = self.beats_per_bar();
        let viewport_width = viewport_width.max(1.0);
        let (start_beat, end_beat) = self.visible_beat_range(viewport_width);
        let start_beat = start_beat.max(0.0);
        let end_beat = end_beat.max(start_beat);

        let mut lines: Vec<GridLine> = Vec::new();
        let mut occupied_x: Vec<i32> = Vec::new();

        let mut add_line = |beat: f32, level: GridLineLevel| {
            if beat < start_beat - bpb || beat > end_beat + bpb {
                return;
            }
            let rb = (beat * 100000.0).round() / 100000.0;
            let x = self.beat_to_x(rb).round();
            let x_key = x as i32;
            if x < -1.0 || x > viewport_width + 1.0 {
                return;
            }
            if occupied_x
                .iter()
                .any(|existing| (x_key - *existing).abs() < 1)
            {
                return;
            }
            occupied_x.push(x_key);
            lines.push(GridLine {
                x,
                beat: rb,
                level,
                show_label: false,
            });
        };

        // Strongest level first. Later, weaker levels skip duplicate pixel x
        // positions, so a bar line can never be overpainted by a beat/sub line.
        let first_bar = (start_beat / bpb).floor() - 1.0;
        let last_bar = (end_beat / bpb).ceil() + 1.0;
        let mut bar = first_bar;
        while bar <= last_bar {
            add_line(bar * bpb, GridLineLevel::Bar);
            bar += 1.0;
        }

        if ppb >= 12.0 {
            let first_beat = start_beat.floor() - 1.0;
            let last_beat = end_beat.ceil() + 1.0;
            let mut beat = first_beat;
            while beat <= last_beat {
                add_line(beat, GridLineLevel::Beat);
                beat += 1.0;
            }
        }

        let sub_step = if !power.allow_sub_grid_lines() {
            None
        } else if ppb >= 96.0 {
            Some(1.0 / 16.0)
        } else if ppb >= 32.0 {
            Some(1.0 / 4.0)
        } else {
            None
        };

        if let Some(step) = sub_step.filter(|step| step * ppb >= min_line_spacing_px) {
            let first_sub = (start_beat / step).floor() - 1.0;
            let last_sub = (end_beat / step).ceil() + 1.0;
            let mut slot = first_sub;
            while slot <= last_sub {
                add_line(slot * step, GridLineLevel::Sub);
                slot += 1.0;
            }
        }

        lines.sort_by(|a, b| a.x.total_cmp(&b.x));

        if lines.len() > max_grid_lines {
            lines.truncate(max_grid_lines);
        }

        let mut last_label_x = f32::NEG_INFINITY;
        let mut ruler_labels = 0u64;
        for line in &mut lines {
            let can_label_level = match line.level {
                GridLineLevel::Bar => true,
                GridLineLevel::Beat => ppb >= 48.0,
                GridLineLevel::Sub => false,
            };
            if can_label_level && line.x - last_label_x >= MIN_LABEL_SPACING_PX {
                line.show_label = true;
                last_label_x = line.x;
                ruler_labels += 1;
            }
        }

        if crate::perf::enabled() {
            let major = lines
                .iter()
                .filter(|l| matches!(l.level, GridLineLevel::Bar))
                .count() as u64;
            let minor = lines.len() as u64 - major;
            crate::perf::count("visible_major_lines", major);
            crate::perf::count("visible_minor_lines", minor);
            crate::perf::count("ruler_labels_drawn", ruler_labels);
        }

        lines
    }

    pub fn format_bar_beat(&self, beats: f32) -> String {
        let bpb = self.beats_per_bar();
        let bar = (beats / bpb).floor() as i32 + 1;
        let beat = (beats % bpb).floor() as i32 + 1;
        format!("{}.{}", bar, beat)
    }

    // ── Identity helpers ─────────────────────────────────────────────────────

    pub fn next_track_id(&self) -> String {
        // Find the highest numeric suffix on "track-N" ids, plus one.
        let mut n = 0u32;
        for t in &self.tracks {
            if let Some(rest) = t.id.strip_prefix("track-") {
                if let Ok(v) = rest.parse::<u32>() {
                    if v > n {
                        n = v;
                    }
                }
            }
        }
        format!("track-{}", n + 1)
    }

    pub fn next_clip_id(&self) -> String {
        let mut n = 0u32;
        for t in &self.tracks {
            for c in &t.clips {
                if let Some(rest) = c.id.strip_prefix("clip-") {
                    if let Ok(v) = rest.parse::<u32>() {
                        if v > n {
                            n = v;
                        }
                    }
                }
            }
        }
        format!("clip-{}", n + 1)
    }

    pub fn track_index_at_y(&self, y: f32) -> Option<usize> {
        if y < 0.0 {
            return None;
        }
        let idx = ((y + self.viewport.scroll_y) / TRACK_HEIGHT).floor() as usize;
        if idx < self.tracks.len() {
            Some(idx)
        } else {
            None
        }
    }

    pub fn track_insert_index_at_y(&self, y: f32) -> usize {
        if self.tracks.is_empty() {
            return 0;
        }
        let content_y = (y + self.viewport.scroll_y).max(0.0);
        ((content_y / TRACK_HEIGHT).round() as usize).clamp(0, self.tracks.len())
    }

    pub fn begin_track_drag(&mut self, track_id: &str, origin_index: usize, y: f32) {
        self.dragging_track_id = Some(track_id.to_string());
        self.drag_origin_index = Some(origin_index);
        self.drag_current_y = y;
        self.drag_target_index = Some(origin_index.min(self.tracks.len()));
    }

    pub fn update_track_drag(&mut self, y: f32) {
        self.drag_current_y = y;
        self.drag_target_index = Some(self.track_insert_index_at_y(y));
    }

    pub fn clear_track_drag(&mut self) {
        self.dragging_track_id = None;
        self.drag_origin_index = None;
        self.drag_current_y = 0.0;
        self.drag_target_index = None;
    }

    pub fn reorder_track(&mut self, track_id: &str, target_index: usize) -> bool {
        let Some(origin_index) = self.tracks.iter().position(|track| track.id == track_id) else {
            self.clear_track_drag();
            return false;
        };
        let target_index = target_index.clamp(0, self.tracks.len());
        let insert_index = if origin_index < target_index {
            target_index.saturating_sub(1)
        } else {
            target_index
        };
        if insert_index == origin_index {
            self.clear_track_drag();
            return false;
        }

        let track = self.tracks.remove(origin_index);
        let insert_index = insert_index.min(self.tracks.len());
        self.tracks.insert(insert_index, track);
        if let Some(selected) = self.selection.selected_track_id.as_deref() {
            if !self.tracks.iter().any(|track| track.id == selected) {
                self.selection.selected_track_id =
                    self.tracks.get(insert_index).map(|t| t.id.clone());
            }
        }
        self.clear_track_drag();
        true
    }

    /// Snap a beat value to the current grid (or return it unchanged when snap is off).
    pub fn snap_beats(&self, beats: f32) -> f32 {
        let snapped_sec = self.snap_time(beats * self.seconds_per_beat());
        snapped_sec / self.seconds_per_beat()
    }

    /// Create a new audio track with auto-assigned id/color.
    pub fn create_audio_track(&mut self) -> String {
        let name = format!("Audio {}", self.tracks.len() + 1);
        let log_name = name.clone();
        let id = self.create_track(CreateTrackOptions {
            track_type: TrackType::Audio,
            name,
            color: self.track_color_for_index(self.tracks.len()),
            volume: volume::db_to_norm(0.0),
            pan: 0.0,
            armed: false,
            input_monitor: false,
        });
        eprintln!("[import] created track id={} name={}", id, log_name);
        id
    }

    pub fn create_midi_track(&mut self) -> String {
        let name = format!("MIDI {}", self.tracks.len() + 1);
        self.create_track(CreateTrackOptions {
            track_type: TrackType::Midi,
            name,
            color: self.track_color_for_index(self.tracks.len()),
            volume: volume::db_to_norm(0.0),
            pan: 0.0,
            armed: false,
            input_monitor: false,
        })
    }

    // ── MIDI clip / note mutations ────────────────────────────────────────
    // Single source of truth for piano-roll edits. The piano-roll editor calls
    // these inside `Timeline::update` and then marks the project dirty so the
    // engine sync + autosave see the change. Notes are stored relative to the
    // clip start (matches the WebUI model). Every mutation clamps to valid
    // ranges so a bad gesture can never produce an out-of-range note.

    /// Create an empty MIDI clip on `track_id` at `start_beat` (snapped by the
    /// caller if desired). Returns the new clip id, or `None` if the track is
    /// missing. The clip is selected so the editor can pick it up immediately.
    pub fn create_midi_clip(&mut self, track_id: &str, start_beat: f32, length_beats: f32) -> Option<String> {
        if !self.tracks.iter().any(|t| t.id == track_id) {
            return None;
        }
        let clip_id = self.next_clip_id();
        let name = format!("MIDI {}", clip_id.strip_prefix("clip-").unwrap_or(&clip_id));
        let new_clip = ClipState {
            id: clip_id.clone(),
            name,
            start_beat: start_beat.max(0.0),
            duration_beats: length_beats.max(1.0),
            source_duration_seconds: None,
            offset_beats: 0.0,
            gain: 1.0,
            clip_type: ClipType::Midi { notes: Vec::new() },
            muted: false,
            audio_import: AudioImportState::default(),
        };
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            track.clips.push(new_clip);
        }
        self.selection.selected_track_id = Some(track_id.to_string());
        self.selection.selected_clip_ids = vec![clip_id.clone()];
        if midi_debug_enabled() {
            eprintln!(
                "[midi] create_midi_clip track={} clip={} start={:.3} len={:.3}",
                track_id, clip_id, start_beat, length_beats
            );
        }
        Some(clip_id)
    }

    /// Borrow the notes of a MIDI clip by id.
    pub fn midi_clip_notes(&self, clip_id: &str) -> Option<&Vec<MidiNoteState>> {
        for track in &self.tracks {
            for clip in &track.clips {
                if clip.id == clip_id {
                    if let ClipType::Midi { notes } = &clip.clip_type {
                        return Some(notes);
                    }
                }
            }
        }
        None
    }

    fn midi_clip_notes_mut(&mut self, clip_id: &str) -> Option<&mut Vec<MidiNoteState>> {
        for track in &mut self.tracks {
            for clip in &mut track.clips {
                if clip.id == clip_id {
                    if let ClipType::Midi { notes } = &mut clip.clip_type {
                        return Some(notes);
                    }
                }
            }
        }
        None
    }

    /// Add a note to a MIDI clip. Returns the new note id.
    pub fn add_midi_note(
        &mut self,
        clip_id: &str,
        pitch: u8,
        start: f32,
        duration: f32,
        velocity: u8,
    ) -> Option<u64> {
        let note = MidiNoteState::new(pitch, start, duration, velocity);
        let id = note.id;
        let notes = self.midi_clip_notes_mut(clip_id)?;
        notes.push(note);
        if midi_debug_enabled() {
            eprintln!(
                "[midi] add_note clip={} id={} pitch={} start={:.3} dur={:.3} vel={}",
                clip_id, id, pitch.min(127), start.max(0.0), duration.max(MIN_NOTE_BEATS), velocity.clamp(1, 127)
            );
        }
        Some(id)
    }

    /// Apply absolute start/pitch to a set of notes (move gesture). Each tuple
    /// is `(note_id, new_start_beats, new_pitch)`.
    pub fn move_midi_notes(&mut self, clip_id: &str, updates: &[(u64, f32, u8)]) {
        let Some(notes) = self.midi_clip_notes_mut(clip_id) else {
            return;
        };
        for (id, new_start, new_pitch) in updates {
            if let Some(note) = notes.iter_mut().find(|n| n.id == *id) {
                note.start = new_start.max(0.0);
                note.pitch = (*new_pitch).min(127);
            }
        }
        if midi_debug_enabled() {
            eprintln!("[midi] move_notes clip={} count={}", clip_id, updates.len());
        }
    }

    /// Set a note's length (resize gesture), clamped to [`MIN_NOTE_BEATS`].
    pub fn resize_midi_note(&mut self, clip_id: &str, id: u64, new_duration: f32) {
        let Some(notes) = self.midi_clip_notes_mut(clip_id) else {
            return;
        };
        if let Some(note) = notes.iter_mut().find(|n| n.id == id) {
            note.duration = new_duration.max(MIN_NOTE_BEATS);
            if midi_debug_enabled() {
                eprintln!(
                    "[midi] resize_note clip={} id={} dur={:.3}",
                    clip_id, id, note.duration
                );
            }
        }
    }

    /// Delete the given note ids from a MIDI clip. Returns how many were removed.
    pub fn delete_midi_notes(&mut self, clip_id: &str, ids: &[u64]) -> usize {
        let Some(notes) = self.midi_clip_notes_mut(clip_id) else {
            return 0;
        };
        let before = notes.len();
        notes.retain(|n| !ids.contains(&n.id));
        let removed = before - notes.len();
        if removed > 0 && midi_debug_enabled() {
            eprintln!("[midi] delete_notes clip={} removed={}", clip_id, removed);
        }
        removed
    }

    /// Set a note's velocity (1..=127).
    pub fn set_midi_note_velocity(&mut self, clip_id: &str, id: u64, velocity: u8) {
        let Some(notes) = self.midi_clip_notes_mut(clip_id) else {
            return;
        };
        if let Some(note) = notes.iter_mut().find(|n| n.id == id) {
            note.velocity = velocity.clamp(1, 127);
        }
    }

    /// Quantize the given note starts (or all notes when `ids` is empty) to the
    /// supplied grid step in beats. Rounds to the nearest step.
    pub fn quantize_midi_notes(&mut self, clip_id: &str, ids: &[u64], step_beats: f32) {
        if step_beats <= 0.0 {
            return;
        }
        let Some(notes) = self.midi_clip_notes_mut(clip_id) else {
            return;
        };
        let mut count = 0;
        for note in notes.iter_mut() {
            if ids.is_empty() || ids.contains(&note.id) {
                note.start = ((note.start / step_beats).round() * step_beats).max(0.0);
                count += 1;
            }
        }
        if midi_debug_enabled() {
            eprintln!(
                "[midi] quantize clip={} count={} step={:.4}",
                clip_id, count, step_beats
            );
        }
    }

    pub fn track_color_for_index(&self, index: usize) -> gpui::Rgba {
        crate::theme::Colors::track_color_for_index(index)
    }

    pub fn create_track(&mut self, options: CreateTrackOptions) -> String {
        let id = self.next_track_id();
        self.tracks.push(TrackState {
            id: id.clone(),
            name: options.name,
            track_type: options.track_type,
            color: options.color,
            volume: options.volume.clamp(0.0, 1.0),
            pan: options.pan.clamp(-1.0, 1.0),
            muted: false,
            solo: false,
            armed: options.armed,
            input_monitor: options.input_monitor,
            meter_level_l: 0.0,
            meter_level_r: 0.0,
            clips: Vec::new(),
            automation_lanes: Vec::new(),
            inserts: Vec::new(),
            sends: Vec::new(),
        });
        id
    }

    pub fn selected_audio_track_id(&self) -> Option<String> {
        let selected = self.selection.selected_track_id.as_deref()?;
        self.tracks
            .iter()
            .find(|track| track.id == selected && matches!(track.track_type, TrackType::Audio))
            .map(|track| track.id.clone())
    }

    // ── Single-source-of-truth mutations ─────────────────────────────────────
    // These are the only paths that should mutate per-track UI state. Both the
    // timeline TrackHeader and the bottom-panel Mixer call into these, so the
    // two views can never drift apart.

    pub fn set_master_volume(&mut self, norm: f32) {
        self.master.volume = norm.clamp(0.0, 1.0);
    }

    pub fn set_track_volume(&mut self, track_id: &str, norm: f32) {
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            t.volume = norm.clamp(0.0, 1.0);
        }
    }

    pub fn set_track_pan(&mut self, track_id: &str, pan: f32) {
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            t.pan = pan.clamp(-1.0, 1.0);
        }
    }

    pub fn toggle_track_mute(&mut self, track_id: &str) {
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            t.muted = !t.muted;
        }
    }

    pub fn toggle_track_solo(&mut self, track_id: &str) {
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            t.solo = !t.solo;
        }
    }

    pub fn toggle_track_arm(&mut self, track_id: &str) {
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            t.armed = !t.armed;
        }
    }

    /// Append an empty insert slot to a track and return the slot id.
    /// Phase 1 — purely UI state; runtime is updated on the next project
    /// sync (the engine ignores unknown plugin descriptors gracefully).
    pub fn add_insert(&mut self, track_id: &str) -> Option<String> {
        let track = self.tracks.iter_mut().find(|t| t.id == track_id)?;
        let slot_id = format!("insert-{}-{}", track.id, track.inserts.len() + 1);
        let slot = InsertSlotState::empty(&slot_id);
        if plugin_debug_enabled() {
            eprintln!(
                "[plugin] add_insert track={} slot_id={}",
                track_id, slot_id
            );
        }
        track.inserts.push(slot);
        Some(slot_id)
    }

    /// Assign a plugin to an insert slot. The caller resolves the
    /// `plugin_id` → display metadata before calling so the UI doesn't
    /// have to know about the plugin registry directly.
    pub fn set_insert_plugin(
        &mut self,
        track_id: &str,
        insert_id: &str,
        plugin_id: String,
        plugin_path: Option<std::path::PathBuf>,
        plugin_format: InsertPluginFormat,
        display_name: String,
    ) {
        let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) else {
            return;
        };
        let Some(slot) = track.inserts.iter_mut().find(|i| i.id == insert_id) else {
            return;
        };
        slot.plugin_id = Some(plugin_id);
        slot.plugin_path = plugin_path;
        slot.plugin_format = Some(plugin_format);
        slot.display_name = display_name;
        slot.load_status = InsertLoadStatus::Ready;
        slot.bypassed = false;
        slot.parameters.clear();
        if plugin_debug_enabled() {
            eprintln!(
                "[plugin] set_insert_plugin track={} slot={} -> {}",
                track_id, insert_id, slot.display_name
            );
        }
    }

    pub fn remove_insert(&mut self, track_id: &str, insert_id: &str) {
        let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) else {
            return;
        };
        track.inserts.retain(|i| i.id != insert_id);
        if plugin_debug_enabled() {
            eprintln!("[plugin] remove_insert track={} slot={}", track_id, insert_id);
        }
    }

    pub fn toggle_insert_bypass(&mut self, track_id: &str, insert_id: &str) -> Option<bool> {
        let track = self.tracks.iter_mut().find(|t| t.id == track_id)?;
        let slot = track.inserts.iter_mut().find(|i| i.id == insert_id)?;
        slot.bypassed = !slot.bypassed;
        if plugin_debug_enabled() {
            eprintln!(
                "[plugin] toggle_bypass track={} slot={} -> {}",
                track_id, insert_id, slot.bypassed
            );
        }
        Some(slot.bypassed)
    }

    /// Set an insert slot's load status by id (Phase 2b engine readback).
    /// Returns `true` if the status actually changed, so callers can decide
    /// whether to repaint. Used by the audio sync completion handler to flip
    /// `Failed` when the engine reports a native plugin failed to instantiate.
    pub fn set_insert_load_status(
        &mut self,
        track_id: &str,
        insert_id: &str,
        status: InsertLoadStatus,
    ) -> bool {
        let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) else {
            return false;
        };
        let Some(slot) = track.inserts.iter_mut().find(|i| i.id == insert_id) else {
            return false;
        };
        if slot.load_status == status {
            return false;
        }
        if plugin_debug_enabled() {
            eprintln!(
                "[plugin] set_load_status track={} slot={} -> {:?}",
                track_id, insert_id, status
            );
        }
        slot.load_status = status;
        true
    }

    /// Add an aux send from `track_id` to the first Bus/Return track that
    /// isn't already a target (Phase 3 — a richer target picker is a follow-up,
    /// mirroring how inserts auto-seeded before the picker overlay). Returns
    /// the new send id, or `None` if there is no eligible routing track or the
    /// track already sends to every routing track.
    pub fn add_send(&mut self, track_id: &str) -> Option<String> {
        let existing: Vec<String> = self
            .tracks
            .iter()
            .find(|t| t.id == track_id)
            .map(|t| t.sends.iter().map(|s| s.target_track_id.clone()).collect())
            .unwrap_or_default();
        let target = self.tracks.iter().find(|t| {
            t.id != track_id && t.track_type.is_routing() && !existing.contains(&t.id)
        })?;
        let target_id = target.id.clone();
        let target_name = target.name.clone();

        let track = self.tracks.iter_mut().find(|t| t.id == track_id)?;
        let send_id = format!("send-{}-{}", track.id, track.sends.len() + 1);
        track.sends.push(SendSlotState {
            id: send_id.clone(),
            target_track_id: target_id.clone(),
            target_name,
            enabled: true,
            pre_fader: false,
            gain_db: 0.0,
        });
        if routing_debug_enabled() {
            eprintln!(
                "[routing] add_send track={} send={} -> {}",
                track_id, send_id, target_id
            );
        }
        Some(send_id)
    }

    pub fn remove_send(&mut self, track_id: &str, send_id: &str) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            track.sends.retain(|s| s.id != send_id);
            if routing_debug_enabled() {
                eprintln!("[routing] remove_send track={} send={}", track_id, send_id);
            }
        }
    }

    pub fn toggle_send_enabled(&mut self, track_id: &str, send_id: &str) -> Option<bool> {
        let track = self.tracks.iter_mut().find(|t| t.id == track_id)?;
        let send = track.sends.iter_mut().find(|s| s.id == send_id)?;
        send.enabled = !send.enabled;
        Some(send.enabled)
    }

    pub fn toggle_track_input_monitor(&mut self, track_id: &str) {
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            t.input_monitor = !t.input_monitor;
        }
    }

    pub fn select_track(&mut self, track_id: &str) {
        self.selection.selected_track_id = Some(track_id.to_string());
        self.selection.selected_clip_ids.clear();
    }

    pub fn select_clip(&mut self, clip_id: &str) {
        self.selection.selected_clip_ids = vec![clip_id.to_string()];
        if let Some(track) = self
            .tracks
            .iter()
            .find(|t| t.clips.iter().any(|c| c.id == clip_id))
        {
            self.selection.selected_track_id = Some(track.id.clone());
        }
    }

    pub fn find_track(&self, track_id: &str) -> Option<&TrackState> {
        self.tracks.iter().find(|t| t.id == track_id)
    }

    pub fn find_clip(&self, clip_id: &str) -> Option<(&TrackState, &ClipState)> {
        for t in &self.tracks {
            if let Some(c) = t.clips.iter().find(|c| c.id == clip_id) {
                return Some((t, c));
            }
        }
        None
    }

    pub fn delete_track(&mut self, track_id: &str) {
        if let Some(index) = self.tracks.iter().position(|track| track.id == track_id) {
            self.tracks.remove(index);
            if self.selection.selected_track_id.as_deref() == Some(track_id) {
                self.selection.selected_track_id = self
                    .tracks
                    .get(index.saturating_sub(1))
                    .map(|t| t.id.clone());
            }
            self.selection.selected_clip_ids.clear();
        }
    }

    pub fn delete_clip(&mut self, clip_id: &str) {
        for track in &mut self.tracks {
            if let Some(index) = track.clips.iter().position(|clip| clip.id == clip_id) {
                track.clips.remove(index);
                self.selection.selected_clip_ids.retain(|id| id != clip_id);
                self.selection.selected_track_id = Some(track.id.clone());
                return;
            }
        }
    }

    pub fn duplicate_clip(&mut self, clip_id: &str) {
        let next_id = self.next_clip_id();
        let snap_step = if self.snap_to_grid && self.grid_division != SnapDivision::Off {
            Some((self.grid_division.step_beats(self.beats_per_bar())).max(0.0))
        } else {
            None
        };
        for track in &mut self.tracks {
            if let Some(index) = track.clips.iter().position(|clip| clip.id == clip_id) {
                let mut duplicate = track.clips[index].clone();
                duplicate.id = next_id;
                duplicate.name = format!("{} Copy", duplicate.name);
                let raw_start = duplicate.start_beat + duplicate.duration_beats;
                duplicate.start_beat = snap_step
                    .filter(|step| *step > 0.0)
                    .map(|step| (raw_start / step).round() * step)
                    .unwrap_or(raw_start)
                    .max(0.0);
                let duplicate_id = duplicate.id.clone();
                track.clips.insert(index + 1, duplicate);
                self.selection.selected_track_id = Some(track.id.clone());
                self.selection.selected_clip_ids = vec![duplicate_id];
                return;
            }
        }
    }

    /// Multiplicative zoom around a content-space x anchor. Updates
    /// `pixels_per_second` and shifts `scroll_x` so the time under `anchor_x`
    /// (a screen-space x inside the track lane, already net of header) stays
    /// fixed under the cursor. Smooth — accepts arbitrary positive `factor`.
    pub fn zoom_by(&mut self, factor: f32, anchor_x: f32) {
        let factor = factor.max(0.0001);
        let old_pps = self.viewport.pixels_per_second.max(0.0001);
        let new_pps = (old_pps * factor).clamp(4.0, 4000.0);
        if (new_pps - old_pps).abs() < 0.0001 {
            return;
        }
        // Time under anchor before the change.
        let anchor_time = (anchor_x + self.viewport.scroll_x) / old_pps;
        self.viewport.pixels_per_second = new_pps;
        self.sync_pixels_per_beat();
        // Re-solve scroll_x so the same anchor_time lands under anchor_x.
        let new_scroll = (anchor_time * new_pps - anchor_x).max(0.0);
        self.viewport.scroll_x = new_scroll;
        self.viewport.target_scroll_x = new_scroll;
    }

    pub fn scroll_by(&mut self, delta_x: f32, delta_y: f32, max_x: f32, max_y: f32) {
        self.viewport.target_scroll_x =
            (self.viewport.target_scroll_x + delta_x).clamp(0.0, max_x.max(0.0));
        self.viewport.target_scroll_y =
            (self.viewport.target_scroll_y + delta_y).clamp(0.0, max_y.max(0.0));
        self.smooth_scroll_towards_target();
    }

    pub fn set_scroll_immediate(&mut self, x: f32, y: f32, max_x: f32, max_y: f32) {
        self.viewport.scroll_x = x.clamp(0.0, max_x.max(0.0));
        self.viewport.scroll_y = y.clamp(0.0, max_y.max(0.0));
        self.viewport.target_scroll_x = self.viewport.scroll_x;
        self.viewport.target_scroll_y = self.viewport.scroll_y;
    }

    pub fn clamp_scroll(&mut self, max_x: f32, max_y: f32) {
        let max_x = max_x.max(0.0);
        let max_y = max_y.max(0.0);
        self.viewport.scroll_x = self.viewport.scroll_x.clamp(0.0, max_x);
        self.viewport.scroll_y = self.viewport.scroll_y.clamp(0.0, max_y);
        self.viewport.target_scroll_x = self.viewport.target_scroll_x.clamp(0.0, max_x);
        self.viewport.target_scroll_y = self.viewport.target_scroll_y.clamp(0.0, max_y);
    }

    /// Keep the playhead inside the visible viewport while playing.
    /// Returns true when the viewport scrolled (caller should `cx.notify`).
    /// Cheap — no allocation, just a couple of float comparisons.
    pub fn update_auto_scroll_for_playhead(&mut self, playhead_beats: f32) -> bool {
        if !self.follow_playhead || self.auto_scroll_mode == AutoScrollMode::Off {
            return false;
        }
        let viewport_width = self.viewport.viewport_width;
        if viewport_width <= 1.0 {
            return false;
        }
        let pps = self.viewport.pixels_per_second.max(0.0001);
        let playhead_content_x = playhead_beats.max(0.0) * self.seconds_per_beat() * pps;
        let scroll_x = self.viewport.scroll_x;

        // Trigger thresholds: right 15% / left 5% of the viewport.
        let right_trigger = scroll_x + viewport_width * 0.85;
        let left_trigger = scroll_x + viewport_width * 0.05;

        let new_scroll_x = match self.auto_scroll_mode {
            AutoScrollMode::Off => return false,
            AutoScrollMode::Page => {
                if playhead_content_x > right_trigger {
                    // Page forward so the playhead lands ~10% into the new viewport.
                    (playhead_content_x - viewport_width * 0.10).max(0.0)
                } else if playhead_content_x < scroll_x {
                    // Playhead seeked or wrapped back behind the viewport — recenter.
                    (playhead_content_x - viewport_width * 0.10).max(0.0)
                } else {
                    return false;
                }
            }
            AutoScrollMode::Continuous => {
                if playhead_content_x > right_trigger || playhead_content_x < left_trigger {
                    (playhead_content_x - viewport_width * 0.40).max(0.0)
                } else {
                    return false;
                }
            }
        };

        if (new_scroll_x - scroll_x).abs() < 0.5 {
            return false;
        }
        if std::env::var_os("FUTUREBOARD_AUTOSCROLL_DEBUG").is_some() {
            eprintln!(
                "[autoscroll] playhead_x={:.1} viewport=[{:.1}..{:.1}] scroll_x: {:.1} -> {:.1} mode={:?}",
                playhead_content_x,
                scroll_x,
                scroll_x + viewport_width,
                scroll_x,
                new_scroll_x,
                self.auto_scroll_mode
            );
        }
        self.viewport.scroll_x = new_scroll_x;
        self.viewport.target_scroll_x = new_scroll_x;
        true
    }

    /// Called when the user manually scrolls/drags the viewport — temporarily
    /// disables follow-playhead so playback won't fight the user. Re-enable
    /// by toggling the Follow control.
    pub fn note_user_scrolled(&mut self) {
        if self.follow_playhead {
            self.follow_playhead = false;
        }
    }

    pub fn set_follow_playhead(&mut self, follow: bool) {
        self.follow_playhead = follow;
    }

    pub fn smooth_scroll_towards_target(&mut self) -> bool {
        let dx = self.viewport.target_scroll_x - self.viewport.scroll_x;
        let dy = self.viewport.target_scroll_y - self.viewport.scroll_y;
        if dx.abs() < 0.35 && dy.abs() < 0.35 {
            self.viewport.scroll_x = self.viewport.target_scroll_x;
            self.viewport.scroll_y = self.viewport.target_scroll_y;
            return false;
        }
        self.viewport.scroll_x += dx * 0.42;
        self.viewport.scroll_y += dy * 0.42;
        true
    }

    pub fn move_clip_to_track(&mut self, clip_id: &str, target_track_id: &str, start_beat: f32) {
        let start_beat = self.snap_beats(start_beat).max(0.0);
        let mut moved_clip = None;
        let mut source_track_id = None;

        for track in &mut self.tracks {
            if let Some(index) = track.clips.iter().position(|clip| clip.id == clip_id) {
                let mut clip = track.clips.remove(index);
                clip.start_beat = start_beat;
                moved_clip = Some(clip);
                source_track_id = Some(track.id.clone());
                break;
            }
        }

        let Some(clip) = moved_clip else {
            return;
        };

        let target_id = if self.tracks.iter().any(|track| track.id == target_track_id) {
            target_track_id.to_string()
        } else {
            source_track_id.unwrap_or_else(|| target_track_id.to_string())
        };

        if let Some(track) = self.tracks.iter_mut().find(|track| track.id == target_id) {
            track.clips.push(clip);
            self.selection.selected_track_id = Some(track.id.clone());
            self.selection.selected_clip_ids = vec![clip_id.to_string()];
        }
    }

    /// Drop a clip onto the timeline. `drop_x` and `drop_y` are in the track
    /// area coordinate system (header_width and ruler_height already stripped).
    /// Imports a clip with unknown metadata. The 2-bar duration is a temporary
    /// placeholder and must be replaced by DirectAudioEngine metadata.
    pub fn import_audio_at(
        &mut self,
        source_path: String,
        clip_name: String,
        drop_x: f32,
        drop_y: f32,
    ) -> String {
        eprintln!(
            "[import] drop path={} clip={} drop_x={:.1} drop_y={:.1}",
            source_path, clip_name, drop_x, drop_y
        );
        // Resolve target track: an existing lane under drop_y, otherwise create one.
        let track_id = match self.track_index_at_y(drop_y) {
            Some(idx) if matches!(self.tracks[idx].track_type, TrackType::Audio) => {
                self.tracks[idx].id.clone()
            }
            _ => self.create_audio_track(),
        };

        // Resolve start beat with snap.
        let raw_beats = self.x_to_beats(drop_x.max(0.0));
        let start_beat = self.snap_beats(raw_beats).max(0.0);

        self.insert_audio_clip(track_id, source_path, clip_name, start_beat)
    }

    pub fn import_audio_to_selected_or_new_track(
        &mut self,
        source_path: String,
        clip_name: String,
    ) -> String {
        let track_id = self
            .selected_audio_track_id()
            .unwrap_or_else(|| self.create_audio_track());
        eprintln!(
            "[import] browser path={} clip={} resolved_track_id={}",
            source_path, clip_name, track_id
        );
        let start_beat = self.snap_beats(self.x_to_beats(0.0)).max(0.0);
        self.insert_audio_clip(track_id, source_path, clip_name, start_beat)
    }

    fn insert_audio_clip(
        &mut self,
        track_id: String,
        source_path: String,
        clip_name: String,
        start_beat: f32,
    ) -> String {
        let track_id = if self.tracks.iter().any(|track| track.id == track_id) {
            track_id
        } else {
            eprintln!(
                "[import] target track id={} missing; creating fallback audio track",
                track_id
            );
            self.create_audio_track()
        };

        let duration_beats = 8.0;
        eprintln!(
            "[audio-import] WARNING using fallback duration because metadata is pending: path={} duration_beats=8.0",
            source_path
        );
        let clip_id = self.next_clip_id();
        let log_clip_name = clip_name.clone();
        let new_clip = ClipState {
            id: clip_id.clone(),
            name: clip_name,
            start_beat: start_beat.max(0.0),
            duration_beats,
            source_duration_seconds: None,
            offset_beats: 0.0,
            gain: 1.0,
            clip_type: ClipType::Audio {
                file_id: source_path.clone(),
                source_path: Some(source_path),
            },
            muted: false,
            audio_import: AudioImportState::Pending,
        };

        if let Some(track) = self.tracks.iter_mut().find(|track| track.id == track_id) {
            track.clips.push(new_clip);
        }
        self.selection.selected_track_id = Some(track_id);
        self.selection.selected_clip_ids = vec![clip_id.clone()];
        debug_assert!(
            self.tracks
                .iter()
                .any(|track| track.clips.iter().any(|clip| clip.id == clip_id)),
            "imported clip must be stored inside a renderable track"
        );
        let selected_track = self
            .selection
            .selected_track_id
            .as_deref()
            .unwrap_or("<none>");
        let total_clips: usize = self.tracks.iter().map(|track| track.clips.len()).sum();
        eprintln!(
            "[import] clip id={} name={} track_id={} tracks.len={} clips.len={} selected_clip={}",
            clip_id,
            log_clip_name,
            selected_track,
            self.tracks.len(),
            total_clips,
            self.selection
                .selected_clip_ids
                .first()
                .map(String::as_str)
                .unwrap_or("<none>")
        );
        clip_id
    }

    pub fn update_audio_clip_metadata(
        &mut self,
        source_path: &str,
        format: &str,
        sample_rate: u32,
        channels: u16,
        total_frames: u64,
        duration_seconds: f64,
    ) -> bool {
        if duration_seconds <= 0.0 {
            return false;
        }
        let duration_beats = self.seconds_to_beats(duration_seconds);
        let mut changed = false;
        let mut matched = false;
        for track in &mut self.tracks {
            for clip in &mut track.clips {
                if let ClipType::Audio {
                    source_path: Some(path),
                    ..
                } = &clip.clip_type
                {
                    if path == source_path {
                        matched = true;
                        clip.source_duration_seconds = Some(duration_seconds);
                        if (clip.duration_beats - duration_beats).abs() > 0.001 {
                            clip.duration_beats = duration_beats;
                            changed = true;
                        }
                    }
                }
            }
        }
        if matched {
            self.log_audio_meta(
                source_path,
                format,
                sample_rate,
                channels,
                total_frames,
                duration_seconds,
            );
            self.log_audio_import(duration_beats);
        }
        changed
    }

    pub fn set_audio_import_for_path(&mut self, source_path: &str, state: AudioImportState) {
        for track in &mut self.tracks {
            for clip in &mut track.clips {
                if let ClipType::Audio {
                    source_path: Some(path),
                    ..
                } = &clip.clip_type
                {
                    if path == source_path {
                        clip.audio_import = state.clone();
                    }
                }
            }
        }
    }

    pub fn audio_source_duration_seconds(&self, source_path: &str) -> Option<f64> {
        self.tracks.iter().find_map(|track| {
            track.clips.iter().find_map(|clip| {
                if let ClipType::Audio {
                    source_path: Some(path),
                    ..
                } = &clip.clip_type
                {
                    if path == source_path {
                        return clip.source_duration_seconds;
                    }
                }
                None
            })
        })
    }

    fn log_audio_meta(
        &self,
        source_path: &str,
        format: &str,
        sample_rate: u32,
        channels: u16,
        total_frames: u64,
        duration_seconds: f64,
    ) {
        eprintln!("[audio-meta] path={}", source_path);
        eprintln!("[audio-meta] format={}", format);
        eprintln!("[audio-meta] sample_rate={}", sample_rate);
        eprintln!("[audio-meta] channels={}", channels);
        eprintln!("[audio-meta] total_frames={}", total_frames);
        eprintln!("[audio-meta] duration_seconds={:.6}", duration_seconds);
    }

    fn log_audio_import(&self, duration_beats: f32) {
        let bars_4_4 = duration_beats / 4.0;
        eprintln!("[audio-import] bpm={:.3}", self.bpm);
        eprintln!("[audio-import] duration_beats={:.6}", duration_beats);
        eprintln!("[audio-import] bars_4_4={:.6}", bars_4_4);
    }
}
