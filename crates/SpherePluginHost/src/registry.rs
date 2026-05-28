//! Platform-neutral plugin registry types and VST3/CLAP scan for native GPUI.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::preset::{clear_all_presets, ensure_preset_folders, register_plugin};
use crate::scanner::{discover_plugin_bundles, scan_plugin_bundle};
use crate::types::PluginInfo;

/// Plug-in container format (aligned with Electron `AudioPluginRegistryEntry.format`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluginFormat {
    Vst3,
    Clap,
    Au,
    Lv2,
    Unknown,
}

impl PluginFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Vst3 => "VST3",
            Self::Clap => "CLAP",
            Self::Au => "AU",
            Self::Lv2 => "LV2",
            Self::Unknown => "Unknown",
        }
    }

    pub fn from_str_lossy(s: &str) -> Self {
        match s.to_ascii_uppercase().as_str() {
            "VST3" => Self::Vst3,
            "CLAP" => Self::Clap,
            "AU" => Self::Au,
            "LV2" => Self::Lv2,
            _ => Self::Unknown,
        }
    }
}

/// Effect vs instrument (heuristic classification; matches Electron).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginKind {
    Effect,
    Instrument,
}

/// Row status in the plug-in manager list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginStatus {
    /// `.pst` on disk (Electron "Available").
    PresetReady,
    MissingPreset,
}

/// Scanner / registry host readiness (maps `AudioPluginHostStatus`).
#[derive(Debug, Clone)]
pub struct NativeHostStatus {
    pub available: bool,
    pub backend: String,
    pub message: String,
    pub db_path: PathBuf,
    pub preset_root: PathBuf,
    pub default_scan_paths: Vec<PathBuf>,
}

/// One plug-in in the cached registry (maps `AudioPluginRegistryEntry`).
#[derive(Debug, Clone)]
pub struct RegistryPlugin {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub format: PluginFormat,
    pub category: String,
    pub raw_category: Option<String>,
    pub sub_categories: Option<String>,
    pub kind: PluginKind,
    pub path: PathBuf,
    pub class_id: Option<String>,
    pub version: Option<String>,
    pub sdk_metadata_loaded: bool,
    pub preset_path: PathBuf,
    pub scanned_at_ms: i64,
    pub status: PluginStatus,
}

impl RegistryPlugin {
    pub fn display_category(&self) -> String {
        display_category(
            self.format,
            &self.category,
            self.raw_category.as_deref(),
            self.sub_categories.as_deref(),
        )
    }

    /// Insert onto a track is supported when format is wired and preset exists.
    pub fn supports_insert(&self) -> bool {
        matches!(self.format, PluginFormat::Vst3 | PluginFormat::Clap)
            && self.status == PluginStatus::PresetReady
    }

    /// Native editor window (VST3 only today, matching Electron lifecycle).
    pub fn supports_editor(&self) -> bool {
        self.format == PluginFormat::Vst3 && self.supports_insert()
    }
}

/// Normalize category label (Electron `normalizeCategoryLabel` + UI fallback).
pub fn display_category(
    format: PluginFormat,
    category: &str,
    raw_category: Option<&str>,
    sub_categories: Option<&str>,
) -> String {
    let tags: Vec<&str> = sub_categories
        .unwrap_or("")
        .split('|')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .collect();

    let has = |needle: &str| tags.iter().any(|t| t.eq_ignore_ascii_case(needle));

    if format == PluginFormat::Vst3 {
        if has("Instrument") {
            return "Instrument".to_string();
        }
        if has("EQ") {
            return "EQ".to_string();
        }
        if has("Dynamics") {
            return "Dynamics".to_string();
        }
        if has("Reverb") {
            return "Reverb".to_string();
        }
        if has("Delay") {
            return "Delay".to_string();
        }
        if category.eq_ignore_ascii_case("audio module class") {
            return tags
                .iter()
                .find(|t| !t.eq_ignore_ascii_case("fx"))
                .map(|s| (*s).to_string())
                .unwrap_or_else(|| "Effect".to_string());
        }
        if !tags.is_empty() {
            return tags.join("|");
        }
        return category.to_string();
    }

    if format == PluginFormat::Clap {
        let specific: Vec<&str> = tags
            .iter()
            .copied()
            .filter(|t| {
                !matches!(
                    t.to_ascii_lowercase().as_str(),
                    "audio-effect" | "audio effect" | "plugin" | "utility"
                )
            })
            .collect();
        let display_tags: Vec<&str> = if specific.is_empty() {
            tags.clone()
        } else {
            specific
        };
        if display_tags.iter().any(|t| t.eq_ignore_ascii_case("instrument")) {
            return "Instrument".to_string();
        }
        if display_tags.iter().any(|t| t.to_ascii_lowercase().contains("effect")) {
            return "Effect".to_string();
        }
        if category.eq_ignore_ascii_case("audio effect") {
            return "Effect".to_string();
        }
        return display_tags
            .first()
            .map(|s| (*s).to_string())
            .unwrap_or_else(|| category.to_string());
    }

    if let Some(sub) = sub_categories.filter(|s| !s.trim().is_empty()) {
        return sub.trim().to_string();
    }
    if !category.is_empty() {
        return category.to_string();
    }
    raw_category
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "Uncategorized".to_string())
}

