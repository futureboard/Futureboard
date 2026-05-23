//! Shared application menu manifest.
//!
//! Source of truth: `packages/shared/src/menu/menuItems.ts`. The Electron
//! sync script (`apps/electron/scripts/sync-shared-menu.mjs`) emits a JSON
//! manifest at `packages/shared/generated/native-menu.json` which this
//! module embeds via `include_str!` and parses at startup.
//!
//! Native must not maintain its own menu definition — `MenuManifest::load`
//! returns the parsed JSON manifest. If parsing fails for any reason we log
//! the error and fall back to a minimal top-level shell so the app still
//! renders something instead of panicking.
//!
//! Realtime / audio rule: this module is pure data, no IO on hot paths.

use std::sync::OnceLock;

use serde::Deserialize;

/// JSON manifest produced by the sync script.
pub const NATIVE_MENU_JSON: &str = include_str!(
    "../../../packages/shared/generated/native-menu.json"
);

#[derive(Debug, Clone, Deserialize)]
pub struct MenuManifest {
    pub version: u32,
    #[serde(default)]
    pub menus: Vec<Menu>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Menu {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub items: Vec<MenuItem>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MenuItemKind {
    Normal,
    Separator,
    Submenu,
    Checkbox,
    Radio,
}

impl Default for MenuItemKind {
    fn default() -> Self {
        MenuItemKind::Normal
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct MenuItem {
    pub id: String,
    #[serde(default)]
    pub kind: MenuItemKind,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub shortcut: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default)]
    pub checked: bool,
    #[serde(default)]
    pub danger: bool,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub children: Vec<MenuItem>,
}

fn default_true() -> bool {
    true
}

static MANIFEST: OnceLock<MenuManifest> = OnceLock::new();

impl MenuManifest {
    /// Parse the embedded JSON once, falling back to [`MenuManifest::fallback`]
    /// on any error. Logs the failure to stderr so the issue is visible in
    /// development without panicking in release.
    pub fn load() -> &'static MenuManifest {
        MANIFEST.get_or_init(|| match serde_json::from_str::<MenuManifest>(NATIVE_MENU_JSON) {
            Ok(m) => m,
            Err(e) => {
                eprintln!(
                    "[menu] failed to parse generated native-menu.json: {e}. Falling back to minimal menu shell."
                );
                MenuManifest::fallback()
            }
        })
    }

    /// Minimal top-level menu used when the generated JSON is missing or
    /// malformed. Keeps the chrome from looking empty in that case.
    pub fn fallback() -> MenuManifest {
        let bare = |id: &str, label: &str| Menu {
            id: id.to_string(),
            label: label.to_string(),
            items: Vec::new(),
        };
        MenuManifest {
            version: 0,
            menus: vec![
                bare("file", "File"),
                bare("edit", "Edit"),
                bare("view", "View"),
                bare("transport", "Transport"),
                bare("window", "Window"),
                bare("help", "Help"),
            ],
        }
    }
}
