use crate::frame_scheduler::FrameRateMode;
use crate::paths::FutureboardPaths;
use gpui::AppContext;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub use sphere_midi_service::{MidiDeviceDirection, MidiDeviceSetting, MidiHardwareSettings};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectDefaults {
    pub tempo: f64,
    pub time_signature_num: u32,
    pub time_signature_den: u32,
    pub sample_rate: u32,
    pub buffer_size: u32,
    pub tracks_count: u32,
}

impl Default for ProjectDefaults {
    fn default() -> Self {
        Self {
            tempo: 120.0,
            time_signature_num: 4,
            time_signature_den: 4,
            sample_rate: 48000,
            buffer_size: 256,
            tracks_count: 4,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AutosaveSettings {
    pub enabled: bool,
    pub interval_minutes: u32,
    pub max_backups: u32,
}

impl Default for AutosaveSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_minutes: 5,
            max_backups: 10,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NotificationSettings {
    pub enable_warnings: bool,
    pub enable_system_notifications: bool,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            enable_warnings: true,
            enable_system_notifications: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeneralSettings {
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_true")]
    pub show_start_screen: bool,
    #[serde(default = "default_true")]
    pub check_updates: bool,
    /// User-configured default directory for new projects. When `None` or empty
    /// the platform default ([`crate::project::io::default_projects_dir`]) is
    /// used. Resolve through [`GeneralSettings::resolved_default_project_dir`].
    #[serde(default)]
    pub default_project_directory: Option<PathBuf>,
    #[serde(default)]
    pub project_defaults: ProjectDefaults,
    #[serde(default)]
    pub autosave: AutosaveSettings,
    #[serde(default)]
    pub notifications: NotificationSettings,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            language: default_language(),
            show_start_screen: default_true(),
            check_updates: default_true(),
            default_project_directory: None,
            project_defaults: ProjectDefaults::default(),
            autosave: AutosaveSettings::default(),
            notifications: NotificationSettings::default(),
        }
    }
}

impl GeneralSettings {
    /// Resolve the effective default project directory. Falls back to the
    /// platform default when unset, empty, or whitespace-only. Never panics.
    pub fn resolved_default_project_dir(&self) -> PathBuf {
        match &self.default_project_directory {
            Some(path) if !path.as_os_str().is_empty() => path.clone(),
            _ => crate::project::io::default_projects_dir(),
        }
    }

    /// True when the user has explicitly configured a default project directory
    /// (as opposed to relying on the platform fallback).
    pub fn has_configured_project_dir(&self) -> bool {
        self.default_project_directory
            .as_ref()
            .is_some_and(|p| !p.as_os_str().is_empty())
    }
}

fn default_language() -> String {
    "en".to_string()
}

fn default_true() -> bool {
    true
}

fn default_audio_driver_type() -> String {
    #[cfg(target_os = "windows")]
    {
        "WASAPI Shared".to_string()
    }
    #[cfg(target_os = "macos")]
    {
        "CoreAudio".to_string()
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        "ALSA".to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AudioHardwareSettings {
    pub driver_type: String,
    pub device_in: String,
    pub device_out: String,
    pub active_inputs: Vec<u32>,
    pub active_outputs: Vec<u32>,
}

impl Default for AudioHardwareSettings {
    fn default() -> Self {
        Self {
            driver_type: default_audio_driver_type(),
            device_in: "System Default Input".to_string(),
            device_out: "System Default Output".to_string(),
            active_inputs: vec![0, 1],
            active_outputs: vec![0, 1],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SyncSettings {
    pub clock_source: String,
    pub ltc_enabled: bool,
}

impl Default for SyncSettings {
    fn default() -> Self {
        Self {
            clock_source: "Internal".to_string(),
            ltc_enabled: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct HardwareSettings {
    #[serde(default)]
    pub audio: AudioHardwareSettings,
    #[serde(default)]
    pub midi: MidiHardwareSettings,
    #[serde(default)]
    pub control_surfaces: Vec<String>,
    #[serde(default)]
    pub sync: SyncSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArrangementAppearanceSettings {
    pub grid_line_intensity: f32,
    pub clip_color_mode: String,
}

impl Default for ArrangementAppearanceSettings {
    fn default() -> Self {
        Self {
            grid_line_intensity: 0.4,
            clip_color_mode: "TrackAccent".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PianoRollAppearanceSettings {
    pub show_key_guides: bool,
}

impl Default for PianoRollAppearanceSettings {
    fn default() -> Self {
        Self {
            show_key_guides: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MixerAppearanceSettings {
    pub meter_decay_db_per_sec: f32,
    pub peak_hold_seconds: f32,
}

impl Default for MixerAppearanceSettings {
    fn default() -> Self {
        Self {
            meter_decay_db_per_sec: 24.0,
            peak_hold_seconds: 3.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppearanceSettings {
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_ui_scale")]
    pub ui_scale: f32,
    #[serde(default)]
    pub arrangement: ArrangementAppearanceSettings,
    #[serde(default)]
    pub piano_roll: PianoRollAppearanceSettings,
    #[serde(default)]
    pub mixer: MixerAppearanceSettings,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            ui_scale: default_ui_scale(),
            arrangement: ArrangementAppearanceSettings::default(),
            piano_roll: PianoRollAppearanceSettings::default(),
            mixer: MixerAppearanceSettings::default(),
        }
    }
}

fn default_theme() -> String {
    "futureboard.default".to_string()
}

fn default_ui_scale() -> f32 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MouseEditingSettings {
    pub zoom_sensitivity: f32,
    pub natural_scroll: bool,
}

impl Default for MouseEditingSettings {
    fn default() -> Self {
        Self {
            zoom_sensitivity: 1.0,
            natural_scroll: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SnapEditingSettings {
    pub snap_to_grid: bool,
    pub default_snap_value: String,
}

impl Default for SnapEditingSettings {
    fn default() -> Self {
        Self {
            snap_to_grid: true,
            default_snap_value: "1/16".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HistoryEditingSettings {
    pub max_undo_steps: u32,
}

impl Default for HistoryEditingSettings {
    fn default() -> Self {
        Self {
            max_undo_steps: 100,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct EditingSettings {
    #[serde(default)]
    pub mouse: MouseEditingSettings,
    #[serde(default)]
    pub snap: SnapEditingSettings,
    #[serde(default)]
    pub history: HistoryEditingSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AudioRecordingSettings {
    pub format: String,
    pub bit_depth: u32,
    pub recording_path: String,
    #[serde(default)]
    pub recording_offset_ms: i32,
    #[serde(default = "default_true")]
    pub save_before_recording: bool,
    #[serde(default = "default_true")]
    pub generate_waveform_after_record: bool,
}

impl Default for AudioRecordingSettings {
    fn default() -> Self {
        Self {
            format: "wav".to_string(),
            bit_depth: 24,
            recording_path: String::new(),
            recording_offset_ms: 0,
            save_before_recording: true,
            generate_waveform_after_record: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DefaultMonitorMode {
    Off,
    Auto,
    Input,
}

impl Default for DefaultMonitorMode {
    fn default() -> Self {
        Self::Off
    }
}

impl DefaultMonitorMode {
    pub fn add_track_value(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Auto => "auto",
            Self::Input => "input",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetronomeSettings {
    pub enabled: bool,
    pub volume: f32,
    pub sound_type: String,
    pub count_in_bars: u32,
}

impl Default for MetronomeSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            volume: 0.8,
            sound_type: "Woodblock".to_string(),
            count_in_bars: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct RecordingSettings {
    #[serde(default)]
    pub audio: AudioRecordingSettings,
    #[serde(default)]
    pub default_monitor_mode: DefaultMonitorMode,
    #[serde(default)]
    pub metronome: MetronomeSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Vst3PluginSettings {
    pub enabled: bool,
    pub paths: Vec<String>,
}

impl Default for Vst3PluginSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            paths: vec![
                "C:\\Program Files\\Common Files\\VST3".to_string(),
                "C:\\Program Files (x86)\\Common Files\\VST3".to_string(),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClapPluginSettings {
    pub enabled: bool,
    pub paths: Vec<String>,
}

impl Default for ClapPluginSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            paths: vec!["C:\\Program Files\\Common Files\\CLAP".to_string()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScanPluginSettings {
    pub background_scan: bool,
    pub failed_plugins: Vec<String>,
}

impl Default for ScanPluginSettings {
    fn default() -> Self {
        Self {
            background_scan: true,
            failed_plugins: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct PluginsSettings {
    #[serde(default)]
    pub vst3: Vst3PluginSettings,
    #[serde(default)]
    pub clap: ClapPluginSettings,
    #[serde(default)]
    pub scan: ScanPluginSettings,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RenderMode {
    /// Let Futureboard choose the safe accelerated default per surface.
    Auto,
    /// Opt into the experimental GPU primitive layer (batched `canvas` paint).
    GpuAcceleration,
    /// Force the legacy CPU/`div` rendering path everywhere.
    CpuRender,
}

impl Default for RenderMode {
    fn default() -> Self {
        RenderMode::Auto
    }
}

impl RenderMode {
    pub fn label(self) -> &'static str {
        match self {
            RenderMode::Auto => "Auto",
            RenderMode::GpuAcceleration => "GPU (Experimental)",
            RenderMode::CpuRender => "CPU Render",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "value")]
pub enum GpuDevicePreference {
    Auto,
    DeviceId(String),
}

impl Default for GpuDevicePreference {
    fn default() -> Self {
        GpuDevicePreference::Auto
    }
}

impl GpuDevicePreference {
    /// Stable string id for matching against detected GPU adapter ids.
    pub fn id(&self) -> &str {
        match self {
            GpuDevicePreference::Auto => "auto",
            GpuDevicePreference::DeviceId(id) => id.as_str(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct PerformanceSettings {
    #[serde(default)]
    pub render_mode: RenderMode,
    #[serde(default)]
    pub gpu_device: GpuDevicePreference,
    /// Frame pacing mode. `DisplaySync` (default) tracks the monitor refresh
    /// rate; the fixed/battery modes are caps for debug/low-power. Overridable
    /// at runtime via `FUTUREBOARD_FRAME_RATE_MODE`.
    #[serde(default)]
    pub frame_rate: FrameRateMode,
    /// Compact FPS / frame-time pill in the status bar (View → Developer).
    #[serde(default)]
    pub show_status_performance_metrics: bool,
    /// Floating verbose performance overlay (View → Developer).
    #[serde(default)]
    pub show_performance_overlay: bool,
}

/// Dropout Protection mode (Settings → Playback). Keeps internal headroom
/// against control/UI/plugin jitter, independent of the device buffer size.
/// Maps 1:1 to `DirectAudio::DropoutProtectionMode`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DropoutProtectionMode {
    /// Lowest latency, minimal safety margin.
    Off,
    /// Small safety margin / conservative scheduling.
    Light,
    /// Default recommended mode — better protection during UI activity.
    Medium,
    /// Maximum stability (may add internal latency).
    High,
}

impl Default for DropoutProtectionMode {
    fn default() -> Self {
        DropoutProtectionMode::Medium
    }
}

impl DropoutProtectionMode {
    pub fn label(self) -> &'static str {
        match self {
            DropoutProtectionMode::Off => "Off",
            DropoutProtectionMode::Light => "Light",
            DropoutProtectionMode::Medium => "Medium (Recommended)",
            DropoutProtectionMode::High => "High",
        }
    }

    pub const ALL: [DropoutProtectionMode; 4] = [
        DropoutProtectionMode::Off,
        DropoutProtectionMode::Light,
        DropoutProtectionMode::Medium,
        DropoutProtectionMode::High,
    ];
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlaybackSettings {
    /// Align parallel track paths at the master bus (Phase W PDC).
    #[serde(default = "default_true")]
    pub latency_compensation: bool,
    /// Realtime dropout protection mode.
    #[serde(default)]
    pub dropout_protection: DropoutProtectionMode,
}

impl Default for PlaybackSettings {
    fn default() -> Self {
        Self {
            latency_compensation: default_true(),
            dropout_protection: DropoutProtectionMode::default(),
        }
    }
}

/// Live latency readout shown in Settings → Audio (polled from DirectAudio).
#[derive(Debug, Clone, Default)]
pub struct SettingsAudioLatencySnapshot {
    pub engine_open: bool,
    pub device_state: String,
    pub backend_name: String,
    pub last_error: Option<String>,
    /// Active runtime sample rate (Hz) — the rate the opened stream runs at.
    /// All timing uses this; the status bar shows this.
    pub active_sample_rate: u32,
    /// Sample rate requested at device open (Hz), or 0 for "device default".
    /// When this differs from `active_sample_rate` the UI warns that timing
    /// follows the active device rate.
    pub requested_sample_rate: u32,
    /// `true` when the user changed the preferred sample rate but chose "Later"
    /// (deferred the engine restart). Timing keeps using `active_sample_rate`
    /// until the project is re-opened / the engine restarted.
    pub restart_pending: bool,
    /// The deferred preferred sample rate (Hz) when `restart_pending`; 0 otherwise.
    pub deferred_sample_rate: u32,
    pub buffer_ms: f64,
    pub buffer_frames: u32,
    pub round_trip_ms: f64,
    pub max_path_ms: f64,
    pub max_path_samples: u32,
    pub master_plugin_ms: f64,
    pub pdc_active: bool,
    pub track_lines: Vec<SettingsTrackLatencyLine>,
}

#[derive(Debug, Clone, Default)]
pub struct SettingsTrackLatencyLine {
    pub track_id: String,
    pub plugin_ms: f64,
    pub path_ms: f64,
    pub pdc_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SettingsSchema {
    #[serde(default)]
    pub general: GeneralSettings,
    #[serde(default)]
    pub hardware: HardwareSettings,
    #[serde(default)]
    pub appearance: AppearanceSettings,
    #[serde(default)]
    pub editing: EditingSettings,
    #[serde(default)]
    pub recording: RecordingSettings,
    #[serde(default)]
    pub playback: PlaybackSettings,
    #[serde(default)]
    pub plugins: PluginsSettings,
    #[serde(default)]
    pub performance: PerformanceSettings,
}

impl SettingsSchema {
    /// Load and validate the settings schema directly from the resolved
    /// settings file. Useful for surfaces (e.g. the Welcome window) that run
    /// before the [`GlobalSettingsModel`] entity exists. Falls back to defaults
    /// on any read/parse error — never panics.
    pub fn load_from_disk() -> Self {
        let path = FutureboardPaths::resolve().settings_file;
        let mut schema = SettingsModel::load_from_path(&path);
        schema.validate_and_clamp();
        schema
    }

    /// Persist only the default project directory back to the settings file,
    /// preserving every other field already on disk. Intended for the Welcome
    /// window, which has no [`SettingsModel`] entity. Best-effort; logs on
    /// failure and never panics.
    pub fn persist_default_project_directory(dir: Option<PathBuf>) {
        let path = FutureboardPaths::resolve().settings_file;
        let mut schema = SettingsModel::load_from_path(&path);
        schema.general.default_project_directory = dir.filter(|p| !p.as_os_str().is_empty());
        schema.validate_and_clamp();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match serde_json::to_string_pretty(&schema) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, json) {
                    eprintln!("[settings] failed to persist default project dir: {e}");
                }
            }
            Err(e) => eprintln!("[settings] failed to serialize settings: {e}"),
        }
    }

    pub fn validate_and_clamp(&mut self) {
        // Clamp tempo
        if self.general.project_defaults.tempo < 20.0 {
            self.general.project_defaults.tempo = 20.0;
        } else if self.general.project_defaults.tempo > 999.0 {
            self.general.project_defaults.tempo = 999.0;
        }

        // Clamp sample rate
        let sr = self.general.project_defaults.sample_rate;
        if sr != 44100 && sr != 48000 && sr != 88200 && sr != 96000 && sr != 192000 {
            self.general.project_defaults.sample_rate = 48000;
        }

        // Clamp buffer size (power of two between 32 and 4096)
        let buf = self.general.project_defaults.buffer_size;
        let is_valid_buf = buf >= 32 && buf <= 4096 && (buf & (buf - 1)) == 0;
        if !is_valid_buf {
            self.general.project_defaults.buffer_size = 256;
        }

        // Clamp ui scale
        if self.appearance.ui_scale < 0.5 {
            self.appearance.ui_scale = 0.5;
        } else if self.appearance.ui_scale > 2.5 {
            self.appearance.ui_scale = 2.5;
        }

        sphere_midi_service::migrate_legacy_midi_settings(&mut self.hardware.midi);
    }
}

impl SettingsAudioLatencySnapshot {
    pub fn from_engine(engine: &DirectAudio::AudioEngine) -> Self {
        let stats = engine.stats();
        let info = engine.latency_info();
        let mut track_lines: Vec<SettingsTrackLatencyLine> = info
            .tracks
            .iter()
            .filter(|track| track.plugin_samples > 0 || track.path_samples > 0)
            .map(|track| SettingsTrackLatencyLine {
                track_id: track.track_id.clone(),
                plugin_ms: track.plugin_ms,
                path_ms: track.path_ms,
                pdc_ms: track.pdc_delay_ms,
            })
            .collect();
        track_lines.sort_by(|a, b| {
            b.path_ms
                .partial_cmp(&a.path_ms)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        track_lines.truncate(6);

        Self {
            engine_open: stats.stream_open,
            device_state: stats.device_state,
            backend_name: stats.backend_name,
            last_error: stats.last_error,
            active_sample_rate: stats.sample_rate,
            requested_sample_rate: stats.requested_sample_rate,
            // Overridden by the latency provider, which knows the deferred target.
            restart_pending: false,
            deferred_sample_rate: 0,
            buffer_ms: info.buffer_ms,
            buffer_frames: info.buffer_frames,
            round_trip_ms: info.buffer_ms * 2.0,
            max_path_ms: info.max_path_ms,
            max_path_samples: info.max_path_samples,
            master_plugin_ms: info.master_ms,
            pdc_active: info.pdc_enabled,
            track_lines,
        }
    }

    pub fn unavailable() -> Self {
        Self::default()
    }
}

pub struct SettingsModel {
    pub current: SettingsSchema,
    pub path: PathBuf,
}

impl SettingsModel {
    pub fn load_or_create(cx: &mut gpui::App) -> gpui::Entity<Self> {
        let path = FutureboardPaths::resolve().settings_file;
        let mut settings = Self::load_from_path(&path);
        settings.validate_and_clamp();

        cx.new(|_cx| Self {
            current: settings,
            path,
        })
    }

    pub fn load_from_path(path: &Path) -> SettingsSchema {
        if path.exists() {
            match std::fs::read_to_string(path) {
                Ok(content) => match serde_json::from_str::<SettingsSchema>(&content) {
                    Ok(schema) => schema,
                    Err(e) => {
                        eprintln!("[settings] failed to parse settings.json: {e}. backing up.");
                        let backup_path = path.with_extension("json.backup");
                        let _ = std::fs::rename(path, &backup_path);
                        let default_schema = SettingsSchema::default();
                        if let Ok(json) = serde_json::to_string_pretty(&default_schema) {
                            let _ = std::fs::write(path, json);
                        }
                        default_schema
                    }
                },
                Err(e) => {
                    eprintln!("[settings] failed to read settings.json: {e}");
                    SettingsSchema::default()
                }
            }
        } else {
            let default_schema = SettingsSchema::default();
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(json) = serde_json::to_string_pretty(&default_schema) {
                let _ = std::fs::write(path, json);
            }
            default_schema
        }
    }

    pub fn save_to_disk(&self) {
        let path = self.path.clone();
        let schema = self.current.clone();

        std::thread::spawn(move || {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            match serde_json::to_string_pretty(&schema) {
                Ok(json) => {
                    if let Err(e) = std::fs::write(&path, json) {
                        eprintln!("[settings] failed to write settings file: {e}");
                    }
                }
                Err(e) => {
                    eprintln!("[settings] failed to serialize settings: {e}");
                }
            }
        });
    }

    pub fn update_setting<F>(&mut self, updater: F, cx: &mut gpui::Context<Self>)
    where
        F: FnOnce(&mut SettingsSchema),
    {
        updater(&mut self.current);
        self.current.validate_and_clamp();
        self.save_to_disk();
        cx.notify();
    }
}

pub struct GlobalSettingsModel(pub gpui::Entity<SettingsModel>);

impl gpui::Global for GlobalSettingsModel {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_settings() {
        let schema = SettingsSchema::default();
        assert_eq!(schema.general.language, "en");
        assert_eq!(schema.general.project_defaults.tempo, 120.0);
        assert_eq!(schema.general.project_defaults.sample_rate, 48000);
        assert_eq!(schema.general.project_defaults.buffer_size, 256);
        assert_eq!(schema.appearance.ui_scale, 1.0);
    }

    #[test]
    fn test_validation_clamping() {
        let mut schema = SettingsSchema::default();

        // Invalid tempo
        schema.general.project_defaults.tempo = 10.0;
        schema.validate_and_clamp();
        assert_eq!(schema.general.project_defaults.tempo, 20.0);

        schema.general.project_defaults.tempo = 1200.0;
        schema.validate_and_clamp();
        assert_eq!(schema.general.project_defaults.tempo, 999.0);

        // Invalid sample rate
        schema.general.project_defaults.sample_rate = 99999;
        schema.validate_and_clamp();
        assert_eq!(schema.general.project_defaults.sample_rate, 48000);

        // Invalid buffer size
        schema.general.project_defaults.buffer_size = 123;
        schema.validate_and_clamp();
        assert_eq!(schema.general.project_defaults.buffer_size, 256);

        // Invalid UI scale
        schema.appearance.ui_scale = 0.1;
        schema.validate_and_clamp();
        assert_eq!(schema.appearance.ui_scale, 0.5);

        schema.appearance.ui_scale = 5.0;
        schema.validate_and_clamp();
        assert_eq!(schema.appearance.ui_scale, 2.5);
    }

    #[test]
    fn test_corrupt_file_recovery() {
        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join("test_settings_corrupt.json");
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }

        // Write invalid JSON
        std::fs::write(&path, "{ invalid json ... }").unwrap();

        // Load it
        let schema = SettingsModel::load_from_path(&path);

        // Should have backed up and loaded defaults
        assert_eq!(schema.general.project_defaults.tempo, 120.0);

        let backup_path = path.with_extension("json.backup");
        assert!(backup_path.exists());

        // Clean up
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&backup_path);
    }
}
