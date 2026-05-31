//! Audio clip editor for Futureboard Studio — waveform view without PCM buffers.

mod audio_editor;
mod audio_editor_state;
mod audio_ruler;
mod editor_kind;
mod waveform_view;

pub use audio_editor::{
    AudioEditorTheme, AudioEditorViewModel, audio_editor_panel, default_wheel_handler,
    empty_audio_editor,
};
pub use audio_editor_state::AudioEditorState;
pub use editor_kind::{ClipEditorKind, ClipTypeHint, editor_kind_for_clip};
pub use waveform_view::{WaveformColumn, WaveformViewModel};
