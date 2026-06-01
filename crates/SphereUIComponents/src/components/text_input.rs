use std::ops::Range;
use std::sync::Arc;

use gpui::prelude::FluentBuilder;
use gpui::{
    div, point, px, relative, App, Bounds, ClipboardItem, Element, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, FocusHandle, GlobalElementId, InteractiveElement, IntoElement,
    KeyDownEvent, LayoutId, MouseButton, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels,
    Style, Styled, UTF16Selection, Window,
};

use crate::components::context_menu::ContextMenuEntry;
use crate::theme::Colors;

pub const TEXT_INPUT_CUT: &str = "text-input:cut";
pub const TEXT_INPUT_COPY: &str = "text-input:copy";
pub const TEXT_INPUT_PASTE: &str = "text-input:paste";
pub const TEXT_INPUT_SELECT_ALL: &str = "text-input:select-all";
const TEXT_INPUT_PAD_X: f32 = 9.0;
const TEXT_INPUT_CHAR_W: f32 = 7.0;

pub type TextInputContextCb = Arc<dyn Fn(&(f32, f32), &mut Window, &mut App) + 'static>;
pub type TextInputMouseCb = Arc<dyn Fn(&TextInputMouseEvent, &mut Window, &mut App) + 'static>;

#[derive(Clone, Default)]
pub struct TextInputCallbacks {
    pub on_context_menu: Option<TextInputContextCb>,
    pub on_mouse: Option<TextInputMouseCb>,
}

#[derive(Debug, Clone, Copy)]
pub enum TextInputMousePhase {
    Down,
    Drag,
    Up,
}

#[derive(Debug, Clone, Copy)]
pub struct TextInputMouseEvent {
    pub phase: TextInputMousePhase,
    /// X position in the input's local coordinate space.
    pub x: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextInputAction {
    Consumed,
    Submit,
    Cancel,
    Pass,
}

#[derive(Clone)]
pub struct TextInputState {
    pub element_id: &'static str,
    pub focus_handle: FocusHandle,
    pub value: String,
    pub cursor: usize,
    selection_anchor: Option<usize>,
    pub placeholder: Option<String>,
    pub disabled: bool,
    pub read_only: bool,
    pub is_password: bool,
    mouse_selecting: bool,
    marked_range: Option<Range<usize>>,
}

impl TextInputState {
    pub fn new(element_id: &'static str, focus_handle: FocusHandle) -> Self {
        Self {
            element_id,
            focus_handle,
            value: String::new(),
            cursor: 0,
            selection_anchor: None,
            placeholder: None,
            disabled: false,
            read_only: false,
            is_password: false,
            mouse_selecting: false,
            marked_range: None,
        }
    }

    pub fn with_placeholder(mut self, p: impl Into<String>) -> Self {
        self.placeholder = Some(p.into());
        self
    }

    pub fn set_value(&mut self, v: impl Into<String>) {
        self.value = v.into();
        self.cursor = self.char_count();
        self.clear_selection();
        self.unmark_text();
    }

    pub fn set_disabled(&mut self, disabled: bool) {
        self.disabled = disabled;
    }

    pub fn set_read_only(&mut self, read_only: bool) {
        self.read_only = read_only;
    }

    pub fn set_password(&mut self, is_password: bool) {
        self.is_password = is_password;
    }

    pub fn select_all(&mut self) {
        let count = self.char_count();
        if count > 0 {
            self.selection_anchor = Some(0);
            self.cursor = count;
        }
    }

    pub fn clear_selection(&mut self) {
        self.selection_anchor = None;
    }

    pub fn unmark_text(&mut self) {
        self.marked_range = None;
    }

    pub fn has_selection(&self) -> bool {
        self.selection_range().is_some()
    }

    pub fn can_cut(&self) -> bool {
        !self.disabled && !self.read_only && self.has_selection()
    }

    pub fn can_copy(&self) -> bool {
        !self.disabled && self.has_selection()
    }

