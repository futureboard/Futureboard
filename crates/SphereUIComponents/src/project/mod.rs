pub mod format;
pub mod io;
pub mod recent;

pub use format::{decode_project, encode_project, ProjectError, PROJECT_MAGIC, PROJECT_VERSION};
pub use io::{
    create_project_folder, default_projects_dir, load_project, sanitize_project_name, save_project,
    PROJECT_FILE_EXT,
};
pub use recent::{RecentProject, RecentProjectsStore};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMonitorMode {
    Off,
    Always,
    WhenRecordArmed,
}

#[derive(Debug, Clone)]
pub enum ClipSource {
    Audio {
        asset_id: String,
        source_path: Option<PathBuf>,
    },
    Midi {
        notes: Vec<MidiNote>,
    },
    Empty,
}

#[derive(Debug, Clone)]
pub struct MidiNote {
    pub pitch: u8,
    pub start_beats: f32,
    pub duration_beats: f32,
    pub velocity: u8,
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

#[derive(Debug, Clone)]
pub struct TrackRouting {
    pub output_bus: Option<String>,
    pub sends: Vec<ProjectSend>,
}

impl Default for TrackRouting {
    fn default() -> Self {
        Self {
            output_bus: None,
            sends: Vec::new(),
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
}

#[derive(Debug, Clone)]
pub struct AutomationLane {
    pub id: String,
    pub parameter_name: String,
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
}

// ── Mixer ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ProjectMixer {
    pub master_volume_norm: f32,
}

impl Default for ProjectMixer {
    fn default() -> Self {
        Self {
            master_volume_norm: 0.818,
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
}

// ── Settings ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ProjectSettings {
    pub bpm: f64,
    pub time_sig_num: u32,
    pub time_sig_den: u32,
    pub sample_rate: u32,
    pub bit_depth: u32,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            bpm: 120.0,
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
pub fn rgba_to_hex(c: gpui::Rgba) -> String {
    let r = (c.r * 255.0).round() as u8;
    let g = (c.g * 255.0).round() as u8;
    let b = (c.b * 255.0).round() as u8;
    format!("#{:02X}{:02X}{:02X}", r, g, b)
}

/// Converts a hex color string "#RRGGBB" to `gpui::Rgba`.
pub fn hex_to_rgba(hex: &str) -> gpui::Rgba {
    let s = hex.trim_start_matches('#');
    let r = u8::from_str_radix(&s.get(0..2).unwrap_or("56"), 16).unwrap_or(0x56);
    let g = u8::from_str_radix(&s.get(2..4).unwrap_or("C7"), 16).unwrap_or(0xC7);
    let b = u8::from_str_radix(&s.get(4..6).unwrap_or("C9"), 16).unwrap_or(0xC9);
    gpui::rgba(((r as u32) << 24) | ((g as u32) << 16) | ((b as u32) << 8) | 0xFF)
}

// ── From TimelineState ────────────────────────────────────────────────────────

use crate::components::timeline::timeline_state::{
    ClipType, TimelineState, TrackType as TlTrackType,
};

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
                            } => ClipSource::Audio {
                                asset_id: file_id.clone(),
                                source_path: source_path.as_deref().map(PathBuf::from),
                            },
                            ClipType::Midi { notes } => ClipSource::Midi {
                                notes: notes
                                    .iter()
                                    .map(|n| MidiNote {
                                        pitch: n.pitch,
                                        start_beats: n.start,
                                        duration_beats: n.duration,
                                        velocity: n.velocity,
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
                        }
                    })
                    .collect();
                let automation_lanes = t
                    .automation_lanes
                    .iter()
                    .map(|al| AutomationLane {
                        id: al.id.clone(),
                        parameter_name: al.name.clone(),
                        points: al
                            .points
                            .iter()
                            .map(|p| AutomationPoint {
                                beat: p.beat,
                                value: p.value,
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
                    input_monitor: if t.input_monitor {
                        InputMonitorMode::Always
                    } else {
                        InputMonitorMode::Off
                    },
                    routing: TrackRouting {
                        output_bus: None,
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
                        .map(|(idx, slot)| {
                            use crate::components::timeline::timeline_state::InsertPluginFormat;
                            let plugin = slot.plugin_id.as_ref().map(|pid| {
                                ProjectPluginInstance {
                                    instance_id: slot.id.clone(),
                                    format: match slot.plugin_format {
                                        Some(InsertPluginFormat::Vst3) => PluginFormat::Vst3,
                                        Some(InsertPluginFormat::Clap) => PluginFormat::Clap,
                                        Some(InsertPluginFormat::Au) => PluginFormat::Au,
                                        Some(InsertPluginFormat::Lv2) => PluginFormat::Lv2,
                                        _ => PluginFormat::Unknown,
                                    },
                                    plugin_path: slot.plugin_path.clone(),
                                    plugin_uid: pid.clone(),
                                    display_name: slot.display_name.clone(),
                                    state: PluginStateBlob::default(),
                                }
                            });
                            ProjectInsert {
                                id: slot.id.clone(),
                                slot_index: idx as u32,
                                bypassed: slot.bypassed,
                                plugin,
                            }
                        })
                        .collect(),
                    automation_lanes,
                    clips,
                }
            })
            .collect();
        let mut project = FutureboardProject::new("Untitled Project");
        project.settings.bpm = tl.bpm as f64;
        project.settings.time_sig_num = tl.time_signature_num;
        project.settings.time_sig_den = tl.time_signature_den;
        project.tracks = tracks;
        project.mixer.master_volume_norm = tl.master.volume;
        project
    }
}

/// Apply a loaded `FutureboardProject` back onto an existing `TimelineState`.
pub fn apply_to_timeline(project: &FutureboardProject, tl: &mut TimelineState) {
    use crate::components::timeline::timeline_state::{
        AutomationLaneState, AutomationPoint as TlAutoPoint, ClipState, MidiNoteState, SendSlotState,
        TrackState,
    };

    tl.bpm = project.settings.bpm as f32;
    tl.time_signature_num = project.settings.time_sig_num;
    tl.time_signature_den = project.settings.time_sig_den;
    tl.master.volume = project.mixer.master_volume_norm;

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
                        ClipSource::Midi { notes } => ClipType::Midi {
                            notes: notes
                                .iter()
                                .map(|n| MidiNoteState::new(
                                    n.pitch,
                                    n.start_beats,
                                    n.duration_beats,
                                    n.velocity,
                                ))
                                .collect(),
                        },
                        ClipSource::Empty => ClipType::Midi { notes: Vec::new() },
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
                    }
                })
                .collect();
            let automation_lanes = pt
                .automation_lanes
                .iter()
                .map(|al| AutomationLaneState {
                    id: al.id.clone(),
                    name: al.parameter_name.clone(),
                    visible: al.visible,
                    points: al
                        .points
                        .iter()
                        .map(|p| TlAutoPoint {
                            beat: p.beat,
                            value: p.value,
                        })
                        .collect(),
                })
                .collect();
            let inserts = pt
                .inserts
                .iter()
                .map(|pi| {
                    use crate::components::timeline::timeline_state::{
                        InsertLoadStatus, InsertPluginFormat, InsertSlotState,
                    };
                    match &pi.plugin {
                        Some(plugin) => InsertSlotState {
                            id: pi.id.clone(),
                            plugin_id: Some(plugin.plugin_uid.clone()),
                            plugin_path: plugin.plugin_path.clone(),
                            plugin_format: Some(match plugin.format {
                                PluginFormat::Vst3 => InsertPluginFormat::Vst3,
                                PluginFormat::Clap => InsertPluginFormat::Clap,
                                PluginFormat::Au => InsertPluginFormat::Au,
                                PluginFormat::Lv2 => InsertPluginFormat::Lv2,
                                PluginFormat::Unknown => InsertPluginFormat::Unknown,
                            }),
                            display_name: plugin.display_name.clone(),
                            enabled: true,
                            bypassed: pi.bypassed,
                            load_status: InsertLoadStatus::Ready,
                            parameters: Vec::new(),
                        },
                        None => InsertSlotState::empty(pi.id.clone()),
                    }
                })
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
            TrackState {
                id: pt.id.clone(),
                name: pt.name.clone(),
                track_type,
                color: hex_to_rgba(&pt.color_hex),
                volume: pt.volume_norm,
                pan: pt.pan,
                muted: pt.muted,
                solo: pt.solo,
                armed: pt.record_arm,
                input_monitor: pt.input_monitor == InputMonitorMode::Always,
                meter_level_l: 0.0,
                meter_level_r: 0.0,
                clips,
                automation_lanes,
                inserts,
                sends,
            }
        })
        .collect();
}
