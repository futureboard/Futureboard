//! Project chord/lyric displays and the structured lyric editor.
//!
//! Size: fills its dock/window, minimum useful size 260×160. The cue list is
//! the sole scroll owner. Focus is local to the editor inputs; displays are
//! read-only. All persisted data lives in `TimelineState::song_text_cues`.

use gpui::{
    div, px, size, App, AppContext, Bounds, Context, Entity, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Render, StatefulInteractiveElement, Styled, Window,
    WindowBackgroundAppearance, WindowBounds, WindowHandle,
};

use crate::components::text_input::{text_field, TextInputAction, TextInputState};
use crate::components::timeline::timeline_state::SongTextCue;
use crate::components::timeline::Timeline;
use crate::theme::Colors;
use crate::window_position::{apply_owner_display, centered_window_bounds};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SongTextPanelKind {
    ChordDisplay,
    LyricDisplay,
    LyricEditor,
}

impl SongTextPanelKind {
    pub fn title(self) -> &'static str {
        match self {
            Self::ChordDisplay => "Chord Display",
            Self::LyricDisplay => "Lyric Display",
            Self::LyricEditor => "Lyric Editor",
        }
    }
}

pub struct SongTextPanelView {
    timeline: Entity<Timeline>,
    kind: SongTextPanelKind,
    selected_id: Option<String>,
    chord_input: TextInputState,
    lyric_input: TextInputState,
}

impl SongTextPanelView {
    pub fn new(
        timeline: Entity<Timeline>,
        kind: SongTextPanelKind,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(100))
                .await;
            if this.update(cx, |_view, cx| cx.notify()).is_err() {
                break;
            }
        })
        .detach();
        Self {
            timeline,
            kind,
            selected_id: None,
            chord_input: TextInputState::new("song-text-chord", cx.focus_handle())
                .with_placeholder("Chord, e.g. Am7"),
            lyric_input: TextInputState::new("song-text-lyric", cx.focus_handle())
                .with_placeholder("Lyric line"),
        }
    }

    pub fn kind(&self) -> SongTextPanelKind {
        self.kind
    }

    fn select(&mut self, id: &str, cx: &mut Context<Self>) {
        let cue = self
            .timeline
            .read(cx)
            .state
            .song_text_cues
            .iter()
            .find(|cue| cue.id == id)
            .cloned();
        if let Some(cue) = cue {
            self.selected_id = Some(cue.id);
            self.chord_input.set_value(cue.chord);
            self.lyric_input.set_value(cue.lyric);
            cx.notify();
        }
    }

    fn add_at_playhead(&mut self, cx: &mut Context<Self>) {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = format!(
            "song-text-{}",
            NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let mut beat = 0.0;
        let _ = self.timeline.update(cx, |timeline, cx| {
            beat = timeline.state.transport.playhead_beats as f64;
            if timeline
                .state
                .upsert_song_text_cue(SongTextCue::new(id.clone(), beat))
            {
                timeline.mark_project_changed(cx);
                cx.notify();
            }
        });
        self.selected_id = Some(id);
        self.chord_input.set_value("");
        self.lyric_input.set_value("");
        cx.notify();
    }

    fn commit_selected(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_id.clone() else {
            return;
        };
        let chord = self.chord_input.value.trim().to_string();
        let lyric = self.lyric_input.value.trim().to_string();
        let _ = self.timeline.update(cx, |timeline, cx| {
            let Some(existing) = timeline
                .state
                .song_text_cues
                .iter()
                .find(|cue| cue.id == id)
                .cloned()
            else {
                return;
            };
            let cue = SongTextCue {
                chord,
                lyric,
                ..existing
            };
            if timeline.state.upsert_song_text_cue(cue) {
                timeline.mark_project_changed(cx);
                cx.notify();
            }
        });
        cx.notify();
    }

    fn delete_selected(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_id.take() else {
            return;
        };
        let _ = self.timeline.update(cx, |timeline, cx| {
            if timeline.state.remove_song_text_cue(&id) {
                timeline.mark_project_changed(cx);
                cx.notify();
            }
        });
        self.chord_input.set_value("");
        self.lyric_input.set_value("");
        cx.notify();
    }

    fn handle_key(
        &mut self,
        event: &KeyDownEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let action = if self.chord_input.focus_handle.is_focused(window) {
            self.chord_input.handle_key_with_clipboard(event, Some(cx))
        } else if self.lyric_input.focus_handle.is_focused(window) {
            self.lyric_input.handle_key_with_clipboard(event, Some(cx))
        } else {
            return false;
        };
        match action {
            TextInputAction::Submit => self.commit_selected(cx),
            TextInputAction::Consumed => cx.notify(),
            TextInputAction::Cancel => {
                if let Some(id) = self.selected_id.clone() {
                    self.select(&id, cx);
                }
            }
            TextInputAction::Pass => return false,
        }
        true
    }
}

