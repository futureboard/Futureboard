//! Audio clip editor for Futureboard Studio — waveform view without PCM buffers.

mod audio_editor;
mod audio_editor_state;
mod audio_ruler;
mod editor_kind;
mod waveform_view;

pub use audio_editor::{
    audio_editor_panel, default_wheel_handler, empty_audio_editor, AudioEditorTheme,
    AudioEditorViewModel,
};
pub use audio_editor_state::AudioEditorState;
pub use editor_kind::{editor_kind_for_clip, ClipEditorKind, ClipTypeHint};
pub use waveform_view::{WaveformColumn, WaveformViewModel};