pub fn classify_kind(category: &str, name: &str, sub_categories: Option<&str>) -> PluginKind {
    let haystack = format!(
        "{} {} {}",
        category,
        sub_categories.unwrap_or(""),
        name
    )
    .to_ascii_lowercase();
    if haystack.contains("instrument")
        || haystack.contains("synth")
        || haystack.contains("synthesizer")
        || haystack.contains("sampler")
        || haystack.contains("rompler")
        || haystack.contains("drum")
        || haystack.contains("piano")
        || haystack.contains("organ")
        || haystack.contains("bass")
        || haystack.contains("generator")
    {
        PluginKind::Instrument
    } else {
        PluginKind::Effect
    }
}

/// OS-default plug-in scan folders (matches Electron `defaultScanPaths`).
pub fn default_scan_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    #[cfg(target_os = "windows")]
    {
        if let Some(pf) = std::env::var_os("ProgramFiles") {
            let pf = PathBuf::from(pf);
            paths.push(pf.join("Common Files").join("VST3"));
            paths.push(pf.join("Common Files").join("CLAP"));
            paths.push(pf.join("VSTPlugins"));
            paths.push(pf.join("Steinberg").join("VSTPlugins"));
        }
        if let Some(pf86) = std::env::var_os("ProgramFiles(x86)") {
            let pf86 = PathBuf::from(pf86);
            paths.push(pf86.join("Common Files").join("VST3"));
            paths.push(pf86.join("Common Files").join("CLAP"));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            let local = PathBuf::from(local);
            paths.push(local.join("Programs").join("Common").join("VST3"));
            paths.push(local.join("Programs").join("Common").join("CLAP"));
        }
    }
    #[cfg(target_os = "macos")]
    {
        paths.push(PathBuf::from("/Library/Audio/Plug-Ins/VST3"));
        paths.push(PathBuf::from("/Library/Audio/Plug-Ins/CLAP"));
        if let Some(home) = dirs::home_dir() {
            paths.push(home.join("Library/Audio/Plug-Ins/VST3"));
            paths.push(home.join("Library/Audio/Plug-Ins/CLAP"));
        }
    }
    #[cfg(target_os = "linux")]
    {
        paths.push(PathBuf::from("/usr/lib/vst3"));
        paths.push(PathBuf::from("/usr/local/lib/vst3"));
        paths.push(PathBuf::from("/usr/lib/clap"));
        paths.push(PathBuf::from("/usr/local/lib/clap"));
        if let Some(home) = dirs::home_dir() {
            paths.push(home.join(".vst3"));
            paths.push(home.join(".clap"));
        }
    }
    paths
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone)]
pub struct PluginScanFailure {
    pub path: PathBuf,
    pub error: String,
}

#[derive(Debug, Clone)]
pub struct RegistryScanResult {
    pub plugins: Vec<RegistryPlugin>,
    pub scanned_paths: Vec<PathBuf>,
    pub failed: Vec<PluginScanFailure>,
    pub generated_presets: u32,
}

/// Scan job options for the native plug-in manager.
#[derive(Debug, Clone, Default)]
pub struct ScanOptions {
    pub paths: Option<Vec<PathBuf>>,
    /// When true, delete all `.pst` files under the preset root before scanning.
    pub delete_presets_first: bool,
}