impl Render for SongTextPanelView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let cues = self.timeline.read(cx).state.song_text_cues.clone();
        let active = self.timeline.read(cx).state.active_song_text_cue().cloned();
        let kind = self.kind;
        let entity = cx.entity().clone();

        let root = div()
            .id(("song-text-panel", kind as u32))
            .flex()
            .flex_col()
            .size_full()
            .min_w(px(260.0))
            .min_h(px(160.0))
            .overflow_hidden()
            .bg(Colors::surface_base())
            .capture_key_down(move |event, window, cx| {
                let handled = entity.update(cx, |view, cx| view.handle_key(event, window, cx));
                if handled {
                    cx.stop_propagation();
                }
            })
            .child(panel_header(kind.title()));

        match kind {
            SongTextPanelKind::ChordDisplay => root.child(display_surface(
                active
                    .as_ref()
                    .and_then(|cue| (!cue.chord.is_empty()).then_some(cue.chord.as_str())),
                active.as_ref().map(|cue| cue.beat),
                "No chord cue at the playhead",
                true,
            )),
            SongTextPanelKind::LyricDisplay => root.child(display_surface(
                active
                    .as_ref()
                    .and_then(|cue| (!cue.lyric.is_empty()).then_some(cue.lyric.as_str())),
                active.as_ref().map(|cue| cue.beat),
                "No lyric cue at the playhead",
                false,
            )),
            SongTextPanelKind::LyricEditor => {
                let selected = self.selected_id.clone();
                let mut list = div()
                    .id("song-text-cue-list")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll();
                for (cue_index, cue) in cues.into_iter().enumerate() {
                    let id = cue.id.clone();
                    let is_selected = selected.as_deref() == Some(id.as_str());
                    let target = cx.entity().clone();
                    list = list.child(
                        div()
                            .id(("song-text-cue", cue_index))
                            .h(px(28.0))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .px(px(8.0))
                            .border_b(px(1.0))
                            .border_color(Colors::border_subtle())
                            .bg(if is_selected {
                                Colors::accent_soft()
                            } else {
                                Colors::surface_base()
                            })
                            .cursor(gpui::CursorStyle::PointingHand)
                            .on_click(move |_, _, cx| {
                                let _ = target.update(cx, |view, cx| view.select(&id, cx));
                            })
                            .child(
                                div()
                                    .w(px(54.0))
                                    .text_size(px(10.0))
                                    .text_color(Colors::text_muted())
                                    .child(format!("{:.2}", cue.beat)),
                            )
                            .child(
                                div()
                                    .w(px(64.0))
                                    .truncate()
                                    .text_size(px(11.0))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(Colors::accent_primary())
                                    .child(cue.chord),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(11.0))
                                    .text_color(Colors::text_primary())
                                    .child(cue.lyric),
                            ),
                    );
                }
                root.child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_h_0()
                        .child(list)
                        .child(
                            div()
                                .w_full()
                                .h(px(150.0))
                                .flex_shrink_0()
                                .flex()
                                .flex_col()
                                .gap(px(8.0))
                                .p(px(10.0))
                                .border_t(px(1.0))
                                .border_color(Colors::border_subtle())
                                .child(field_label("CHORD"))
                                .child(text_field(
                                    &self.chord_input,
                                    self.chord_input.is_focused(window),
                                ))
                                .child(field_label("LYRIC"))
                                .child(text_field(
                                    &self.lyric_input,
                                    self.lyric_input.is_focused(window),
                                ))
                                .child(div().flex_1())
                                .child(editor_actions(
                                    cx.entity().clone(),
                                    self.selected_id.is_some(),
                                )),
                        ),
                )
            }
        }
    }
}