    pub fn can_paste(&self) -> bool {
        !self.disabled && !self.read_only
    }

    pub fn can_select_all(&self) -> bool {
        !self.disabled && !self.value.is_empty()
    }

    pub fn is_focused(&self, window: &Window) -> bool {
        self.focus_handle.is_focused(window)
    }

    /// Begin mouse selection/cursor placement.
    ///
    /// Note: `x` is treated as local-x within the text field. Since GPUI does not
    /// currently expose element-local coordinates here, we use a best-effort mapping
    /// that is "good enough" for drag selection.
    pub fn handle_mouse_down(&mut self, x: f32, extend: bool) {
        if self.disabled {
            return;
        }
        let idx = self.cursor_from_x(x);
        if extend {
            self.move_cursor_to(idx, true);
        } else {
            self.unmark_text();
            self.cursor = idx;
            self.selection_anchor = Some(idx);
        }
        self.mouse_selecting = true;
    }

    pub fn handle_mouse_drag(&mut self, x: f32) {
        if self.disabled || !self.mouse_selecting {
            return;
        }
        let idx = self.cursor_from_x(x);
        self.move_cursor_to(idx, true);
    }

    pub fn handle_mouse_up(&mut self) {
        self.mouse_selecting = false;
        // Collapse empty selection created by click.
        if self.selection_anchor == Some(self.cursor) {
            self.clear_selection();
        }
    }

    fn cursor_from_x(&self, x: f32) -> usize {
        let local = (x - TEXT_INPUT_PAD_X).max(0.0);
        let idx = (local / TEXT_INPUT_CHAR_W).round() as isize;
        idx.clamp(0, self.char_count() as isize) as usize
    }

    pub fn handle_key(&mut self, event: &KeyDownEvent) -> TextInputAction {
        self.handle_key_with_clipboard(event, None)
    }

    pub fn handle_key_with_clipboard(
        &mut self,
        event: &KeyDownEvent,
        cx: Option<&mut App>,
    ) -> TextInputAction {
        if self.disabled {
            return TextInputAction::Pass;
        }

        let key = event.keystroke.key.as_str();
        let mods = event.keystroke.modifiers;
        let command = mods.control || mods.platform;

        if command && !mods.alt && !mods.function {
            return match key {
                "a" | "A" => {
                    self.select_all();
                    TextInputAction::Consumed
                }
                "c" | "C" => {
                    if self.copy_to_clipboard(cx) {
                        TextInputAction::Consumed
                    } else {
                        TextInputAction::Pass
                    }
                }
                "x" | "X" => {
                    if self.cut_to_clipboard(cx) {
                        TextInputAction::Consumed
                    } else {
                        TextInputAction::Pass
                    }
                }
                "v" | "V" => {
                    if self.paste_from_clipboard(cx) {
                        TextInputAction::Consumed
                    } else {
                        TextInputAction::Pass
                    }
                }
                _ => TextInputAction::Pass,
            };
        }

        match key {
            "backspace" if !self.read_only => {
                self.delete_backward();
                TextInputAction::Consumed
            }
            "delete" if !self.read_only => {
                self.delete_forward();
                TextInputAction::Consumed
            }
            "left" | "arrow_left" => {
                self.move_cursor_left(mods.shift);
                TextInputAction::Consumed
            }
            "right" | "arrow_right" => {
                self.move_cursor_right(mods.shift);
                TextInputAction::Consumed
            }
            "home" => {
                self.move_cursor_to(0, mods.shift);
                TextInputAction::Consumed
            }
            "end" => {
                self.move_cursor_to(self.char_count(), mods.shift);
                TextInputAction::Consumed
            }
            "enter" | "numpad_enter" => TextInputAction::Submit,
            "escape" => TextInputAction::Cancel,
            _ if !self.read_only => {
                if let Some(text) = printable_text(key) {
                    self.insert_str(text);
                    TextInputAction::Consumed
                } else {
                    TextInputAction::Pass
                }
            }
            _ => TextInputAction::Pass,
        }
    }

