use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeymapSource {
    Default,
    User,
    Imported,
    Plugin,
}

impl KeymapSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::User => "User",
            Self::Imported => "Imported",
            Self::Plugin => "Plugin",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeymapProfile {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub extends: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub bindings: Vec<KeyBinding>,
}

impl Default for KeymapProfile {
    fn default() -> Self {
        Self {
            name: String::new(),
            extends: None,
            version: Some("1".to_string()),
            bindings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KeyBinding {
    pub action: String,
    #[serde(default)]
    pub keys: Vec<String>,
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default)]
    pub args: Option<Value>,
    #[serde(default)]
    pub when: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedKeyBinding {
    pub action: String,
    pub keys: Vec<String>,
    pub context: Option<String>,
    pub args: Option<Value>,
    pub source: KeymapSource,
    pub profile: String,
    pub is_user_override: bool,
}

#[derive(Debug, Clone)]
pub struct KeymapRow {
    pub id: String,
    pub action_id: String,
    pub action_label: String,
    pub command: String,
    pub arguments_json: Option<String>,
    pub keystrokes: Vec<String>,
    pub context: Option<String>,
    pub source: KeymapSource,
    pub profile: String,
    pub is_user_override: bool,
    pub is_conflict: bool,
    pub conflict_with: Vec<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct KeymapConflict {
    pub keystroke: String,
    pub action: String,
    pub action_label: String,
    pub context: Option<String>,
    pub source: KeymapSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfileDescriptor {
    pub id: &'static str,
    pub label: &'static str,
    pub builtin: bool,
}

pub const PROFILE_DESCRIPTORS: &[ProfileDescriptor] = &[
    ProfileDescriptor {
        id: "default",
        label: "Default",
        builtin: true,
    },
    ProfileDescriptor {
        id: "futureboard",
        label: "Futureboard",
        builtin: true,
    },
    ProfileDescriptor {
        id: "fl-studio",
        label: "FL Studio",
        builtin: true,
    },
    ProfileDescriptor {
        id: "ableton-live",
        label: "Ableton Live",
        builtin: true,
    },
    ProfileDescriptor {
        id: "cubase",
        label: "Cubase",
        builtin: true,
    },
    ProfileDescriptor {
        id: "pro-tools",
        label: "Pro Tools",
        builtin: true,
    },
    ProfileDescriptor {
        id: "custom",
        label: "Custom",
        builtin: false,
    },
];

pub const USER_OVERRIDES_FILE: &str = "user-overrides.json";
