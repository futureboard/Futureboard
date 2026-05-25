use gpui::prelude::FluentBuilder;
use gpui::{
    div, px, rgba, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent,
    MouseButton, ParentElement, Styled, Window,
};

use crate::theme::Colors;

// ── Action ────────────────────────────────────────────────────────────────────

/// What the parent should do after processing a key event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextInputAction {
    /// Key consumed — update display.
    Consumed,
    /// Enter pressed — parent should submit/confirm.
    Submit,
    /// Escape pressed — parent should cancel/close.
    Cancel,
    /// Key not for this input (Ctrl+S etc.) — pass to app shortcuts.
    Pass,
}

// ── State ─────────────────────────────────────────────────────────────────────

/// Plain-data text input state owned by a parent GPUI entity (StudioLayout).
/// No entity of its own — all event routing is done in the parent's
/// `capture_key_down` handler.
#[derive(Clone)]
pub struct TextInputState {
    /// Unique element id used for the div's `.id()`.
    pub element_id: &'static str,
    /// GPUI focus handle — one per text input.
    pub focus_handle: FocusHandle,
    /// Current string value.
    pub value: String,
    /// Cursor position in *chars* (not bytes).
    pub cursor: usize,
    /// True when Ctrl+A was pressed; clears on next printable key.
    pub selected_all: bool,
    /// Grey hint text shown when value is empty.
    pub placeholder: Option<String>,
}

impl TextInputState {
    pub fn new(element_id: &'static str, focus_handle: FocusHandle) -> Self {
        Self {
            element_id,
            focus_handle,
            value: String::new(),
            cursor: 0,
            selected_all: false,
            placeholder: None,
        }
    }

    pub fn with_placeholder(mut self, p: impl Into<String>) -> Self {
        self.placeholder = Some(p.into());
        self
    }

    pub fn set_value(&mut self, v: impl Into<String>) {
        self.value = v.into();
        self.cursor = self.char_count();
        self.selected_all = false;
    }

    pub fn select_all(&mut self) {
        if !self.value.is_empty() {
            self.selected_all = true;
        }
    }

    pub fn is_focused(&self, window: &Window) -> bool {
        self.focus_handle.is_focused(window)
    }

    // ── Key handling ──────────────────────────────────────────────────────────