fn panel_header(title: &'static str) -> impl IntoElement {
    div()
        .h(px(28.0))
        .flex_shrink_0()
        .flex()
        .items_center()
        .px(px(9.0))
        .border_b(px(1.0))
        .border_color(Colors::border_subtle())
        .bg(Colors::surface_panel())
        .text_size(px(10.5))
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(Colors::tab_text())
        .child(title)
}

fn display_surface(
    value: Option<&str>,
    beat: Option<f64>,
    empty: &'static str,
    chord: bool,
) -> impl IntoElement {
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(8.0))
        .p(px(12.0))
        .child(
            div()
                .text_size(px(if chord { 42.0 } else { 28.0 }))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(
                    value
                        .map(|_| Colors::text_primary())
                        .unwrap_or_else(Colors::text_faint),
                )
                .child(value.unwrap_or(empty).to_string()),
        )
        .children(beat.map(|beat| {
            div()
                .text_size(px(10.0))
                .text_color(Colors::text_muted())
                .child(format!("Beat {beat:.2}"))
        }))
}

fn field_label(label: &'static str) -> impl IntoElement {
    div()
        .text_size(px(9.5))
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(Colors::text_muted())
        .child(label)
}

fn editor_actions(target: Entity<SongTextPanelView>, selected: bool) -> impl IntoElement {
    let add_target = target.clone();
    let save_target = target.clone();
    div()
        .flex()
        .gap(px(6.0))
        .child(
            action_button("Add at Playhead", true).on_click(move |_, _, cx| {
                let _ = add_target.update(cx, |view, cx| view.add_at_playhead(cx));
            }),
        )
        .child(action_button("Save", selected).on_click(move |_, _, cx| {
            let _ = save_target.update(cx, |view, cx| view.commit_selected(cx));
        }))
        .child(action_button("Delete", selected).on_click(move |_, _, cx| {
            let _ = target.update(cx, |view, cx| view.delete_selected(cx));
        }))
}

fn action_button(label: &'static str, enabled: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(label)
        .h(px(24.0))
        .px(px(8.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded_sm()
        .border(px(1.0))
        .border_color(Colors::border_subtle())
        .bg(Colors::surface_input())
        .text_size(px(10.5))
        .text_color(if enabled {
            Colors::text_secondary()
        } else {
            Colors::text_faint()
        })
        .cursor(if enabled {
            gpui::CursorStyle::PointingHand
        } else {
            gpui::CursorStyle::Arrow
        })
}

pub struct SongTextWindow {
    panel: Entity<SongTextPanelView>,
    kind: SongTextPanelKind,
    on_close: std::sync::Arc<dyn Fn(SongTextPanelKind, &mut App) + Send + Sync>,
}

impl Render for SongTextWindow {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let kind = self.kind;
        let on_close = self.on_close.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(Colors::surface_window())
            .child(crate::components::title_bar::external_window_titlebar(
                kind.title(),
                "song-text-window-close",
                move |window, cx| {
                    on_close(kind, cx);
                    window.remove_window();
                },
            ))
            .child(div().flex_1().min_h_0().child(self.panel.clone()))
    }
}

pub fn open_song_text_window(
    owner_bounds: Option<Bounds<gpui::Pixels>>,
    timeline: Entity<Timeline>,
    kind: SongTextPanelKind,
    on_close: std::sync::Arc<dyn Fn(SongTextPanelKind, &mut App) + Send + Sync>,
    cx: &mut App,
) -> Result<WindowHandle<SongTextWindow>, String> {
    let window_bounds = centered_window_bounds(owner_bounds, size(px(640.0), px(400.0)), cx);
    let mut options = crate::platform_chrome::external_window_options_partial();
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.title = Some(crate::platform_chrome::branded_window_title(kind.title()).into());
    }
    options.window_bounds = Some(WindowBounds::Windowed(window_bounds));
    options.window_background = WindowBackgroundAppearance::Opaque;
    options.window_min_size = Some(size(px(300.0), px(200.0)));
    apply_owner_display(&mut options, owner_bounds, cx);
    cx.open_window(options, move |_window, cx| {
        let panel = cx.new(|cx| SongTextPanelView::new(timeline, kind, cx));
        cx.new(|_| SongTextWindow {
            panel,
            kind,
            on_close,
        })
    })
    .map_err(|error| error.to_string())
}
