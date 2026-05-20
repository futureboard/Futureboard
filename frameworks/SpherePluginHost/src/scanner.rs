use std::fs;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::path::{Path, PathBuf};

use crate::types::PluginInfo;

#[repr(C)]
struct SpherePluginHostString {
    data: *const c_char,
    len: u64,
}

extern "C" {
    fn sphere_vst3_scan_path_json(path: *const c_char) -> SpherePluginHostString;
    fn sphere_plugin_host_free_string(value: SpherePluginHostString);
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativePluginInfo {
    name: String,
    vendor: String,
    category: String,
    format: String,
    path: String,
    #[serde(default)]
    sub_categories: Option<String>,
    #[serde(default)]
    module_path: Option<String>,
    #[serde(default)]
    class_id: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    sdk_version: Option<String>,
    #[serde(default)]
    is_shell_child: Option<bool>,
    sdk_metadata_loaded: bool,
}

pub fn scan_vst3_paths(paths: &[String]) -> Result<Vec<PluginInfo>, String> {
    let mut plugins = Vec::new();
    for path in paths {
        match scan_native_root(path) {
            Ok(mut native_plugins) => {
                plugins.append(&mut native_plugins);
                continue;
            }
            Err(_) => {
                // Keep scanning usable even if a malformed path cannot cross the C ABI.
            }
        }

        let root = PathBuf::from(path);
        if !root.exists() {
            continue;
        }
        collect_vst3_entries(&root, &mut plugins)?;
    }
    plugins.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    // Deduplicate by stable id (path + classId hash) so multi-class modules
    // like WaveShell keep all their plugin entries — only true duplicates
    // (same classId from the same module scanned twice) are removed.
    plugins.dedup_by(|a, b| a.id == b.id);
    Ok(plugins)
}

fn scan_native_root(path: &str) -> Result<Vec<PluginInfo>, String> {
    let c_path = CString::new(path).map_err(|error| error.to_string())?;
    let native = unsafe { sphere_vst3_scan_path_json(c_path.as_ptr()) };
    if native.data.is_null() {
        return Err("VST3 scanner returned an empty native string".to_string());
    }

    let json = unsafe { CStr::from_ptr(native.data) }
        .to_string_lossy()
        .to_string();
    unsafe { sphere_plugin_host_free_string(native) };

    let scanned: Vec<NativePluginInfo> =
        serde_json::from_str(&json).map_err(|error| format!("Invalid VST3 scanner JSON: {error}"))?;
    Ok(scanned
        .into_iter()
        .map(|plugin| {
            let id_source = plugin
                .class_id
                .as_ref()
                .map(|class_id| format!("{}:{class_id}", plugin.path))
                .unwrap_or_else(|| plugin.path.clone());
            let module_path = plugin
                .module_path
                .unwrap_or_else(|| plugin.path.clone());
            PluginInfo {
                id: stable_id(&id_source),
                name: plugin.name,
                vendor: plugin.vendor,
                category: plugin.category,
                sub_categories: plugin.sub_categories,
                format: plugin.format,
                path: plugin.path,
                module_path: Some(module_path),
                class_id: plugin.class_id,
                version: plugin.version,
                sdk_version: plugin.sdk_version,
                is_shell_child: plugin.is_shell_child.unwrap_or(false),
                sdk_metadata_loaded: plugin.sdk_metadata_loaded,
            }
        })
        .collect())
}

fn collect_vst3_entries(path: &Path, plugins: &mut Vec<PluginInfo>) -> Result<(), String> {
    if is_vst3_bundle(path) {
        plugins.push(plugin_from_path(path));
        return Ok(());
    }

    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) => return Err(format!("Failed to read {}: {error}", path.display())),
    };

    for entry in entries.flatten() {
        let p = entry.path();
        if is_vst3_bundle(&p) {
            plugins.push(plugin_from_path(&p));
            continue;
        }
        if p.is_dir() {
            let _ = collect_vst3_entries(&p, plugins);
        }
    }
    Ok(())
}

fn is_vst3_bundle(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("vst3"))
}

fn plugin_from_path(path: &Path) -> PluginInfo {
    let name = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("Unknown Plug-in")
        .to_string();
    let path_string = path.to_string_lossy().to_string();
    PluginInfo {
        id: stable_id(&path_string),
        name,
        vendor: "Unknown Vendor".to_string(),
        category: "Uncategorized".to_string(),
        sub_categories: None,
        format: "VST3".to_string(),
        path: path_string.clone(),
        module_path: Some(path_string),
        class_id: None,
        version: None,
        sdk_version: None,
        is_shell_child: false,
        sdk_metadata_loaded: false,
    }
}

fn stable_id(input: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("vst3:{hash:016x}")
}