/// Incremental scan progress (bundle discovery → metadata → registration).
#[derive(Debug, Clone)]
pub enum ScanProgress {
    Started {
        bundle_total: usize,
    },
    ScanningBundle {
        current: usize,
        total: usize,
        path: PathBuf,
    },
    Registering {
        current: usize,
        total: usize,
        name: String,
        plugin: RegistryPlugin,
        generated_presets: u32,
    },
    Failed {
        path: PathBuf,
        error: String,
    },
}

pub fn default_preset_root() -> PathBuf {
    dirs::document_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Futureboard Studio")
        .join("Audio Plug-ins")
}

fn safe_file_name(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if "<>:\"/\\|?*\x00-\x1f".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(120)
        .collect()
}

fn preset_path_for_plugin(
    preset_root: &Path,
    format: PluginFormat,
    kind: PluginKind,
    name: &str,
) -> PathBuf {
    let fmt_dir = match format {
        PluginFormat::Clap => "CLAP",
        _ => "VST3",
    };
    let kind_dir = match kind {
        PluginKind::Instrument => "Instruments",
        PluginKind::Effect => "Effects",
    };
    preset_root
        .join(fmt_dir)
        .join(kind_dir)
        .join(format!("{}.pst", safe_file_name(name)))
}

fn resolve_unique_preset_path(
    plugin: RegistryPlugin,
    occupied: &mut HashSet<String>,
) -> RegistryPlugin {
    let mut candidate = plugin.preset_path.to_string_lossy().to_string();
    let mut index = 2;
    while occupied.contains(&candidate.to_lowercase()) {
        let parsed = plugin.preset_path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let stem = plugin
            .preset_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Plug-in");
        let ext = plugin.preset_path.extension().and_then(|e| e.to_str()).unwrap_or("pst");
        candidate = parsed
            .join(format!("{stem} ({index}).{ext}"))
            .to_string_lossy()
            .to_string();
        index += 1;
    }
    occupied.insert(candidate.to_lowercase());
    RegistryPlugin {
        preset_path: PathBuf::from(candidate),
        ..plugin
    }
}

fn registry_display_key(plugin: &RegistryPlugin) -> String {
    [
        plugin.vendor.as_str(),
        plugin.name.as_str(),
        plugin.format.label(),
        plugin.category.as_str(),
        match plugin.kind {
            PluginKind::Instrument => "instrument",
            PluginKind::Effect => "effect",
        },
    ]
    .join("|")
    .to_lowercase()
}

/// Build a registry row from a native scan result (`PluginInfo`).
pub fn registry_plugin_from_scan(info: &PluginInfo, scanned_at_ms: i64) -> RegistryPlugin {
    let format = PluginFormat::from_str_lossy(&info.format);
    let raw = info.category.clone();
    let sub = info.sub_categories.clone();
    let category = display_category(format, &raw, Some(&raw), sub.as_deref());
    let kind = classify_kind(&raw, &info.name, sub.as_deref());
    let preset_root = default_preset_root();
    let preset_path = preset_path_for_plugin(&preset_root, format, kind, &info.name);
    let path = PathBuf::from(&info.path);
    let status = if !path.exists() {
        PluginStatus::MissingPreset
    } else if preset_path.exists() {
        PluginStatus::PresetReady
    } else {
        PluginStatus::MissingPreset
    };
    RegistryPlugin {
        id: info.id.clone(),
        name: info.name.clone(),
        vendor: info.vendor.clone(),
        format,
        category,
        raw_category: Some(raw),
        sub_categories: sub,
        kind,
        path,
        class_id: info.class_id.clone(),
        version: info.version.clone(),
        sdk_metadata_loaded: info.sdk_metadata_loaded,
        preset_path,
        scanned_at_ms,
        status,
    }
}

/// Host readiness for the plug-in manager UI.
pub fn native_host_status() -> NativeHostStatus {
    let preset_root = default_preset_root();
    let init = crate::init_plugin_host().ok();
    let (available, backend, message) = if let Some(status) = init {
        (
            status.available,
            status.backend,
            status.message,
        )
    } else {
        (
            false,
            "unavailable".to_string(),
            "SpherePluginHost failed to initialize.".to_string(),
        )
    };
    NativeHostStatus {
        available,
        backend,
        message,
        db_path: dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Futureboard Studio")
            .join("audio-plugin-registry.sqlite"),
        preset_root,
        default_scan_paths: default_scan_paths(),
    }
}

/// Registry service: scan VST3 + CLAP via [`scan_audio_plugin_paths`].
pub struct PluginRegistry;