    pub fn apply_context_command(&mut self, command: &str, cx: &mut App) -> bool {
        match command {
            TEXT_INPUT_CUT => self.cut_to_clipboard(Some(cx)),
            TEXT_INPUT_COPY => self.copy_to_clipboard(Some(cx)),
            TEXT_INPUT_PASTE => self.paste_from_clipboard(Some(cx)),
            TEXT_INPUT_SELECT_ALL => {
                self.select_all();
                true
            }
            _ => false,
        }
    }

    pub fn selected_text(&self) -> Option<String> {
        let range = self.selection_range()?;
        Some(self.slice_chars(range))
    }

    fn copy_to_clipboard(&self, cx: Option<&mut App>) -> bool {
        let Some(text) = self.selected_text() else {
            return false;
        };
        let Some(cx) = cx else {
            return false;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        true
    }

    fn cut_to_clipboard(&mut self, cx: Option<&mut App>) -> bool {
        if self.read_only || self.disabled {
            return false;
        }
        let Some(text) = self.selected_text() else {
            return false;
        };
        let Some(cx) = cx else {
            return false;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.delete_selection();
        true
    }

    fn paste_from_clipboard(&mut self, cx: Option<&mut App>) -> bool {
        if self.read_only || self.disabled {
            return false;
        }
        let Some(cx) = cx else {
            return false;
        };
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return false;
        };
        self.insert_str(&text);
        true
    }

    fn char_count(&self) -> usize {
        self.value.chars().count()
    }

    fn char_to_utf16(&self, char_idx: usize) -> usize {
        self.value
            .chars()
            .take(char_idx.min(self.char_count()))
            .map(char::len_utf16)
            .sum()
    }

    fn char_from_utf16(&self, utf16_idx: usize) -> usize {
        let mut utf16_count = 0usize;
        let mut char_count = 0usize;
        for ch in self.value.chars() {
            let next = utf16_count + ch.len_utf16();
            if next > utf16_idx {
                break;
            }
            utf16_count = next;
            char_count += 1;
        }
        char_count
    }

    fn range_to_utf16(&self, range: Range<usize>) -> Range<usize> {
        self.char_to_utf16(range.start)..self.char_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range: Range<usize>) -> Range<usize> {
        let start = self.char_from_utf16(range.start);
        let end = self.char_from_utf16(range.end);
        start.min(end)..start.max(end)
    }

    pub fn text_for_utf16_range(
        &self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
    ) -> Option<String> {
        let range = self.range_from_utf16(range_utf16);
        actual_range.replace(self.range_to_utf16(range.clone()));
        Some(self.slice_chars(range))
    }

    pub fn selected_text_range_utf16(&self, ignore_disabled_input: bool) -> Option<UTF16Selection> {
        if self.disabled && !ignore_disabled_input {
            return None;
        }
        let anchor = self.selection_anchor.unwrap_or(self.cursor);
        let range = anchor.min(self.cursor)..anchor.max(self.cursor);
        Some(UTF16Selection {
            range: self.range_to_utf16(range),
            reversed: anchor > self.cursor,
        })
    }

    pub fn marked_text_range_utf16(&self) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range.clone()))
    }

    pub fn replace_text_in_utf16_range(&mut self, range_utf16: Option<Range<usize>>, text: &str) {
        if self.disabled || self.read_only {
            return;
        }
        let range = range_utf16
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked_range.clone())
            .or_else(|| self.selection_range())
            .unwrap_or(self.cursor..self.cursor);
        self.replace_char_range(range.clone(), text);
        self.cursor = range.start + text.chars().count();
        self.selection_anchor = None;
        self.marked_range = None;
    }

    pub fn replace_and_mark_text_in_utf16_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        selected_range_utf16: Option<Range<usize>>,
    ) {
        if self.disabled || self.read_only {
            return;
        }
        let range = range_utf16
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked_range.clone())
            .or_else(|| self.selection_range())
            .unwrap_or(self.cursor..self.cursor);
        let start = range.start;
        let text_chars = text.chars().count();
        self.replace_char_range(range, text);
        self.marked_range = (text_chars > 0).then_some(start..start + text_chars);

        if let Some(selected_range_utf16) = selected_range_utf16 {
            let selected = char_range_from_utf16_text(text, selected_range_utf16);
            self.cursor = start + selected.end.min(text_chars);
            self.selection_anchor =
                (selected.start != selected.end).then_some(start + selected.start.min(text_chars));
        } else {
            self.cursor = start + text_chars;
            self.selection_anchor = None;
        }
    }

    pub fn bounds_for_utf16_range(
        &self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(range_utf16);
        let left = bounds.left() + px(TEXT_INPUT_PAD_X + range.start as f32 * TEXT_INPUT_CHAR_W);
        let right = bounds.left() + px(TEXT_INPUT_PAD_X + range.end as f32 * TEXT_INPUT_CHAR_W);
        Some(Bounds::from_corners(
            point(left, bounds.top()),
            point(right.max(left + px(1.0)), bounds.bottom()),
        ))
    }

    fn display_value(&self) -> String {
        if self.is_password {
            "•".repeat(self.char_count())
        } else {
            self.value.clone()
        }
    }

    fn byte_at_char(&self, char_idx: usize) -> usize {
        self.value
            .char_indices()
            .nth(char_idx)
            .map(|(byte, _)| byte)
            .unwrap_or(self.value.len())
    }

    fn selection_range(&self) -> Option<Range<usize>> {
        let anchor = self.selection_anchor?;
        if anchor == self.cursor {
            return None;
        }
        Some(anchor.min(self.cursor)..anchor.max(self.cursor))
    }

    fn slice_chars(&self, range: Range<usize>) -> String {
        self.value
            .chars()
            .skip(range.start)
            .take(range.end - range.start)
            .collect()
    }

    fn delete_selection(&mut self) -> bool {
        let Some(range) = self.selection_range() else {
            self.clear_selection();
            return false;
        };
        self.unmark_text();
        self.replace_char_range(range.clone(), "");
        self.cursor = range.start;
        self.clear_selection();
        true
    }

    fn replace_char_range(&mut self, range: Range<usize>, text: &str) {
        let start_byte = self.byte_at_char(range.start);
        let end_byte = self.byte_at_char(range.end);
        self.value.replace_range(start_byte..end_byte, text);
    }

    fn insert_str(&mut self, text: &str) {
        self.delete_selection();
        self.unmark_text();
        let byte_pos = self.byte_at_char(self.cursor);
        self.value.insert_str(byte_pos, text);
        self.cursor += text.chars().count();
    }

    fn delete_backward(&mut self) {
        if self.delete_selection() || self.cursor == 0 {
            return;
        }
        self.unmark_text();
        let start = self.byte_at_char(self.cursor - 1);
        let end = self.byte_at_char(self.cursor);
        self.value.replace_range(start..end, "");
        self.cursor -= 1;
    }

    fn delete_forward(&mut self) {
        if self.delete_selection() || self.cursor >= self.char_count() {
            return;
        }
        self.unmark_text();
        let start = self.byte_at_char(self.cursor);
        let end = self.byte_at_char(self.cursor + 1);
        self.value.replace_range(start..end, "");
    }

    fn move_cursor_to(&mut self, position: usize, extend: bool) {
        let position = position.min(self.char_count());
        if extend {
            if self.selection_anchor.is_none() {
                self.selection_anchor = Some(self.cursor);
            }
        } else {
            self.unmark_text();
            self.clear_selection();
        }
        self.cursor = position;
        if self.selection_anchor == Some(self.cursor) {
            self.clear_selection();
        }
    }

    fn move_cursor_left(&mut self, extend: bool) {
        if !extend {
            if let Some(range) = self.selection_range() {
                self.cursor = range.start;
                self.clear_selection();
                return;
            }
        }
        self.move_cursor_to(self.cursor.saturating_sub(1), extend);
    }

    fn move_cursor_right(&mut self, extend: bool) {
        if !extend {
            if let Some(range) = self.selection_range() {
                self.cursor = range.end;
                self.clear_selection();
                return;
            }
        }
        self.move_cursor_to((self.cursor + 1).min(self.char_count()), extend);
    }
}

