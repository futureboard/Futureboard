pub mod format;
pub mod io;
pub mod recent;
pub mod session;
pub mod template;

pub use format::{decode_project, encode_project, ProjectError, PROJECT_MAGIC, PROJECT_VERSION};
pub use io::{
    create_project_folder, default_projects_dir, import_audio_file_to_project, load_project,
    project_backup_path, project_temp_path, sanitize_project_name, save_project,
    validate_project_file, verify_project_file, LEGACY_PROJECT_FILE_EXT, PROJECT_FILE_EXT,
    SUPPORTED_PROJECT_FILE_EXTS,
};
pub use recent::{RecentProject, RecentProjectsStore};
pub use session::ProjectSession;
pub use template::{ProjectCreateOptions, ProjectTemplate};

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

// ── Identifiers ───────────────────────────────────────────────────────────────

fn new_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    // Cheap non-crypto ID: timestamp + stack address mix.
    let addr = &ts as *const _ as u64;
    format!("{:016x}{:016x}", ts as u64, addr ^ 0xDEAD_BEEF_CAFE_BABE)
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ── Enumerations ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectTrackType {
    Audio,
    Midi,
    Instrument,
    Bus,
    Return,
    Group,
    Master,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMonitorMode {
    #[default]
    Off,
    /// Monitor input whenever this mode is selected (Input).
    Always,
    /// Monitor input whenever the track is record-armed (Auto).
    WhenRecordArmed,
}

impl InputMonitorMode {
    pub fn cycle(self) -> Self {
        match self {
            Self::Off => Self::WhenRecordArmed,
            Self::WhenRecordArmed => Self::Always,
            Self::Always => Self::Off,
        }
    }

    pub fn is_active(self, armed: bool) -> bool {
        match self {
            Self::Off => false,
            Self::Always => true,
            Self::WhenRecordArmed => armed,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::WhenRecordArmed => "Auto",
            Self::Always => "Input",
        }
    }
}

#[derive(Debug, Clone)]
pub enum ClipSource {
    Audio {
        asset_id: String,
        source_path: Option<PathBuf>,
    },
    Rauf {
        asset_id: String,
        source_path: PathBuf,
        metadata_path: Option<PathBuf>,
        sample_format: String,
        sample_rate: u32,
        channels: u16,
        start_frame: u64,
        length_frames: u64,
    },
    Midi {
        notes: Vec<MidiNote>,
        controller_lanes: Vec<MidiControllerLane>,
    },
    Empty,
}

#[derive(Debug, Clone)]
pub struct MidiNote {
    pub pitch: u8,
    pub start_beats: f32,
    pub duration_beats: f32,
    pub velocity: u8,
    pub muted: bool,
}

/// Serialized MIDI controller stream selector. Mirrors
/// [`timeline_state::MidiControllerKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiControllerKind {
    CC(u8),
    PitchBend,
    ChannelPressure,
    PolyPressure,
}

#[derive(Debug, Clone)]
pub struct MidiControllerPoint {
    pub beat: f32,
    /// Normalized `0.0..=1.0`.
    pub value: f32,
}

#[derive(Debug, Clone)]
pub struct MidiControllerLane {
    pub kind: MidiControllerKind,
    pub points: Vec<MidiControllerPoint>,
    pub visible: bool,
    pub height: f32,
    pub collapsed: bool,
}

use crate::components::timeline::timeline_state::MidiControllerKind as TlControllerKind;

/// Map a live controller kind to its serialized form.
fn controller_kind_to_project(k: TlControllerKind) -> MidiControllerKind {
    match k {
        TlControllerKind::CC(n) => MidiControllerKind::CC(n),
        TlControllerKind::PitchBend => MidiControllerKind::PitchBend,
        TlControllerKind::ChannelPressure => MidiControllerKind::ChannelPressure,
        TlControllerKind::PolyPressure => MidiControllerKind::PolyPressure,
    }
}