    /// Process one key event. Returns the action the parent should take.
    /// This does NOT call `cx.notify()` — the caller must do that.
    pub fn handle_key(&mut self, event: &KeyDownEvent) -> TextInputAction {
        let key = event.keystroke.key.as_str();
        let mods = event.keystroke.modifiers;

        // Ctrl / Cmd combos
        if mods.control || mods.platform {
            return match key {
                "a" | "A" => {
                    self.select_all();
                    TextInputAction::Consumed
                }
                // Let Ctrl+C / Ctrl+V / Ctrl+X / Ctrl+S etc. pass through
                _ => TextInputAction::Pass,
            };
        }

        // Navigation & editing
        match key {
            "backspace" => {
                self.delete_backward();
                TextInputAction::Consumed
            }
            "delete" => {
                self.delete_forward();
                TextInputAction::Consumed
            }
            "left" => {
                self.selected_all = false;
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
                TextInputAction::Consumed
            }
            "right" => {
                self.selected_all = false;
                let n = self.char_count();
                if self.cursor < n {
                    self.cursor += 1;
                }
                TextInputAction::Consumed
            }
            "home" => {
                self.selected_all = false;
                self.cursor = 0;
                TextInputAction::Consumed
            }
            "end" => {
                self.selected_all = false;
                self.cursor = self.char_count();
                TextInputAction::Consumed
            }
            "enter" | "numpad_enter" => TextInputAction::Submit,
            "escape" => TextInputAction::Cancel,
            _ => {
                if let Some(text) = printable_text(key) {
                    self.insert_str(text);
                    TextInputAction::Consumed
                } else {
                    TextInputAction::Pass
                }
            }
        }
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    fn char_count(&self) -> usize {
        self.value.chars().count()
    }

    fn byte_at_char(&self, char_idx: usize) -> usize {
        self.value
            .char_indices()
            .nth(char_idx)
            .map(|(b, _)| b)
            .unwrap_or(self.value.len())
    }

    fn insert_str(&mut self, text: &str) {
        if self.selected_all {
            self.value.clear();
            self.cursor = 0;
            self.selected_all = false;
        }
        let byte_pos = self.byte_at_char(self.cursor);
        self.value.insert_str(byte_pos, text);
        self.cursor += text.chars().count();
    }

    fn delete_backward(&mut self) {
        if self.selected_all {
            self.value.clear();
            self.cursor = 0;
            self.selected_all = false;
            return;
        }
        if self.cursor == 0 {
            return;
        }
        let chars: Vec<(usize, char)> = self.value.char_indices().collect();
        let (byte, ch) = chars[self.cursor - 1];
        self.value.drain(byte..byte + ch.len_utf8());
        self.cursor -= 1;
    }

    fn delete_forward(&mut self) {
        if self.selected_all {
            self.value.clear();
            self.cursor = 0;
            self.selected_all = false;
            return;
        }
        if self.cursor >= self.char_count() {
            return;
        }
        let chars: Vec<(usize, char)> = self.value.char_indices().collect();
        let (byte, ch) = chars[self.cursor];
        self.value.drain(byte..byte + ch.len_utf8());
    }
}

// ── Printable character extraction ────────────────────────────────────────────

fn printable_text(key: &str) -> Option<&str> {
    match key {
        "space" => Some(" "),
        k if k.chars().count() == 1 => {
            let c = k.chars().next()?;
            if c.is_control() { None } else { Some(k) }
        }
        _ => None,
    }
}

// ── Render ────────────────────────────────────────────────────────────────────

/// Render a text input field.
///
/// `focused` must be `state.focus_handle.is_focused(cx)` from the parent render.
/// The div has `track_focus` applied so GPUI routes key events here when the
/// focus handle is active. The `on_mouse_down` handler focuses the handle.
pub fn text_field(state: &TextInputState, focused: bool) -> impl IntoElement {
    let fh_click = state.focus_handle.clone();
    let fh_track = state.focus_handle.clone();
    let value = state.value.clone();
    let cursor = state.cursor;
    let sel_all = state.selected_all;
    let is_empty = value.is_empty();
    let placeholder = state.placeholder.clone().unwrap_or_default();

    let border = if focused { rgba(0x5FCED0B0) } else { rgba(0xFFFFFF1A) };
    let bg = if focused { rgba(0x192030FF) } else { rgba(0x0E1117FF) };

    // Build the text + cursor content
    let content: gpui::AnyElement = if is_empty {
        div()
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(rgba(0x4A556680))
                    .child(placeholder),
            )
            .when(focused, |d| {
                d.child(
                    div()
                        .w(px(1.5))
                        .h(px(15.0))
                        .bg(Colors::accent_primary())
                        .rounded_sm(),
                )
            })
            .into_any_element()
    } else if sel_all {
        let sel_bg = rgba(0x5FCED030);
        div()
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .child(
                div()
                    .flex_1()
                    .rounded_sm()
                    .bg(sel_bg)
                    .px(px(3.0))
                    .text_size(px(12.0))
                    .text_color(Colors::text_primary())
                    .child(value),
            )
            .into_any_element()
    } else {
        let before: String = value.chars().take(cursor).collect();
        let after: String = value.chars().skip(cursor).collect();
        div()
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(Colors::text_primary())
                    .child(before),
            )
            .when(focused, |d| {
                d.child(
                    div()
                        .w(px(1.5))
                        .h(px(15.0))
                        .bg(Colors::accent_primary())
                        .rounded_sm(),
                )
            })
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(Colors::text_primary())
                    .child(after),
            )
            .into_any_element()
    };

    div()
        .id(state.element_id)
        .track_focus(&fh_track)
        .flex()
        .flex_row()
        .items_center()
        .w_full()
        .h(px(28.0))
        .px(px(10.0))
        .rounded_md()
        .border(px(1.0))
        .border_color(border)
        .bg(bg)
        .cursor(gpui::CursorStyle::IBeam)
        .on_mouse_down(MouseButton::Left, move |_, window, _cx| {
            fh_click.focus(window);
        })
        .child(content)
}
