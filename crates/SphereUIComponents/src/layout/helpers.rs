use gpui::KeyDownEvent;

use crate::components::add_track_dialog::AddTrackKind;
use crate::components::timeline::timeline_state::TrackState;

use super::studio_state::TransportCommand;

/// Keep in sync with `DAUx::probe_audio_file`,
/// `waveform_cache::decode_file_uncached`, and
/// `file_browser::FileBrowserEntry::is_audio` — any divergence between
/// these lists creates "imports but never plays" or "looks pending
/// forever" bugs.
pub(super) fn is_supported_audio_ext(ext: &str) -> bool {
    matches!(
        ext,
        "wav" | "wave" | "mp3" | "flac" | "ogg" | "oga" | "aiff" | "aif"
    )
}

/// Resolve a shared menu command ID to a transport action.
/// Returns `None` for commands the unified dispatcher should log as
/// unsupported. Keep in lock-step with `apps/web/src/menu/actionRunner.ts`
/// and `packages/shared/generated/native-menu.json`.
pub(super) fn transport_command_from_id(command_id: &str) -> Option<TransportCommand> {
    match command_id {
        "transport:play-pause" => Some(TransportCommand::PlayPause),
        "transport:stop" => Some(TransportCommand::Stop),
        "transport:go-to-start" => Some(TransportCommand::ReturnToStart),
        "transport:toggle-loop" => Some(TransportCommand::ToggleLoop),
        "transport:toggle-metronome" => Some(TransportCommand::ToggleMetronome),
        "transport:toggle-follow-playhead" | "transport:toggle-autoscroll" => {
            Some(TransportCommand::ToggleFollowPlayhead)
        }
        "transport:record" => Some(TransportCommand::Record),
        _ => None,
    }
}

/// Focus-relevant snapshot used to decide whether a global transport shortcut
/// (Space, Enter, ...) should be handled by the workspace or left to the focused
/// widget. Captured on the UI thread at the moment a key arrives.
pub(super) struct FocusContext {
    /// A Futureboard text field (search / rename / numeric edit) owns focus.
    pub text_input_focused: bool,
}

/// Whether the workspace should claim a global transport shortcut.
///
/// - Text field focused -> keep the keystroke (Space types a space).
/// - Otherwise -> the workspace handles it (Space toggles playback).
///
/// Note: when the native plugin editor window is the active OS window this
/// code path is never reached - Windows delivers the key to the plugin's HWND,
/// not the GPUI workspace window - so "plugin editor focused" implicitly means
/// the plugin consumes the key, matching the current policy.
pub(super) fn should_handle_global_transport_shortcut(focus: &FocusContext) -> bool {
    !focus.text_input_focused
}

pub(super) fn key_debug() -> bool {
    std::env::var_os("FUTUREBOARD_KEY_DEBUG").is_some()
}

pub(super) fn is_text_input_key(event: &KeyDownEvent) -> bool {
    let key = event.keystroke.key.as_str();
    let mods = event.keystroke.modifiers;
    if (mods.control || mods.platform) && !mods.alt && !mods.function {
        return matches!(key, "a" | "A" | "c" | "C" | "v" | "V" | "x" | "X");
    }
    if mods.control || mods.alt || mods.platform || mods.function {
        return false;
    }
    matches!(
        key,
        "backspace"
            | "delete"
            | "left"
            | "arrow_left"
            | "right"
            | "arrow_right"
            | "home"
            | "end"
            | "space"
    ) || key.chars().count() == 1
}

pub(super) fn normalize_command_id(command_id: &str) -> String {
    command_id.trim().replace('.', ":").replace('_', "-")
}

pub(super) fn cleaned_track_name(name: &str, kind: AddTrackKind) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        kind.label().to_string()
    } else {
        trimmed.to_string()
    }
}

pub(super) fn numbered_name_stem(name: &str) -> String {
    let stem = name
        .trim_end_matches(|c: char| c.is_ascii_digit())
        .trim_end();
    if stem.is_empty() {
        "Track".to_string()
    } else {
        stem.to_string()
    }
}

pub(super) fn smooth_meter_value(current: &mut f32, target: f32) -> bool {
    let target = target.clamp(0.0, 1.0);
    let rate = if target > *current { 0.72 } else { 0.18 };
    let next = (*current + (target - *current) * rate).clamp(0.0, 1.0);
    let changed = (*current - next).abs() > 0.001;
    *current = if next < 0.002 { 0.0 } else { next };
    changed
}

pub(super) fn find_clip_summary<'a>(
    tracks: &'a [TrackState],
    clip_id: Option<&str>,
) -> Option<crate::components::panel::SelectedClipSummary<'a>> {
    let id = clip_id?;
    for t in tracks {
        if let Some(c) = t.clips.iter().find(|c| c.id == id) {
            let (kind, source_path, note_count) = match &c.clip_type {
                crate::components::timeline::timeline_state::ClipType::Audio {
                    source_path,
                    ..
                } => ("Audio", source_path.as_deref(), None),
                crate::components::timeline::timeline_state::ClipType::Midi { notes, .. } => {
                    ("MIDI", None, Some(notes.len()))
                }
            };
            return Some(crate::components::panel::SelectedClipSummary {
                clip_id: &c.id,
                track_id: &t.id,
                name: &c.name,
                start_beat: c.start_beat,
                duration_beats: c.duration_beats,
                muted: c.muted,
                gain: c.gain,
                source_duration_seconds: c.source_duration_seconds,
                source_path,
                note_count,
                kind,
                track_name: &t.name,
            });
        }
    }
    None
}

pub(super) fn reveal_path(path: &std::path::Path) {
    #[cfg(target_os = "windows")]
    {
        if path.is_file() {
            let _ = std::process::Command::new("explorer")
                .arg(format!("/select,\"{}\"", path.display()))
                .spawn();
        } else {
            let _ = std::process::Command::new("explorer")
                .arg(format!("\"{}\"", path.display()))
                .spawn();
        }
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg(if path.is_file() { "-R" } else { "" })
            .arg(path)
            .spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let parent = if path.is_file() {
            path.parent().unwrap_or(path)
        } else {
            path
        };
        let _ = std::process::Command::new("xdg-open").arg(parent).spawn();
    }
}