/// Map a serialized controller kind back to the live form.
fn controller_kind_from_project(k: MidiControllerKind) -> TlControllerKind {
    match k {
        MidiControllerKind::CC(n) => TlControllerKind::CC(n),
        MidiControllerKind::PitchBend => TlControllerKind::PitchBend,
        MidiControllerKind::ChannelPressure => TlControllerKind::ChannelPressure,
        MidiControllerKind::PolyPressure => TlControllerKind::PolyPressure,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginFormat {
    Vst3,
    Clap,
    Au,
    Lv2,
    Unknown,
}

// ── Plugin state (binary blobs — future VST/CLAP ready) ──────────────────────

/// Raw binary snapshot of a plugin's internal state. Never JSON/base64.
/// Empty `state_bytes` is valid and means "use plugin defaults".
#[derive(Debug, Clone, Default)]
pub struct PluginStateBlob {
    pub plugin_id: String,
    pub format: Option<PluginFormat>,
    pub state_bytes: Vec<u8>,
    pub vendor: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProjectPluginInstance {
    pub instance_id: String,
    pub format: PluginFormat,
    pub plugin_path: Option<PathBuf>,
    pub plugin_uid: String,
    pub display_name: String,
    pub state: PluginStateBlob,
}

#[derive(Debug, Clone, Default)]
pub struct ProjectInsert {
    pub id: String,
    pub slot_index: u32,
    pub bypassed: bool,
    pub plugin: Option<ProjectPluginInstance>,
}

// ── Track routing ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectTrackInputRouting {
    None,
    AllInputs,
    AudioDeviceChannel {
        device_id: String,
        channel: u32,
    },
    AudioDeviceChannels {
        device_id: String,
        channels: Vec<u32>,
    },
    MidiDevice {
        device_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectTrackOutputRouting {
    Main,
    Bus { bus_id: String },
    HardwareOutput { device_id: String, channel: u32 },
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectTrackAudioFormat {
    Mono,
    Stereo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectTrackMidiInputRouting {
    None,
    AllInputs,
    MidiDevice { device_id: String },
}

#[derive(Debug, Clone)]
pub struct TrackRouting {
    pub input: ProjectTrackInputRouting,
    pub output: ProjectTrackOutputRouting,
    pub audio_format: ProjectTrackAudioFormat,
    pub midi_input: ProjectTrackMidiInputRouting,
    pub midi_channel: Option<u8>,
    pub sends: Vec<ProjectSend>,
}

impl Default for TrackRouting {
    fn default() -> Self {
        Self {
            input: ProjectTrackInputRouting::None,
            output: ProjectTrackOutputRouting::Main,
            audio_format: ProjectTrackAudioFormat::Stereo,
            midi_input: ProjectTrackMidiInputRouting::None,
            midi_channel: None,
            sends: Vec::new(),
        }
    }
}

impl TrackRouting {
    pub fn default_for_track_type(track_type: ProjectTrackType) -> Self {
        match track_type {
            ProjectTrackType::Audio => Self::default(),
            ProjectTrackType::Instrument => Self {
                midi_input: ProjectTrackMidiInputRouting::AllInputs,
                ..Self::default()
            },
            ProjectTrackType::Midi => Self {
                output: ProjectTrackOutputRouting::None,
                midi_input: ProjectTrackMidiInputRouting::AllInputs,
                ..Self::default()
            },
            ProjectTrackType::Bus
            | ProjectTrackType::Return
            | ProjectTrackType::Group
            | ProjectTrackType::Master => Self::default(),
        }
    }
}

/// Persisted aux send (Phase 3). Mirrors `timeline_state::SendSlotState`
/// minus the transient resolved `target_name`.
#[derive(Debug, Clone)]
pub struct ProjectSend {
    pub id: String,
    pub target_track_id: String,
    pub enabled: bool,
    pub pre_fader: bool,
    pub gain_db: f32,
}

// ── Automation ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AutomationPoint {
    pub beat: f32,
    pub value: f32,
    /// [`AutomationCurve`](crate::components::timeline::timeline_state::AutomationCurve)
    /// tag. Persisted from project version 2 onward; defaults to Linear (0)
    /// when loading older files.
    pub curve: u8,
}

/// Flattened automation target descriptor for persistence. `tag` matches
/// `AutomationTarget::to_tag`; the descriptor strings are only meaningful for
/// the plugin/send variants and are empty otherwise.
#[derive(Debug, Clone, Default)]
pub struct AutomationTargetDesc {
    pub tag: u8,
    pub insert_id: String,
    pub parameter_id: String,
    pub parameter_name: String,
    pub send_id: String,
}

#[derive(Debug, Clone)]
pub struct AutomationLane {
    pub id: String,
    pub parameter_name: String,
    /// Persisted from project version 2 onward; derived from `parameter_name`
    /// for older files.
    pub target: AutomationTargetDesc,
    pub enabled: bool,
    pub points: Vec<AutomationPoint>,
    pub visible: bool,
}

// ── Tracks & clips ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ProjectClip {
    pub id: String,
    pub name: String,
    pub start_beat: f64,
    pub duration_beats: f64,
    pub offset_beats: f32,
    pub gain: f32,
    pub muted: bool,
    pub source: ClipSource,
    /// Non-destructive clip-level stretch / pitch state (persisted v16+). Loads
    /// as [`AudioClipStretchState::default`] (mode Off, ratio 1.0) for older
    /// projects.
    pub stretch: AudioClipStretchState,
}

#[derive(Debug, Clone)]
pub struct ProjectTrack {
    pub id: String,
    pub name: String,
    pub track_type: ProjectTrackType,
    /// RGBA hex string e.g. "#56C7C9". Chosen to be human-readable in the file.
    pub color_hex: String,
    pub volume_norm: f32,
    pub pan: f32,
    pub muted: bool,
    pub solo: bool,
    pub record_arm: bool,
    pub input_monitor: InputMonitorMode,
    pub routing: TrackRouting,
    pub inserts: Vec<ProjectInsert>,
    pub automation_lanes: Vec<AutomationLane>,
    pub clips: Vec<ProjectClip>,
    /// Arrangement row height in px (v17+). `None` uses the default height.
    pub row_height_px: Option<f32>,
}

// ── Mixer ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ProjectMixer {
    pub master_volume_norm: f32,
    pub master_inserts: Vec<ProjectInsert>,
}

impl Default for ProjectMixer {
    fn default() -> Self {
        Self {
            master_volume_norm: crate::components::timeline::timeline_state::volume::db_to_norm(
                0.0,
            ),
            master_inserts: Vec::new(),
        }
    }
}

// ── Assets ───────────────────────────────────────────────────────────────────

/// An audio (or other media) file referenced by the project.
#[derive(Debug, Clone)]
pub struct ProjectAsset {
    pub id: String,
    pub original_filename: String,
    /// Path relative to project folder root, e.g. "Media/Audio/kick.wav"
    pub relative_path: Option<String>,
    /// Absolute fallback — used when file isn't inside project folder.
    pub absolute_path: Option<PathBuf>,
    pub duration_secs: Option<f64>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u8>,
    /// Content fingerprint (`"<len:x>-<crc:08x>"`) of the copied audio bytes.
    /// Persisted from project version 11 so re-imports of identical content can
    /// be deduplicated without re-hashing the whole asset folder on save.
    /// `None` for assets written by older versions.
    pub source_fingerprint: Option<String>,
    /// Project-relative peak cache path, e.g. `Cache/Waveforms/Assets__Audio__kick.wav.peaks`.
    pub waveform_peak_relative_path: Option<String>,
    /// Total PCM frames in the asset (v12+).
    pub duration_samples: Option<u64>,
}

/// Alias used in specs/docs for persisted audio registry entries.
pub type AudioAsset = ProjectAsset;

// ── Settings ──────────────────────────────────────────────────────────────────

/// A persisted tempo marker. `curve` is the `TempoCurve` tag (0=Hold,
/// 1=Linear, 2=Smooth). `id` is empty in v7 files and assigned on load.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectTempoPoint {
    pub id: String,
    pub beat: f64,
    pub bpm: f64,
    pub curve: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectTimeSignaturePoint {
    pub id: String,
    pub beat: f64,
    pub numerator: u16,
    pub denominator: u16,
    pub grouping: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectTimelineMarker {
    pub id: String,
    pub beat: f64,
    pub name: String,
    pub color_hex: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectTimelineRegion {
    pub id: String,
    pub start_beat: f64,
    pub end_beat: f64,
    pub name: String,
    pub color_hex: String,
}

#[derive(Debug, Clone)]
pub struct ProjectSettings {
    pub bpm: f64,
    /// Project-level tempo automation markers. Empty = static tempo at `bpm`.
    pub tempo_points: Vec<ProjectTempoPoint>,
    /// Global time signature markers. Empty on disk = migrate from legacy pair.
    pub time_signature_points: Vec<ProjectTimeSignaturePoint>,
    pub timeline_markers: Vec<ProjectTimelineMarker>,
    pub timeline_regions: Vec<ProjectTimelineRegion>,
    pub time_sig_num: u32,
    pub time_sig_den: u32,
    pub sample_rate: u32,
    pub bit_depth: u32,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            bpm: 120.0,
            tempo_points: Vec::new(),
            time_signature_points: Vec::new(),
            timeline_markers: Vec::new(),
            timeline_regions: Vec::new(),
            time_sig_num: 4,
            time_sig_den: 4,
            sample_rate: 48000,
            bit_depth: 24,
        }
    }
}

// ── Root project ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct FutureboardProject {
    pub id: String,
    pub name: String,
    pub created_at: u64,
    pub modified_at: u64,
    pub settings: ProjectSettings,
    pub tracks: Vec<ProjectTrack>,
    pub mixer: ProjectMixer,
    pub assets: Vec<ProjectAsset>,
}

impl FutureboardProject {
    pub fn new(name: impl Into<String>) -> Self {
        let now = now_secs();
        Self {
            id: new_id(),
            name: name.into(),
            created_at: now,
            modified_at: now,
            settings: ProjectSettings::default(),
            tracks: Vec::new(),
            mixer: ProjectMixer::default(),
            assets: Vec::new(),
        }
    }
}

// ── Conversion helpers ────────────────────────────────────────────────────────

/// Converts a `gpui::Rgba` to a hex color string "#RRGGBB".
/// Format an `Rgba` as a stable `#RRGGBB` string. Delegates to the canonical
/// [`crate::color`] helper so there is one color implementation project-wide.
pub fn rgba_to_hex(c: gpui::Rgba) -> String {
    crate::color::rgba_to_hex(c)
}

/// Converts a hex color string to `gpui::Rgba`. Unparseable values fall back to
/// the first default-palette color rather than panicking.
pub fn hex_to_rgba(hex: &str) -> gpui::Rgba {
    crate::color::parse_hex_color(hex).unwrap_or_else(|_| crate::color::auto_color_for_index(0))
}

// ── From TimelineState ────────────────────────────────────────────────────────

use crate::components::timeline::timeline_state::{
    AudioClipStretchState, ClipType, InsertSlotState, TimelineMarkerState, TimelineRegionState,
    TimelineState, TrackType as TlTrackType,
};

fn timeline_insert_to_project(idx: usize, slot: &InsertSlotState) -> ProjectInsert {
    use crate::components::timeline::timeline_state::InsertPluginFormat;

    let plugin = slot.plugin_id.as_ref().map(|pid| {
        let format = match slot.plugin_format {
            Some(InsertPluginFormat::Vst3) => PluginFormat::Vst3,
            Some(InsertPluginFormat::Clap) => PluginFormat::Clap,
            Some(InsertPluginFormat::Au) => PluginFormat::Au,
            Some(InsertPluginFormat::Lv2) => PluginFormat::Lv2,
            _ => PluginFormat::Unknown,
        };
        ProjectPluginInstance {
            instance_id: slot.id.clone(),
            format,
            plugin_path: slot.plugin_path.clone(),
            plugin_uid: pid.clone(),
            display_name: slot.display_name.clone(),
            state: PluginStateBlob {
                plugin_id: pid.clone(),
                format: Some(format),
                state_bytes: slot
                    .vst3_state
                    .as_ref()
                    .map(|state| state.as_ref().clone())
                    .unwrap_or_default(),
                vendor: None,
                name: Some(slot.display_name.clone()),
                version: None,
            },
        }
    });
    ProjectInsert {
        id: slot.id.clone(),
        slot_index: idx as u32,
        bypassed: slot.bypassed,
        plugin,
    }
}

fn project_insert_to_timeline(pi: &ProjectInsert) -> InsertSlotState {
    use crate::components::timeline::timeline_state::{
        InsertLoadStatus, InsertPluginFormat, PluginRuntimeBackend, PluginRuntimeState,
    };

    match &pi.plugin {
        Some(plugin) => {
            let plugin_format = match plugin.format {
                PluginFormat::Vst3 => InsertPluginFormat::Vst3,
                PluginFormat::Clap => InsertPluginFormat::Clap,
                PluginFormat::Au => InsertPluginFormat::Au,
                PluginFormat::Lv2 => InsertPluginFormat::Lv2,
                PluginFormat::Unknown => InsertPluginFormat::Unknown,
            };
            let bridge = SpherePluginHost::plugin_host_client::plugin_host_bridge_enabled()
                && plugin_format == InsertPluginFormat::Vst3;
            let path_missing = plugin
                .plugin_path
                .as_ref()
                .is_none_or(|path| !path.exists());
            let (load_status, runtime_state, runtime_backend) = if path_missing {
                (
                    InsertLoadStatus::Missing("Plugin file not found".to_string()),
                    PluginRuntimeState::Missing("Plugin file not found".to_string()),
                    if bridge {
                        PluginRuntimeBackend::ExternalBridge
                    } else {
                        PluginRuntimeBackend::InProcess
                    },
                )
            } else {
                (
                    InsertLoadStatus::Loading,
                    PluginRuntimeState::NotLoaded,
                    if bridge {
                        PluginRuntimeBackend::ExternalBridge
                    } else {
                        PluginRuntimeBackend::InProcess
                    },
                )
            };
            InsertSlotState {
                id: pi.id.clone(),
                plugin_id: Some(plugin.plugin_uid.clone()),
                plugin_path: plugin.plugin_path.clone(),
                plugin_format: Some(plugin_format),
                display_name: plugin.display_name.clone(),
                enabled: true,
                bypassed: pi.bypassed,
                load_status,
                runtime_backend,
                runtime_state,
                host_pid: None,
                parameters: Vec::new(),
                pending_open_editor: false,
                vst3_state: (!plugin.state.state_bytes.is_empty())
                    .then(|| std::sync::Arc::new(plugin.state.state_bytes.clone())),
            }
        }
        None => InsertSlotState::empty(pi.id.clone()),
    }
}

impl From<&TimelineState> for FutureboardProject {
    fn from(tl: &TimelineState) -> Self {
        let tracks = tl
            .tracks
            .iter()
            .map(|t| {
                let track_type = match t.track_type {
                    TlTrackType::Audio => ProjectTrackType::Audio,
                    TlTrackType::Midi => ProjectTrackType::Midi,
                    TlTrackType::Instrument => ProjectTrackType::Instrument,
                    TlTrackType::Bus => ProjectTrackType::Bus,
                    TlTrackType::Return => ProjectTrackType::Return,
                    TlTrackType::Master => ProjectTrackType::Master,
                };
                let clips = t
                    .clips
                    .iter()
                    .map(|c| {
                        let source = match &c.clip_type {
                            ClipType::Audio {
                                file_id,
                                source_path,
                            } => {
                                let path = source_path.as_deref().map(PathBuf::from);
                                if path
                                    .as_ref()
                                    .and_then(|p| p.extension())
                                    .and_then(|ext| ext.to_str())
                                    .is_some_and(|ext| ext.eq_ignore_ascii_case("rauf"))
                                {
                                    let metadata_path = path.as_ref().map(|p| {
                                        let mut value = p.as_os_str().to_os_string();
                                        value.push(".json");
                                        PathBuf::from(value)
                                    });
                                    ClipSource::Rauf {
                                        asset_id: file_id.clone(),
                                        source_path: path.unwrap_or_default(),
                                        metadata_path,
                                        sample_format: "s32le".to_string(),
                                        sample_rate: 48_000,
                                        channels: 0,
                                        start_frame: 0,
                                        length_frames: 0,
                                    }
                                } else {
                                    ClipSource::Audio {
                                        asset_id: file_id.clone(),
                                        source_path: path,
                                    }
                                }
                            }
                            ClipType::Midi {
                                notes,
                                controller_lanes,
                            } => ClipSource::Midi {
                                notes: notes
                                    .iter()
                                    .map(|n| MidiNote {
                                        pitch: n.pitch,
                                        start_beats: n.start,
                                        duration_beats: n.duration,
                                        velocity: n.velocity,
                                        muted: n.muted,
                                    })
                                    .collect(),
                                controller_lanes: controller_lanes
                                    .iter()
                                    .map(|lane| MidiControllerLane {
                                        kind: controller_kind_to_project(lane.kind),
                                        points: lane
                                            .points
                                            .iter()
                                            .map(|p| MidiControllerPoint {
                                                beat: p.beat,
                                                value: p.value,
                                            })
                                            .collect(),
                                        visible: lane.visible,
                                        height: lane.height,
                                        collapsed: lane.collapsed,
                                    })
                                    .collect(),
                            },
                        };
                        ProjectClip {
                            id: c.id.clone(),
                            name: c.name.clone(),
                            start_beat: c.start_beat as f64,
                            duration_beats: c.duration_beats as f64,
                            offset_beats: c.offset_beats,
                            gain: c.gain,
                            muted: c.muted,
                            source,
                            stretch: c.stretch.clone(),
                        }
                    })
                    .collect();
                let automation_lanes = t
                    .automation_lanes
                    .iter()
                    .map(|al| AutomationLane {
                        id: al.id.clone(),
                        parameter_name: al.name.clone(),
                        target: target_to_desc(&al.target),
                        enabled: al.enabled,
                        points: al
                            .points
                            .iter()
                            .map(|p| AutomationPoint {
                                beat: p.beat,
                                value: p.value,
                                curve: p.curve.to_tag(),
                            })
                            .collect(),
                        visible: al.visible,
                    })
                    .collect();
                ProjectTrack {
                    id: t.id.clone(),
                    name: t.name.clone(),
                    track_type,
                    color_hex: rgba_to_hex(t.color),
                    volume_norm: t.volume,
                    pan: t.pan,
                    muted: t.muted,
                    solo: t.solo,
                    record_arm: t.armed,
                    input_monitor: t.input_monitor,
                    routing: TrackRouting {
                        input: timeline_input_to_project(&t.routing.input),
                        output: timeline_output_to_project(&t.routing.output),
                        audio_format: timeline_audio_format_to_project(t.routing.audio_format),
                        midi_input: timeline_midi_input_to_project(&t.routing.midi_input),
                        midi_channel: t.routing.midi_channel.map(|ch| ch.clamp(1, 16)),
                        sends: t
                            .sends
                            .iter()
                            .map(|s| ProjectSend {
                                id: s.id.clone(),
                                target_track_id: s.target_track_id.clone(),
                                enabled: s.enabled,
                                pre_fader: s.pre_fader,
                                gain_db: s.gain_db,
                            })
                            .collect(),
                    },
                    inserts: t
                        .inserts
                        .iter()
                        .enumerate()
                        .map(|(idx, slot)| timeline_insert_to_project(idx, slot))
                        .collect(),
                    automation_lanes,
                    clips,
                    row_height_px: tl
                        .track_view_layout
                        .height_for(&t.id)
                        .filter(|h| {
                            (*h - crate::components::timeline::timeline_state::DEFAULT_TRACK_HEIGHT)
                                .abs()
                                >= 0.01
                        }),
                }
            })
            .collect();
        let mut project = FutureboardProject::new("Untitled Project");
        project.settings.bpm = tl.bpm as f64;
        project.settings.tempo_points = tl
            .tempo_map
            .points
            .iter()
            .map(|p| ProjectTempoPoint {
                id: p.id.clone(),
                beat: p.beat,
                bpm: p.bpm,
                curve: p.curve.to_tag(),
            })
            .collect();
        project.settings.time_signature_points = tl
            .time_signature_map
            .points
            .iter()
            .map(|p| ProjectTimeSignaturePoint {
                id: p.id.clone(),
                beat: p.beat,
                numerator: p.numerator,
                denominator: p.denominator,
                grouping: p.effective_grouping(),
            })
            .collect();
        project.settings.timeline_markers = tl
            .markers
            .iter()
            .map(|marker| ProjectTimelineMarker {
                id: marker.id.clone(),
                beat: marker.beat,
                name: marker.name.clone(),
                color_hex: marker.color_hex.clone(),
            })
            .collect();
        project.settings.timeline_regions = tl
            .regions
            .iter()
            .map(|region| ProjectTimelineRegion {
                id: region.id.clone(),
                start_beat: region.start_beat,
                end_beat: region.end_beat,
                name: region.name.clone(),
                color_hex: region.color_hex.clone(),
            })
            .collect();
        project.settings.time_sig_num = tl.time_signature_num;
        project.settings.time_sig_den = tl.time_signature_den;
        project.tracks = tracks;
        project.mixer.master_volume_norm = tl.master.volume;
        project.mixer.master_inserts = tl
            .master
            .inserts
            .iter()
            .enumerate()
            .map(|(idx, slot)| timeline_insert_to_project(idx, slot))
            .collect();
        project
    }
}

/// Apply a loaded `FutureboardProject` back onto an existing `TimelineState`.
pub fn apply_to_timeline(project: &FutureboardProject, tl: &mut TimelineState) {
    use crate::components::timeline::timeline_state::{
        AutomationLaneState, AutomationPoint as TlAutoPoint, ClipState,
        MidiControllerLane as TlControllerLane, MidiControllerPoint as TlControllerPoint,
        MidiNoteState, SendSlotState, TrackState,
    };

    tl.bpm = project.settings.bpm as f32;
    tl.tempo_map = crate::components::timeline::timeline_state::TempoMap::with_points(
        project
            .settings
            .tempo_points
            .iter()
            .map(|p| {
                crate::components::timeline::timeline_state::TempoPoint::with_id(
                    p.id.clone(),
                    p.beat,
                    p.bpm,
                    crate::components::timeline::timeline_state::TempoCurve::from_tag(p.curve),
                )
            })
            .collect(),
    );
    tl.tempo_map.ensure_point_ids();
    tl.markers = project
        .settings
        .timeline_markers
        .iter()
        .map(|marker| {
            TimelineMarkerState::with_id(
                marker.id.clone(),
                marker.beat,
                marker.name.clone(),
                marker.color_hex.clone(),
            )
        })
        .collect();
    tl.markers
        .sort_by(|a, b| a.beat.total_cmp(&b.beat).then_with(|| a.id.cmp(&b.id)));
    tl.regions = project
        .settings
        .timeline_regions
        .iter()
        .map(|region| {
            TimelineRegionState::with_id(
                region.id.clone(),
                region.start_beat,
                region.end_beat,
                region.name.clone(),
                region.color_hex.clone(),
            )
        })
        .collect();
    tl.regions.sort_by(|a, b| {
        a.start_beat
            .total_cmp(&b.start_beat)
            .then_with(|| a.id.cmp(&b.id))
    });
    if project.settings.time_signature_points.is_empty() {
        tl.time_signature_map =
            crate::components::timeline::timeline_state::TimeSignatureMap::with_default_4_4();
        tl.time_signature_map.points[0].numerator =
            project.settings.time_sig_num.clamp(1, 64) as u16;
        tl.time_signature_map.points[0].denominator =
            project.settings.time_sig_den.clamp(1, 32) as u16;
    } else {
        tl.time_signature_map =
            crate::components::timeline::timeline_state::TimeSignatureMap::with_points(
                project
                    .settings
                    .time_signature_points
                    .iter()
                    .map(|p| {
                        crate::components::timeline::timeline_state::TimeSignaturePoint::with_grouping(
                            p.id.clone(),
                            p.beat,
                            p.numerator,
                            p.denominator,
                            p.grouping.clone(),
                        )
                    })
                    .collect(),
            );
        tl.time_signature_map.ensure_point_ids();
    }
    tl.sync_legacy_time_signature_fields();
    tl.master.volume = project.mixer.master_volume_norm;
    tl.master.inserts = project
        .mixer
        .master_inserts
        .iter()
        .map(project_insert_to_timeline)
        .collect();

    tl.tracks = project
        .tracks
        .iter()
        .map(|pt| {
            let track_type = match pt.track_type {
                ProjectTrackType::Audio => TlTrackType::Audio,
                ProjectTrackType::Midi => TlTrackType::Midi,
                ProjectTrackType::Instrument => TlTrackType::Instrument,
                ProjectTrackType::Bus => TlTrackType::Bus,
                ProjectTrackType::Return => TlTrackType::Return,
                ProjectTrackType::Master => TlTrackType::Master,
                // Group has no timeline equivalent yet — treat as a bus.
                ProjectTrackType::Group => TlTrackType::Bus,
            };
            let clips = pt
                .clips
                .iter()
                .map(|pc| {
                    let clip_type = match &pc.source {
                        ClipSource::Audio {
                            asset_id,
                            source_path,
                        } => ClipType::Audio {
                            file_id: asset_id.clone(),
                            source_path: source_path
                                .as_ref()
                                .map(|p| p.to_string_lossy().into_owned()),
                        },
                        ClipSource::Rauf {
                            asset_id,
                            source_path,
                            ..
                        } => ClipType::Audio {
                            file_id: asset_id.clone(),
                            source_path: Some(source_path.to_string_lossy().into_owned()),
                        },
                        ClipSource::Midi {
                            notes,
                            controller_lanes,
                        } => ClipType::Midi {
                            notes: notes
                                .iter()
                                .map(|n| {
                                    let mut note = MidiNoteState::new(
                                        n.pitch,
                                        n.start_beats,
                                        n.duration_beats,
                                        n.velocity,
                                    );
                                    note.muted = n.muted;
                                    note
                                })
                                .collect(),
                            controller_lanes: controller_lanes
                                .iter()
                                .map(|lane| TlControllerLane {
                                    kind: controller_kind_from_project(lane.kind),
                                    points: lane
                                        .points
                                        .iter()
                                        .map(|p| TlControllerPoint::new(p.beat, p.value))
                                        .collect(),
                                    visible: lane.visible,
                                    height: lane.height,
                                    collapsed: lane.collapsed,
                                })
                                .collect(),
                        },
                        ClipSource::Empty => ClipType::Midi {
                            notes: Vec::new(),
                            controller_lanes: Vec::new(),
                        },
                    };
                    ClipState {
                        id: pc.id.clone(),
                        name: pc.name.clone(),
                        start_beat: pc.start_beat as f32,
                        duration_beats: pc.duration_beats as f32,
                        source_duration_seconds: None,
                        offset_beats: pc.offset_beats,
                        gain: pc.gain,
                        clip_type,
                        muted: pc.muted,
                        audio_import: crate::components::timeline::timeline_state::AudioImportState::default(),
                        stretch: pc.stretch.clone(),
                    }
                })
                .collect();
            let automation_lanes = pt
                .automation_lanes
                .iter()
                .map(|al| AutomationLaneState {
                    id: al.id.clone(),
                    name: al.parameter_name.clone(),
                    target: desc_to_target(&al.target, &al.parameter_name),
                    enabled: al.enabled,
                    visible: al.visible,
                    points: al
                        .points
                        .iter()
                        .map(|p| TlAutoPoint::with_curve(
                            p.beat,
                            p.value,
                            crate::components::timeline::timeline_state::AutomationCurve::from_tag(
                                p.curve,
                            ),
                        ))
                        .collect(),
                })
                .collect();
            let inserts: Vec<InsertSlotState> = pt
                .inserts
                .iter()
                .map(project_insert_to_timeline)
                .collect();
            let sends = pt
                .routing
                .sends
                .iter()
                .map(|s| {
                    let target_name = project
                        .tracks
                        .iter()
                        .find(|t| t.id == s.target_track_id)
                        .map(|t| t.name.clone())
                        .unwrap_or_else(|| s.target_track_id.clone());
                    SendSlotState {
                        id: s.id.clone(),
                        target_track_id: s.target_track_id.clone(),
                        target_name,
                        enabled: s.enabled,
                        pre_fader: s.pre_fader,
                        gain_db: s.gain_db,
                    }
                })
                .collect();
            let instrument_plugin_instance_id = match track_type {
                crate::components::timeline::timeline_state::TrackType::Instrument
                | crate::components::timeline::timeline_state::TrackType::Midi => inserts
                    .first()
                    .filter(|slot| slot.plugin_id.is_some())
                    .map(|slot| slot.id.clone()),
                _ => None,
            };
            TrackState {
                id: pt.id.clone(),
                name: pt.name.clone(),
                track_type,
                color: hex_to_rgba(&pt.color_hex),
                volume: pt.volume_norm,
                // Effective volume is derived (recomputed from automation at the
                // playhead after load); seed it from the persisted base so the
                // first frame before any recompute shows the saved value.
                volume_effective: pt.volume_norm,
                volume_automation_read: true,
                pan: pt.pan,
                muted: pt.muted,
                solo: pt.solo,
                armed: pt.record_arm,
                input_monitor: pt.input_monitor,
                meter_level_l: 0.0,
                meter_level_r: 0.0,
                meter_peak_hold_l: 0.0,
                meter_peak_hold_r: 0.0,
                meter_clip: false,
                clips,
                automation_lanes,
                lane_mode: crate::components::timeline::timeline_state::TrackLaneMode::Clips,
                selected_automation_target: None,
                inserts,
                sends,
                routing: project_routing_to_timeline(&pt.routing, track_type),
                instrument_plugin_instance_id,
            }
        })
        .collect();

    tl.track_view_layout.clear();
    tl.track_height_resize = None;
    tl.track_height_resize_arm = None;
    for pt in &project.tracks {
        let Some(height) = pt.row_height_px else {
            continue;
        };
        let Some(track) = tl.tracks.iter().find(|t| t.id == pt.id) else {
            continue;
        };
        let clamped = crate::components::timeline::timeline_state::clamp_track_row_height(
            track.track_type,
            height,
        );
        tl.track_view_layout.set_height(pt.id.clone(), clamped);
    }
}

fn timeline_input_to_project(
    input: &crate::components::timeline::timeline_state::TrackInputRouting,
) -> ProjectTrackInputRouting {
    use crate::components::timeline::timeline_state::TrackInputRouting as T;
    match input {
        T::None => ProjectTrackInputRouting::None,
        T::AllInputs => ProjectTrackInputRouting::AllInputs,
        T::AudioDeviceChannel { device_id, channel } => {
            ProjectTrackInputRouting::AudioDeviceChannel {
                device_id: device_id.clone(),
                channel: *channel,
            }
        }
        T::AudioDeviceChannels {
            device_id,
            channels,
        } => ProjectTrackInputRouting::AudioDeviceChannels {
            device_id: device_id.clone(),
            channels: channels.clone(),
        },
        T::MidiDevice { device_id } => ProjectTrackInputRouting::MidiDevice {
            device_id: device_id.clone(),
        },
    }
}

fn timeline_output_to_project(
    output: &crate::components::timeline::timeline_state::TrackOutputRouting,
) -> ProjectTrackOutputRouting {
    use crate::components::timeline::timeline_state::TrackOutputRouting as T;
    match output {
        T::Main => ProjectTrackOutputRouting::Main,
        T::Bus { bus_id } => ProjectTrackOutputRouting::Bus {
            bus_id: bus_id.clone(),
        },
        T::HardwareOutput { device_id, channel } => ProjectTrackOutputRouting::HardwareOutput {
            device_id: device_id.clone(),
            channel: *channel,
        },
        T::None => ProjectTrackOutputRouting::None,
    }
}

fn timeline_audio_format_to_project(
    audio_format: crate::components::timeline::timeline_state::TrackAudioFormat,
) -> ProjectTrackAudioFormat {
    match audio_format {
        crate::components::timeline::timeline_state::TrackAudioFormat::Mono => {
            ProjectTrackAudioFormat::Mono
        }
        crate::components::timeline::timeline_state::TrackAudioFormat::Stereo => {
            ProjectTrackAudioFormat::Stereo
        }
    }
}

fn timeline_midi_input_to_project(
    input: &crate::components::timeline::timeline_state::TrackMidiInputRouting,
) -> ProjectTrackMidiInputRouting {
    use crate::components::timeline::timeline_state::TrackMidiInputRouting as T;
    match input {
        T::None => ProjectTrackMidiInputRouting::None,
        T::AllInputs => ProjectTrackMidiInputRouting::AllInputs,
        T::MidiDevice { device_id } => ProjectTrackMidiInputRouting::MidiDevice {
            device_id: device_id.clone(),
        },
    }
}

fn project_routing_to_timeline(
    routing: &TrackRouting,
    track_type: TlTrackType,
) -> crate::components::timeline::timeline_state::TrackRoutingState {
    use crate::components::timeline::timeline_state::{
        TrackAudioFormat, TrackInputRouting, TrackMidiInputRouting, TrackOutputRouting,
        TrackRoutingState,
    };
    let mut state = TrackRoutingState::for_track_type(track_type);
    state.input = match &routing.input {
        ProjectTrackInputRouting::None => TrackInputRouting::None,
        ProjectTrackInputRouting::AllInputs => TrackInputRouting::AllInputs,
        ProjectTrackInputRouting::AudioDeviceChannel { device_id, channel } => {
            TrackInputRouting::AudioDeviceChannel {
                device_id: device_id.clone(),
                channel: *channel,
            }
        }
        ProjectTrackInputRouting::AudioDeviceChannels {
            device_id,
            channels,
        } => TrackInputRouting::AudioDeviceChannels {
            device_id: device_id.clone(),
            channels: channels.clone(),
        },
        ProjectTrackInputRouting::MidiDevice { device_id } => TrackInputRouting::MidiDevice {
            device_id: device_id.clone(),
        },
    };
    state.output = match &routing.output {
        ProjectTrackOutputRouting::Main => TrackOutputRouting::Main,
        ProjectTrackOutputRouting::Bus { bus_id } => TrackOutputRouting::Bus {
            bus_id: bus_id.clone(),
        },
        ProjectTrackOutputRouting::HardwareOutput { device_id, channel } => {
            TrackOutputRouting::HardwareOutput {
                device_id: device_id.clone(),
                channel: *channel,
            }
        }
        ProjectTrackOutputRouting::None => TrackOutputRouting::None,
    };
    state.audio_format = match routing.audio_format {
        ProjectTrackAudioFormat::Mono => TrackAudioFormat::Mono,
        ProjectTrackAudioFormat::Stereo => TrackAudioFormat::Stereo,
    };
    state.midi_input = match &routing.midi_input {
        ProjectTrackMidiInputRouting::None => TrackMidiInputRouting::None,
        ProjectTrackMidiInputRouting::AllInputs => TrackMidiInputRouting::AllInputs,
        ProjectTrackMidiInputRouting::MidiDevice { device_id } => {
            TrackMidiInputRouting::MidiDevice {
                device_id: device_id.clone(),
            }
        }
    };
    state.midi_channel = routing.midi_channel.map(|ch| ch.clamp(1, 16));
    state
}

/// Flatten an [`AutomationTarget`] into its persisted descriptor.
fn target_to_desc(
    target: &crate::components::timeline::timeline_state::AutomationTarget,
) -> AutomationTargetDesc {
    use crate::components::timeline::timeline_state::AutomationTarget as T;
    let mut desc = AutomationTargetDesc {
        tag: target.to_tag(),
        ..Default::default()
    };
    match target {
        T::PluginParameter {
            insert_id,
            parameter_id,
            parameter_name,
        } => {
            desc.insert_id = insert_id.clone();
            desc.parameter_id = parameter_id.clone();
            desc.parameter_name = parameter_name.clone();
        }
        T::SendLevel { send_id } => desc.send_id = send_id.clone(),
        _ => {}
    }
    desc
}

/// Rebuild an [`AutomationTarget`] from a persisted descriptor. Falls back to
/// deriving from `parameter_name` when the descriptor is from an older file
/// (tag 0 with no plugin/send descriptor strings).
fn desc_to_target(
    desc: &AutomationTargetDesc,
    parameter_name: &str,
) -> crate::components::timeline::timeline_state::AutomationTarget {
    use crate::components::timeline::timeline_state::AutomationTarget as T;
    match desc.tag {
        1 => T::TrackPan,
        2 => T::TrackMute,
        3 => T::PluginParameter {
            insert_id: desc.insert_id.clone(),
            parameter_id: desc.parameter_id.clone(),
            parameter_name: if desc.parameter_name.is_empty() {
                parameter_name.to_string()
            } else {
                desc.parameter_name.clone()
            },
        },
        4 => T::SendLevel {
            send_id: desc.send_id.clone(),
        },
        // tag 0: TrackVolume, or a legacy file — derive from the lane name.
        _ => {
            if desc.insert_id.is_empty() && desc.send_id.is_empty() {
                T::from_legacy_name(parameter_name)
            } else {
                T::TrackVolume
            }
        }
    }
}
