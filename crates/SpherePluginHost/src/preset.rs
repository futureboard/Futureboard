//! Futureboard `.pst` preset files (FBPST format, aligned with Electron `PluginHostNative`).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::registry::{default_preset_root, RegistryPlugin};

const PRESET_MAGIC: &[u8; 5] = b"FBPST";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PresetMetadata<'a> {
    preset_format: &'static str,
    version: u32,
    created_at: i64,
    plugin_metadata: PresetPluginMetadata<'a>,
    plugin_state: PresetPluginState,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PresetPluginMetadata<'a> {
    id: &'a str,
    name: &'a str,
    vendor: &'a str,
    format: &'a str,
    category: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    raw_category: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sub_categories: Option<&'a str>,
    kind: &'a str,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    class_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<&'a str>,
    sdk_metadata_loaded: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PresetPluginState {
    encoding: &'static str,
    byte_length: u32,
    source: &'static str,
}

/// Ensure `Documents/Futureboard Studio/Audio Plug-ins/{VST3,CLAP}/{Instruments,Effects}` exist.
pub fn ensure_preset_folders() -> Result<(), String> {
    for folder in preset_subfolders() {
        fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn preset_subfolders() -> Vec<PathBuf> {
    let root = default_preset_root();
    [
        "VST3/Instruments",
        "VST3/Effects",
        "CLAP/Instruments",
        "CLAP/Effects",
    ]
    .into_iter()
    .map(|rel| root.join(rel))
    .collect()
}

/// Delete every `.pst` under the preset root. Returns number of files removed.
pub fn clear_all_presets() -> Result<u32, String> {
    let root = default_preset_root();
    if !root.exists() {
        return Ok(0);
    }
    clear_pst_files_recursive(&root)
}

fn clear_pst_files_recursive(dir: &Path) -> Result<u32, String> {
    let mut deleted = 0u32;
    let entries = fs::read_dir(dir).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            deleted = deleted.saturating_add(clear_pst_files_recursive(&path)?);
        } else if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("pst"))
        {
            fs::remove_file(&path).map_err(|e| e.to_string())?;
            deleted += 1;
        }
    }
    Ok(deleted)
}

/// Validate that a plug-in binary exists before registration.
pub fn validate_plugin_for_registration(plugin: &RegistryPlugin) -> Result<(), String> {
    if !plugin.path.exists() {
        return Err(format!(
            "Plug-in binary is missing: {}",
            plugin.path.display()
        ));
    }
    Ok(())
}

/// Write `.pst` for this registry row (does not change [`RegistryPlugin::status`]).
pub fn write_preset(plugin: &RegistryPlugin) -> Result<(), String> {
    validate_plugin_for_registration(plugin)?;
    ensure_preset_folders()?;

    if let Some(parent) = plugin.preset_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let bytes = build_preset_binary(plugin);
    let tmp = plugin.preset_path.with_extension("pst.tmp");
    {
        let mut file = fs::File::create(&tmp).map_err(|e| e.to_string())?;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    fs::rename(&tmp, &plugin.preset_path).map_err(|e| e.to_string())?;
    Ok(())
}

/// Validate and write `.pst`; marks the row as [`crate::registry::PluginStatus::PresetReady`].
pub fn register_plugin(plugin: &mut RegistryPlugin) -> Result<(), String> {
    write_preset(plugin)?;
    plugin.status = crate::registry::PluginStatus::PresetReady;
    Ok(())
}

fn build_preset_binary(plugin: &RegistryPlugin) -> Vec<u8> {
    let kind = match plugin.kind {
        crate::registry::PluginKind::Instrument => "instrument",
        crate::registry::PluginKind::Effect => "effect",
    };
    let metadata = PresetMetadata {
        preset_format: "Mochi preset: Futureboard",
        version: 1,
        created_at: plugin.scanned_at_ms,
        plugin_metadata: PresetPluginMetadata {
            id: &plugin.id,
            name: &plugin.name,
            vendor: &plugin.vendor,
            format: plugin.format.label(),
            category: &plugin.category,
            raw_category: plugin.raw_category.as_deref(),
            sub_categories: plugin.sub_categories.as_deref(),
            kind,
            path: plugin.path.display().to_string(),
            class_id: plugin.class_id.as_deref(),
            version: plugin.version.as_deref(),
            sdk_metadata_loaded: plugin.sdk_metadata_loaded,
        },
        plugin_state: PresetPluginState {
            encoding: "binary",
            byte_length: 0,
            source: "pending-native-instantiation",
        },
    };

    let meta = serde_json::to_vec(&metadata).unwrap_or_default();
    let mut header = [0u8; 24];
    header[..5].copy_from_slice(PRESET_MAGIC);
    header[6..8].copy_from_slice(&1u16.to_le_bytes());
    header[8..12].copy_from_slice(&(meta.len() as u32).to_le_bytes());
    header[12..16].copy_from_slice(&0u32.to_le_bytes());

    let mut out = Vec::with_capacity(24 + meta.len());
    out.extend_from_slice(&header);
    out.extend_from_slice(&meta);
    out
}
