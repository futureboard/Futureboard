//! Keyboard shortcut profiles.
//!
//! The **default** profile (`Futureboard Default`) is bundled into the binary
//! from `packages/keymaps/default.json`. Every **other** profile (Ableton,
//! Cubase, FL Studio, Studio One, or any user-authored map) is loaded at
//! runtime from `<app dir>/Keymaps/<id>.json`, where `<app dir>` is the folder
//! containing the executable.
//!
//! A profile maps command ids to accelerator strings ("Ctrl+S", "V", "Space",
//! "Shift+ArrowUp"). At load time we build a reverse index from a *canonical*
//! accelerator token back to a command id so a live [`KeyDownEvent`] can be
//! resolved to the command it should run.
//!
//! `FUTUREBOARD_SHORTCUT_DEBUG=1` traces profile load and command resolution.

use std::collections::HashMap;
use std::path::PathBuf;

use gpui::KeyDownEvent;
use serde::Deserialize;

/// Source of truth for the bundled default profile. The other profiles are NOT
/// embedded — they ship in `<app dir>/Keymaps/`.
const DEFAULT_KEYMAP_JSON: &str = include_str!("../../../packages/keymaps/default.json");

/// `FUTUREBOARD_SHORTCUT_DEBUG=1` enables shortcut profile/resolution traces.
pub fn shortcut_debug_enabled() -> bool {
    static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FLAG.get_or_init(|| std::env::var_os("FUTUREBOARD_SHORTCUT_DEBUG").is_some())
}

/// Folder the active executable lives in (`{AppDir}`), e.g. the install dir.
fn app_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.to_path_buf()))
}

/// `{AppDir}/Keymaps` — where non-default profiles are loaded from.
pub fn keymaps_dir() -> Option<PathBuf> {
    app_dir().map(|dir| dir.join("Keymaps"))
}

#[derive(Debug, Clone, Deserialize)]
struct KeymapFile {
    #[serde(default)]
    id: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    bindings: HashMap<String, String>,
}

/// A resolved keyboard profile: command id ⇄ accelerator, plus the reverse
/// index used to dispatch live key events.
#[derive(Debug, Clone, Default)]
pub struct Keymap {
    pub id: String,
    pub label: String,
    /// command id -> accelerator (as authored, e.g. "Ctrl+Shift+S").
    pub bindings: HashMap<String, String>,
    /// canonical accelerator token -> command id (built on load).
    reverse: HashMap<String, String>,
}

impl Keymap {
    /// The bundled default profile. Falls back to an empty map if the embedded
    /// JSON ever fails to parse (it is validated at build time, so this is just
    /// belt-and-braces).
    pub fn bundled_default() -> Self {
        Self::from_json(DEFAULT_KEYMAP_JSON).unwrap_or_else(|| Keymap {
            id: "default".to_string(),
            label: "Futureboard Default".to_string(),
            ..Keymap::default()
        })
    }

    fn from_json(text: &str) -> Option<Self> {
        let file: KeymapFile = serde_json::from_str(text).ok()?;
        let mut map = Keymap {
            id: file.id,
            label: file.label,
            bindings: file.bindings,
            reverse: HashMap::new(),
        };
        map.build_reverse();
        Some(map)
    }

    /// Load a profile by id. `"default"` (or empty) returns the bundled map;
    /// any other id reads `<app dir>/Keymaps/<id>.json`. Returns `None` when the
    /// file is missing or invalid so the caller can keep the current profile.
    pub fn load_profile(id: &str) -> Option<Self> {
        if id.is_empty() || id == "default" {
            return Some(Self::bundled_default());
        }
        let path = keymaps_dir()?.join(format!("{id}.json"));
        let text = std::fs::read_to_string(&path).ok()?;
        let map = Self::from_json(&text)?;
        if shortcut_debug_enabled() {
            eprintln!(
                "[shortcut] loaded profile id={} label={} from {}",
                map.id,
                map.label,
                path.display()
            );
        }
        Some(map)
    }