fn printable_text(key: &str) -> Option<&str> {
    match key {
        "space" => Some(" "),
        k if k.chars().count() == 1 => {
            let c = k.chars().next()?;
            (!c.is_control()).then_some(k)
        }
        _ => None,
    }
}

fn char_range_from_utf16_text(text: &str, range: Range<usize>) -> Range<usize> {
    fn char_from_utf16(text: &str, utf16_idx: usize) -> usize {
        let mut utf16_count = 0usize;
        let mut char_count = 0usize;
        for ch in text.chars() {
            let next = utf16_count + ch.len_utf16();
            if next > utf16_idx {
                break;
            }
            utf16_count = next;
            char_count += 1;
        }
        char_count
    }

    let start = char_from_utf16(text, range.start);
    let end = char_from_utf16(text, range.end);
    start.min(end)..start.max(end)
}

pub fn text_input_context_entries(
    state: &TextInputState,
    clipboard_has_text: bool,
) -> Vec<ContextMenuEntry> {
    vec![
        menu_item("Cut", TEXT_INPUT_CUT, state.can_cut(), Some("Ctrl+X")),
        menu_item("Copy", TEXT_INPUT_COPY, state.can_copy(), Some("Ctrl+C")),
        menu_item(
            "Paste",
            TEXT_INPUT_PASTE,
            state.can_paste() && clipboard_has_text,
            Some("Ctrl+V"),
        ),
        ContextMenuEntry::Separator,
        menu_item(
            "Select All",
            TEXT_INPUT_SELECT_ALL,
            state.can_select_all(),
            Some("Ctrl+A"),
        ),
    ]
}

