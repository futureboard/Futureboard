//! Shared editor routing types — clip kind → which bottom editor to show.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipEditorKind {
    Empty,
    Audio,
    Midi,
}

/// Minimal clip-type hint for routing (no project/timeline dependency).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipTypeHint {
    Audio,
    Midi,
}

/// Resolve which editor panel to show for the current selection.
pub fn editor_kind_for_clip(selected: Option<ClipTypeHint>) -> ClipEditorKind {
    match selected {
        Some(ClipTypeHint::Audio) => ClipEditorKind::Audio,
        Some(ClipTypeHint::Midi) => ClipEditorKind::Midi,
        None => ClipEditorKind::Empty,
    }
}