    /// Resolve a live key event to its command id under this profile.
    pub fn command_for_event(&self, event: &KeyDownEvent) -> Option<&str> {
        if event.is_held {
            return None;
        }
        let token = canonical_event(event)?;
        let command = self.reverse.get(&token).map(String::as_str);
        if shortcut_debug_enabled() {
            eprintln!(
                "[shortcut] resolve profile={} token={} -> {:?}",
                self.id, token, command
            );
        }
        command
    }

    fn build_reverse(&mut self) {
        // Sort by command id for deterministic collision handling, then keep the
        // higher-priority command when two share an accelerator (several commands
        // map to Ctrl+A / Delete; the editor-local `midi:`/`automation:` ones are
        // handled by the focused editor, so the global map prefers the rest).
        let mut entries: Vec<(&String, &String)> = self.bindings.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        for (command, accel) in entries {
            let Some(token) = canonical_accel(accel) else {
                continue;
            };
            match self.reverse.get(&token) {
                Some(existing) if global_priority(existing) <= global_priority(command) => {}
                _ => {
                    self.reverse.insert(token, command.clone());
                }
            }
        }
    }
}

/// Lower = higher priority for the *global* reverse index. Editor-local command
/// families lose accelerator collisions because the focused editor handles them.
fn global_priority(command: &str) -> u8 {
    if command.starts_with("midi:") || command.starts_with("automation:") {
        2
    } else {
        0
    }
}

/// Canonicalize an authored accelerator ("Ctrl+Shift+S") into a stable token
/// ("ctrl+shift+s"). Modifiers are sorted so token order never matters.
fn canonical_accel(accel: &str) -> Option<String> {
    let mut ctrl = false;
    let mut shift = false;
    let mut alt = false;
    let mut base: Option<String> = None;
    for raw in accel.split('+') {
        let part = raw.trim().to_ascii_lowercase();
        match part.as_str() {
            "" => {}
            "ctrl" | "control" | "cmd" | "command" | "meta" | "super" => ctrl = true,
            "shift" => shift = true,
            "alt" | "option" | "opt" => alt = true,
            other => base = Some(canonical_key(other)),
        }
    }
    Some(join_token(ctrl, shift, alt, base?))
}

/// Canonicalize a live [`KeyDownEvent`] into the same token space as
/// [`canonical_accel`].
fn canonical_event(event: &KeyDownEvent) -> Option<String> {
    let m = &event.keystroke.modifiers;
    let ctrl = m.control || m.platform;
    let shift = m.shift;
    let alt = m.alt;
    let base = canonical_key(&event.keystroke.key.to_ascii_lowercase());
    if base.is_empty() {
        return None;
    }
    Some(join_token(ctrl, shift, alt, base))
}

fn join_token(ctrl: bool, shift: bool, alt: bool, base: String) -> String {
    // Alphabetical, fixed order: alt < ctrl < shift.
    let mut out = String::new();
    if alt {
        out.push_str("alt+");
    }
    if ctrl {
        out.push_str("ctrl+");
    }
    if shift {
        out.push_str("shift+");
    }
    out.push_str(&base);
    out
}

/// Normalize a single base-key name from either an authored accelerator
/// ("ArrowUp", "Esc", "Space") or a GPUI keystroke ("up", "escape", "space").
fn canonical_key(key: &str) -> String {
    match key {
        "space" | "spacebar" => "space",
        "escape" | "esc" => "escape",
        "enter" | "return" | "numpad_enter" => "enter",
        "delete" | "del" => "delete",
        "backspace" => "backspace",
        "tab" => "tab",
        "home" => "home",
        "end" => "end",
        "pageup" | "page_up" | "pgup" => "pageup",
        "pagedown" | "page_down" | "pgdn" => "pagedown",
        "arrowleft" | "arrow_left" | "left" => "left",
        "arrowright" | "arrow_right" | "right" => "right",
        "arrowup" | "arrow_up" | "up" => "up",
        "arrowdown" | "arrow_down" | "down" => "down",
        "plus" => "=",
        "minus" => "-",
        other => other,
    }
    .to_string()
}
