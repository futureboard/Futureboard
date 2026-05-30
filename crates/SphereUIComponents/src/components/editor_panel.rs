//! Routes the bottom editor panel between AudioEditor, MidiEditor, and empty state.

use gpui::{div, px, Context, Entity, IntoElement, ParentElement, Render, Styled, Window};
use sphere_audio_editor::{editor_kind_for_clip, ClipEditorKind};

use crate::components::audio_editor_adapter::{audio_editor_theme, clip_type_hint_for_selection};
use crate::components::audio_editor_host::AudioEditorHost;
use crate::components::piano_roll::PianoRoll;
use crate::components::timeline::timeline::Timeline;
use crate::theme::Colors;

pub struct ClipEditorPanel {
    timeline: Entity<Timeline>,
    piano_roll: Entity<PianoRoll>,
    audio_editor: Entity<AudioEditorHost>,
}

impl ClipEditorPanel {
    pub fn new(
        timeline: Entity<Timeline>,
        piano_roll: Entity<PianoRoll>,
        audio_editor: Entity<AudioEditorHost>,
    ) -> Self {
        Self {
            timeline,
            piano_roll,
            audio_editor,
        }
    }

    fn current_kind(&self, cx: &Context<Self>) -> ClipEditorKind {
        let hint = clip_type_hint_for_selection(&self.timeline.read(cx).state);
        editor_kind_for_clip(hint)
    }
}

impl Render for ClipEditorPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.current_kind(cx) {
            ClipEditorKind::Audio => self.audio_editor.clone().into_any_element(),
            ClipEditorKind::Midi => self.piano_roll.clone().into_any_element(),
            ClipEditorKind::Empty => empty_editor_panel().into_any_element(),
        }
    }
}

fn empty_editor_panel() -> impl IntoElement {
    let theme = audio_editor_theme();
    div()
        .flex()
        .items_center()
        .justify_center()
        .size_full()
        .bg(Colors::surface_base())
        .text_size(px(11.0))
        .text_color(theme.text_muted)
        .child("Select a clip to edit")
}