/// Convenience helper: binds mouse drag selection to a `TextInputState` stored
/// inside an entity, without duplicating handler boilerplate.
pub fn bind_mouse_selection<T: gpui::Render>(
    target: gpui::Entity<T>,
    get: impl Fn(&mut T) -> &mut TextInputState + Send + Sync + 'static,
) -> TextInputCallbacks {
    bind_mouse_selection_with_offset(target, get, 0.0)
}

/// Same as [`bind_mouse_selection`], with a known window-local x offset for
/// text fields nested in labeled form rows.
pub fn bind_mouse_selection_with_offset<T: gpui::Render>(
    target: gpui::Entity<T>,
    get: impl Fn(&mut T) -> &mut TextInputState + Send + Sync + 'static,
    local_x_offset: f32,
) -> TextInputCallbacks {
    TextInputCallbacks {
        on_context_menu: None,
        on_mouse: Some(Arc::new(move |event: &TextInputMouseEvent, _w, cx| {
            let x = event.x - local_x_offset;
            let phase = event.phase;
            let _ = target.update(cx, |this, cx| {
                let input = get(this);
                match phase {
                    TextInputMousePhase::Down => input.handle_mouse_down(x, false),
                    TextInputMousePhase::Drag => input.handle_mouse_drag(x),
                    TextInputMousePhase::Up => input.handle_mouse_up(),
                }
                cx.notify();
            });
        })),
    }
}

fn menu_item(
    label: &'static str,
    command: &'static str,
    enabled: bool,
    shortcut: Option<&'static str>,
) -> ContextMenuEntry {
    let item = if enabled {
        ContextMenuEntry::item(label, command)
    } else {
        ContextMenuEntry::disabled_item(label, command)
    };
    if let Some(shortcut) = shortcut {
        item.with_shortcut(shortcut)
    } else {
        item
    }
}

