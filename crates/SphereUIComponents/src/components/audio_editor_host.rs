//! GPUI host for the audio clip editor — reads timeline + peak cache each frame.

use std::{cell::Cell, rc::Rc};

use gpui::{
    div, Context, Entity, FocusHandle, InteractiveElement, IntoElement, ParentElement, Render,
    ScrollWheelEvent, Styled, Window,
};
use sphere_audio_editor::{
    audio_editor_panel, default_wheel_handler, empty_audio_editor, AudioEditorState,
    AudioEditorViewModel,
};

use crate::components::audio_editor_adapter::{
    audio_editor_theme, build_waveform_view_model, selected_audio_clip,
};
use crate::components::timeline::timeline::Timeline;
use crate::theme::Colors;

pub struct AudioEditorHost {
    timeline: Entity<Timeline>,
    state: AudioEditorState,
    viewport_width: Rc<Cell<f32>>,
    focus: FocusHandle,
    last_editing_clip: Option<String>,
}

impl AudioEditorHost {
    pub fn new(timeline: Entity<Timeline>, cx: &mut Context<Self>) -> Self {
        Self {
            timeline,
            state: AudioEditorState::default(),
            viewport_width: Rc::new(Cell::new(800.0)),
            focus: cx.focus_handle(),
            last_editing_clip: None,
        }
    }

    fn build_view_model(&self, cx: &Context<Self>) -> Option<AudioEditorViewModel> {
        let tl = self.timeline.read(cx);
        let (track, clip) = selected_audio_clip(&tl.state)?;
        let theme = audio_editor_theme();
        let viewport_w = self.viewport_width.get().max(320.0);
        let ppb = self.state.pixels_per_beat;
        let scroll_x = self.state.scroll_x;

        let playhead_in_clip = {
            let rel = tl.state.transport.playhead_beats - clip.start_beat;
            if tl.state.transport.playing && rel >= 0.0 && rel <= clip.duration_beats {
                Some(rel)
            } else {
                None
            }
        };

        let selection_range = self.state.selection_range.or_else(|| {
            tl.state.arrangement_range.as_ref().map(|range| {
                let (a, b) = range.as_f32_range();
                let lo = (a - clip.start_beat).max(0.0);
                let hi = (b - clip.start_beat).min(clip.duration_beats);
                (lo.min(hi), lo.max(hi))
            })
        });

        let file_label = match &clip.clip_type {
            crate::components::timeline::timeline_state::ClipType::Audio {
                source_path: Some(path),
                ..
            } => Some(
                std::path::Path::new(path)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(path)
                    .to_string(),
            ),
            _ => None,
        };

        Some(AudioEditorViewModel {
            clip_id: clip.id.clone(),
            clip_name: clip.name.clone(),
            file_label,
            start_beat: clip.start_beat,
            duration_beats: clip.duration_beats,
            offset_beats: clip.offset_beats,
            beats_per_bar: tl.state.beats_per_bar(),
            bpm: tl.state.bpm,
            track_color: track.color,
            waveform: build_waveform_view_model(clip, &tl.state, ppb, scroll_x, viewport_w),
            playhead_in_clip,
            selection_range,
            theme,
        })
    }
}

impl Render for AudioEditorHost {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let clip_id = self
            .timeline
            .read(cx)
            .state
            .selection
            .selected_clip_ids
            .first()
            .cloned()
            .filter(|id| {
                self.timeline
                    .read(cx)
                    .state
                    .find_clip(id)
                    .is_some_and(|(_, c)| {
                        matches!(
                            c.clip_type,
                            crate::components::timeline::timeline_state::ClipType::Audio { .. }
                        )
                    })
            });

        if clip_id != self.last_editing_clip {
            self.last_editing_clip = clip_id.clone();
            if clip_id.is_none() {
                self.state.fitted_clip_id = None;
            }
        }

        let theme = audio_editor_theme();
        let body: gpui::AnyElement = match self.build_view_model(cx) {
            Some(vm) => {
                self.state.reset_for_clip_change(Some(&vm.clip_id));
                let viewport_w = self.viewport_width.get().max(320.0);
                self.state
                    .fit_clip(&vm.clip_id, vm.duration_beats, viewport_w);
                audio_editor_panel(&vm, &self.state, viewport_w).into_any_element()
            }
            None => empty_audio_editor(&theme).into_any_element(),
        };
        let viewport_width = self.viewport_width.clone();

        div()
            .key_context("AudioEditor")
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .size_full()
            .bg(Colors::surface_base())
            .on_scroll_wheel(cx.listener(Self::on_wheel))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .size_full()
                    .on_children_prepainted(move |bounds, _window, cx| {
                        let Some(bounds) = bounds.first() else {
                            return;
                        };
                        let width = f32::from(bounds.size.width).max(1.0);
                        if (viewport_width.get() - width).abs() > 0.5 {
                            viewport_width.set(width);
                            cx.refresh_windows();
                        }
                    })
                    .child(body),
            )
    }
}

impl AudioEditorHost {
    fn on_wheel(&mut self, event: &ScrollWheelEvent, _window: &mut Window, cx: &mut Context<Self>) {
        default_wheel_handler(event, &mut self.state);
        cx.notify();
    }
}