impl PluginRegistry {
    pub fn host_status() -> NativeHostStatus {
        native_host_status()
    }

    /// Scan default OS paths, or the provided folders (VST3 + CLAP).
    pub fn scan(requested_paths: Option<Vec<PathBuf>>) -> RegistryScanResult {
        Self::scan_with_progress(
            ScanOptions {
                paths: requested_paths,
                delete_presets_first: false,
            },
            |_| {},
        )
    }

    /// Discover bundles, read metadata, validate, and write `.pst` presets with progress callbacks.
    pub fn scan_with_progress(
        options: ScanOptions,
        mut on_progress: impl FnMut(ScanProgress) + Send,
    ) -> RegistryScanResult {
        let requested: Vec<PathBuf> = options
            .paths
            .filter(|p| !p.is_empty())
            .unwrap_or_else(default_scan_paths);

        let mut scanned_paths = Vec::new();
        let mut failed = Vec::new();

        for path in requested {
            if path.exists() {
                scanned_paths.push(path);
            } else {
                failed.push(PluginScanFailure {
                    path: path.clone(),
                    error: "Path does not exist".to_string(),
                });
                on_progress(ScanProgress::Failed {
                    path,
                    error: "Path does not exist".to_string(),
                });
            }
        }

        if options.delete_presets_first {
            if let Err(error) = clear_all_presets() {
                failed.push(PluginScanFailure {
                    path: PathBuf::from("(presets)"),
                    error: format!("Failed to clear presets: {error}"),
                });
            }
        }

        let _ = ensure_preset_folders();

        let bundles = discover_plugin_bundles(&scanned_paths);
        let bundle_total = bundles.len();
        on_progress(ScanProgress::Started { bundle_total });

        let scanned_at = now_ms();
        let mut pending = Vec::new();
        let mut seen = HashSet::new();
        let mut occupied_presets = HashSet::new();

        for (index, bundle) in bundles.iter().enumerate() {
            on_progress(ScanProgress::ScanningBundle {
                current: index + 1,
                total: bundle_total.max(1),
                path: bundle.clone(),
            });

            match scan_plugin_bundle(bundle) {
                Ok(infos) => {
                    for info in infos {
                        let mut plugin = registry_plugin_from_scan(&info, scanned_at);
                        let key = registry_display_key(&plugin);
                        if !seen.insert(key) {
                            continue;
                        }
                        plugin = resolve_unique_preset_path(plugin, &mut occupied_presets);
                        pending.push(plugin);
                    }
                }
                Err(error) => {
                    failed.push(PluginScanFailure {
                        path: bundle.clone(),
                        error: error.clone(),
                    });
                    on_progress(ScanProgress::Failed {
                        path: bundle.clone(),
                        error,
                    });
                }
            }
        }

        let register_total = pending.len();
        let mut plugins = Vec::with_capacity(register_total);
        let mut generated_presets = 0u32;

        for (index, mut plugin) in pending.into_iter().enumerate() {
            let current = index + 1;
            match register_plugin(&mut plugin) {
                Ok(()) => {
                    generated_presets += 1;
                }
                Err(error) => {
                    failed.push(PluginScanFailure {
                        path: plugin.path.clone(),
                        error: error.clone(),
                    });
                    on_progress(ScanProgress::Failed {
                        path: plugin.path.clone(),
                        error,
                    });
                }
            }

            plugins.push(plugin.clone());
            on_progress(ScanProgress::Registering {
                current,
                total: register_total.max(1),
                name: plugin.name.clone(),
                plugin,
                generated_presets,
            });
        }

        plugins.sort_by(|a, b| {
            let kind = match (a.kind, b.kind) {
                (PluginKind::Instrument, PluginKind::Effect) => std::cmp::Ordering::Less,
                (PluginKind::Effect, PluginKind::Instrument) => std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Equal,
            };
            kind.then_with(|| a.vendor.cmp(&b.vendor))
                .then_with(|| a.name.cmp(&b.name))
        });

        RegistryScanResult {
            plugins,
            scanned_paths,
            failed,
            generated_presets,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vst3_instrument_category_normalization() {
        let cat = display_category(
            PluginFormat::Vst3,
            "Audio Module Class",
            Some("Audio Module Class"),
            Some("Instrument|Synth"),
        );
        assert_eq!(cat, "Instrument");
    }
}