pub fn text_field(state: &TextInputState, focused: bool) -> impl IntoElement {
    text_field_with_callbacks(state, focused, TextInputCallbacks::default())
}

pub fn text_field_with_callbacks(
    state: &TextInputState,
    focused: bool,
    callbacks: TextInputCallbacks,
) -> impl IntoElement {
    text_field_inner(state, focused, callbacks, None)
}

pub fn text_field_with_callbacks_and_ime<V: EntityInputHandler>(
    state: &TextInputState,
    focused: bool,
    callbacks: TextInputCallbacks,
    ime_target: Entity<V>,
) -> impl IntoElement {
    text_field_inner(
        state,
        focused,
        callbacks,
        Some(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left_0()
                .right_0()
                .child(TextInputImeLayer {
                    target: ime_target,
                    focus_handle: state.focus_handle.clone(),
                })
                .into_any_element(),
        ),
    )
}

fn text_field_inner(
    state: &TextInputState,
    focused: bool,
    callbacks: TextInputCallbacks,
    ime_layer: Option<gpui::AnyElement>,
) -> impl IntoElement {
    let fh_click = state.focus_handle.clone();
    let fh_right = state.focus_handle.clone();
    let fh_track = state.focus_handle.clone();
    let disabled = state.disabled;
    let focused = focused && !disabled;
    let value = state.display_value();
    let placeholder = state.placeholder.clone().unwrap_or_default();
    let selection = state.selection_range();
    let cursor = state.cursor.min(value.chars().count());
    let on_context_menu = callbacks.on_context_menu.clone();
    let on_mouse_down = callbacks.on_mouse.clone();
    let on_mouse_move = callbacks.on_mouse.clone();
    let on_mouse_up = callbacks.on_mouse.clone();
    let on_mouse_up_out = callbacks.on_mouse.clone();

    let border = if focused {
        Colors::border_focus()
    } else {
        Colors::border_subtle()
    };
    let bg = if focused {
        Colors::surface_card()
    } else {
        Colors::surface_input()
    };

    let content: gpui::AnyElement = if value.is_empty() {
        div()
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(12.0))
                    .text_color(Colors::text_faint())
                    .child(placeholder),
            )
            .when(focused, |d| d.child(caret()))
            .into_any_element()
    } else if let Some(range) = selection {
        let before: String = value.chars().take(range.start).collect();
        let selected: String = value
            .chars()
            .skip(range.start)
            .take(range.end - range.start)
            .collect();
        let after: String = value.chars().skip(range.end).collect();
        div()
            .flex()
            .flex_row()
            .items_center()
            .min_w_0()
            .overflow_hidden()
            .child(text_segment(before, false))
            .when(cursor == range.start && focused, |d| d.child(caret()))
            .child(text_segment(selected, true))
            .when(cursor == range.end && focused, |d| d.child(caret()))
            .child(text_segment(after, false))
            .into_any_element()
    } else if let Some(range) = state.marked_range.clone() {
        let before: String = value.chars().take(range.start).collect();
        let marked: String = value
            .chars()
            .skip(range.start)
            .take(range.end - range.start)
            .collect();
        let after: String = value.chars().skip(range.end).collect();
        div()
            .flex()
            .flex_row()
            .items_center()
            .min_w_0()
            .overflow_hidden()
            .child(text_segment(before, false))
            .child(text_segment(marked, true))
            .when(focused, |d| d.child(caret()))
            .child(text_segment(after, false))
            .into_any_element()
    } else {
        let before: String = value.chars().take(cursor).collect();
        let after: String = value.chars().skip(cursor).collect();
        div()
            .flex()
            .flex_row()
            .items_center()
            .min_w_0()
            .overflow_hidden()
            .child(text_segment(before, false))
            .when(focused, |d| d.child(caret()))
            .child(text_segment(after, false))
            .into_any_element()
    };

    div()
        .id(state.element_id)
        .relative()
        .track_focus(&fh_track)
        .flex()
        .flex_row()
        .items_center()
        .w_full()
        .h(px(30.0))
        .px(px(9.0))
        .rounded_md()
        .overflow_hidden()
        .border(px(1.0))
        .border_color(border)
        .bg(bg)
        .opacity(if disabled { 0.48 } else { 1.0 })
        .cursor(if disabled {
            gpui::CursorStyle::Arrow
        } else {
            gpui::CursorStyle::IBeam
        })
        .when(focused, |this| {
            this.shadow(vec![gpui::BoxShadow {
                color: Colors::with_alpha(Colors::border_focus(), 0.15).into(),
                offset: gpui::point(px(0.0), px(0.0)),
                blur_radius: px(0.0),
                spread_radius: px(1.0),
            }])
        })
        .on_mouse_down(MouseButton::Left, move |event, window, cx| {
            if !disabled {
                fh_click.focus(window);
                cx.stop_propagation();
                if let Some(cb) = on_mouse_down.as_ref() {
                    let x: f32 = event.position.x.into();
                    cb(
                        &TextInputMouseEvent {
                            phase: TextInputMousePhase::Down,
                            x,
                        },
                        window,
                        cx,
                    );
                }
            }
        })
        .on_mouse_move(move |event: &MouseMoveEvent, window, cx| {
            if disabled {
                return;
            }
            if let Some(cb) = on_mouse_move.as_ref() {
                let x: f32 = event.position.x.into();
                cb(
                    &TextInputMouseEvent {
                        phase: TextInputMousePhase::Drag,
                        x,
                    },
                    window,
                    cx,
                );
            }
        })
        .on_mouse_up(
            MouseButton::Left,
            move |event: &MouseUpEvent, window, cx| {
                if disabled {
                    return;
                }
                if let Some(cb) = on_mouse_up.as_ref() {
                    let x: f32 = event.position.x.into();
                    cb(
                        &TextInputMouseEvent {
                            phase: TextInputMousePhase::Up,
                            x,
                        },
                        window,
                        cx,
                    );
                }
            },
        )
        .on_mouse_up_out(
            MouseButton::Left,
            move |event: &MouseUpEvent, window, cx| {
                if disabled {
                    return;
                }
                if let Some(cb) = on_mouse_up_out.as_ref() {
                    let x: f32 = event.position.x.into();
                    cb(
                        &TextInputMouseEvent {
                            phase: TextInputMousePhase::Up,
                            x,
                        },
                        window,
                        cx,
                    );
                }
            },
        )
        .on_mouse_down(MouseButton::Right, move |event, window, cx| {
            if disabled {
                return;
            }
            fh_right.focus(window);
            if let Some(callback) = on_context_menu.as_ref() {
                let x: f32 = event.position.x.into();
                let y: f32 = event.position.y.into();
                callback(&(x, y), window, cx);
            }
        })
        .child(content)
        .children(ime_layer)
}

fn text_segment(text: String, selected: bool) -> impl IntoElement {
    div()
        .min_w(px(0.0))
        .overflow_hidden()
        .truncate()
        .rounded_sm()
        .when(selected, |d| {
            d.bg(Colors::with_alpha(Colors::accent_primary(), 0.25))
                .px(px(2.0))
        })
        .text_size(px(12.0))
        .text_color(Colors::text_primary())
        .child(text)
}

fn caret() -> impl IntoElement {
    div()
        .flex_none()
        .w(px(1.5))
        .h(px(15.0))
        .bg(Colors::accent_primary())
        .rounded_sm()
}

struct TextInputImeLayer<V: EntityInputHandler> {
    target: Entity<V>,
    focus_handle: FocusHandle,
}

impl<V: EntityInputHandler> IntoElement for TextInputImeLayer<V> {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<V: EntityInputHandler> Element for TextInputImeLayer<V> {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = relative(1.0).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.handle_input(
            &self.focus_handle,
            ElementInputHandler::new(bounds, self.target.clone()),
            cx,
        );
    }
}
