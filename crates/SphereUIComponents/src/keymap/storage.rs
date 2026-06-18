use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::model::{KeyBinding, KeymapProfile, USER_OVERRIDES_FILE};

#[derive(Debug, Clone, Deserialize)]
struct LegacyKeymapFile {
    #[serde(default)]
    version: Option<u32>,
    #[serde(default)]
    id: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    extends: Option<String>,
    #[serde(default)]
    bindings: LegacyBindings,
}

impl Default for LegacyBindings {
    fn default() -> Self {
        LegacyBindings::Map(HashMap::new())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum LegacyBindings {
    Map(HashMap<String, String>),
    List(Vec<KeyBinding>),
}

pub fn user_keymaps_dir(app_data: &Path) -> PathBuf {
    app_data.join("Keymaps")
}

pub fn ensure_user_keymaps_dir(app_data: &Path) -> std::io::Result<PathBuf> {
    let dir = user_keymaps_dir(app_data);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn user_overrides_path(app_data: &Path) -> PathBuf {
    user_keymaps_dir(app_data).join(USER_OVERRIDES_FILE)
}

pub fn load_profile_json(text: &str) -> Result<KeymapProfile, String> {
    let raw: LegacyKeymapFile =
        serde_json::from_str(text).map_err(|error| format!("Invalid keymap JSON: {error}"))?;
    let name = if !raw.name.is_empty() {
        raw.name
    } else if !raw.label.is_empty() {
        raw.label
    } else {
        raw.id.clone()
    };
    let bindings = match raw.bindings {
        LegacyBindings::Map(map) => map
            .into_iter()
            .map(|(action, keys)| KeyBinding {
                action,
                keys: vec![keys],
                context: Some("Studio".to_string()),
                args: None,
                when: None,
            })
            .collect(),
        LegacyBindings::List(list) => list,
    };
    Ok(KeymapProfile {
        name,
        extends: raw.extends,
        version: raw.version.map(|v| v.to_string()),
        bindings,
    })
}

pub fn save_profile_json(profile: &KeymapProfile, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("Failed to create keymap folder: {error}"))?;
    }
    let text = serde_json::to_string_pretty(profile)
        .map_err(|error| format!("Failed to serialize keymap: {error}"))?;
    fs::write(path, text).map_err(|error| format!("Failed to write keymap: {error}"))
}

pub fn load_user_overrides(app_data: &Path) -> KeymapProfile {
    let path = user_overrides_path(app_data);
    let Ok(text) = fs::read_to_string(&path) else {
        return KeymapProfile {
            name: "User Overrides".to_string(),
            extends: Some("default".to_string()),
            ..KeymapProfile::default()
        };
    };
    load_profile_json(&text).unwrap_or_else(|error| {
        eprintln!("[keymap] failed to load user overrides: {error}");
        KeymapProfile {
            name: "User Overrides".to_string(),
            extends: Some("default".to_string()),
            ..KeymapProfile::default()
        }
    })
}

pub fn save_user_overrides(app_data: &Path, profile: &KeymapProfile) -> Result<(), String> {
    let path = user_overrides_path(app_data);
    save_profile_json(profile, &path)
}

pub fn builtin_profile_json(profile_id: &str) -> Option<&'static str> {
    match profile_id {
        "default" => Some(include_str!("../../../../packages/keymaps/default.json")),
        "futureboard" => Some(include_str!("../../../../packages/keymaps/futureboard.json")),
        "fl-studio" => Some(include_str!("../../../../packages/keymaps/fl_studio.json")),
        "ableton-live" => Some(include_str!("../../../../packages/keymaps/ableton.json")),
        "cubase" => Some(include_str!("../../../../packages/keymaps/cubase.json")),
        "pro-tools" => Some(include_str!("../../../../packages/keymaps/pro_tools.json")),
        _ => None,
    }
}

pub fn load_builtin_profile(profile_id: &str) -> Result<KeymapProfile, String> {
    let text = builtin_profile_json(profile_id)
        .ok_or_else(|| format!("Unknown builtin profile: {profile_id}"))?;
    load_profile_json(text)
}

pub fn import_profile_file(path: &Path) -> Result<KeymapProfile, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("Failed to read file: {error}"))?;
    load_profile_json(&text)
}
