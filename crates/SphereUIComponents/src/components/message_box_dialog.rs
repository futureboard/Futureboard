//! Borderless native message box (Windows only).
//!
//! Mirrors the Electron / web [`MessageBoxOptions`] surface: title, message,
//! optional detail, custom button labels, default/cancel indices, and kind
//! (info / warning / error / question). Opens as a compact floating GPUI window
//! using the same chrome as Add Track / Settings dialogs.

use std::sync::Arc;

use gpui::{
    div, px, size, App, AppContext, Bounds, Context, FocusHandle, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Point, Render, StatefulInteractiveElement, Styled, Window,
    WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind,
};

use crate::components::controls::{fb_button, FbButtonKind};
use crate::components::title_bar::{external_window_titlebar, TITLEBAR_HEIGHT};
use crate::theme::{self, Colors};

pub const MESSAGE_BOX_WIDTH: f32 = 400.0;
const BODY_PAD: f32 = 16.0;
const FOOTER_H: f32 = 46.0;
const MESSAGE_LINE_H: f32 = 20.0;
const DETAIL_LINE_H: f32 = 36.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MessageBoxKind {
    #[default]
    None,
    Info,
    Error,
    Question,
    Warning,
}

#[derive(Debug, Clone)]
pub struct MessageBoxOptions {
    pub kind: MessageBoxKind,
    pub title: String,
    pub message: String,
    pub detail: Option<String>,
    pub buttons: Vec<String>,
    pub default_id: usize,
    pub cancel_id: Option<usize>,
}

impl Default for MessageBoxOptions {
    fn default() -> Self {
        Self {
            kind: MessageBoxKind::None,
            title: String::new(),
            message: String::new(),
            detail: None,
            buttons: vec!["OK".to_string()],
            default_id: 0,
            cancel_id: None,
        }
    }
}

impl MessageBoxOptions {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            ..Default::default()
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn kind(mut self, kind: MessageBoxKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn buttons(mut self, buttons: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.buttons = buttons.into_iter().map(Into::into).collect();
        self
    }

    pub fn default_id(mut self, id: usize) -> Self {
        self.default_id = id;
        self
    }

    pub fn cancel_id(mut self, id: usize) -> Self {
        self.cancel_id = Some(id);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageBoxResult {
    pub response: usize,
}

type ResponseCb = Arc<dyn Fn(MessageBoxResult, &mut Window, &mut App) + Send + Sync>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum MessageBoxButtonStyle {
    Default,
    Primary,
    Destructive,
}

fn message_box_height(options: &MessageBoxOptions) -> f32 {
    let mut h = TITLEBAR_HEIGHT + BODY_PAD + MESSAGE_LINE_H + FOOTER_H;
    if options.detail.as_ref().is_some_and(|d| !d.is_empty()) {
        h += DETAIL_LINE_H;
    }
    h + BODY_PAD
}

fn normalized_buttons(options: &MessageBoxOptions) -> Vec<String> {
    if options.buttons.is_empty() {
        return vec!["OK".to_string()];
    }
    options.buttons.clone()
}

fn clamp_index(index: Option<usize>, len: usize) -> Option<usize> {
    index.filter(|&i| i < len)
}

fn button_style(
    index: usize,
    label: &str,
    options: &MessageBoxOptions,
    len: usize,
) -> MessageBoxButtonStyle {
    if clamp_index(Some(options.default_id), len) == Some(index) {
        return MessageBoxButtonStyle::Primary;
    }
    let lower = label.to_ascii_lowercase();
    if lower.contains("don't save") || lower == "discard" || lower == "delete" {
        return MessageBoxButtonStyle::Destructive;
    }
    MessageBoxButtonStyle::Default
}

fn kind_accent(kind: MessageBoxKind) -> gpui::Rgba {
    match kind {
        MessageBoxKind::Error => Colors::status_error(),
        MessageBoxKind::Warning => Colors::status_warning(),
        MessageBoxKind::Info | MessageBoxKind::Question => Colors::accent_primary(),
        MessageBoxKind::None => Colors::text_muted(),
    }
}

fn kind_glyph(kind: MessageBoxKind) -> &'static str {
    match kind {
        MessageBoxKind::Error => "!",
        MessageBoxKind::Warning => "!",
        MessageBoxKind::Info => "i",
        MessageBoxKind::Question => "?",
        MessageBoxKind::None => "·",
    }
}

fn message_box_body(options: &MessageBoxOptions, on_response: ResponseCb) -> impl IntoElement {
    let buttons = normalized_buttons(options);
    let len = buttons.len();
    let accent = kind_accent(options.kind);
    let glyph = kind_glyph(options.kind);

    let content_row = div()
        .flex()
        .flex_row()
        .gap(px(12.0))
        .px(px(BODY_PAD))
        .pt(px(4.0))
        .child(
            div()
                .flex_shrink_0()
                .w(px(32.0))
                .h(px(32.0))
                .rounded_lg()
                .border(px(1.0))
                .border_color(Colors::with_alpha(accent, 0.35))
                .bg(Colors::with_alpha(accent, 0.12))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(14.0))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(accent)
                .child(glyph),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(Colors::text_primary())
                        .child(options.message.clone()),
                )
                .children(
                    options
                        .detail
                        .as_ref()
                        .filter(|d| !d.is_empty())
                        .map(|detail| {
                            div()
                                .text_size(px(11.0))
                                .line_height(px(16.0))
                                .text_color(Colors::text_muted())
                                .child(detail.clone())
                        }),
                ),
        );

    let mut footer = div()
        .flex()
        .flex_row()
        .justify_end()
        .items_center()
        .gap(px(8.0))
        .h(px(FOOTER_H))
        .px(px(BODY_PAD))
        .border_t(px(1.0))
        .border_color(Colors::border_subtle());

    for (index, label) in buttons.iter().enumerate() {
        let style = button_style(index, label, options, len);
        let on_response = on_response.clone();
        let label = label.clone();
        let btn_id = ("message-box-btn", index);
        let on_click = move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
            on_response(MessageBoxResult { response: index }, window, cx);
        };
        footer = match style {
            MessageBoxButtonStyle::Primary => footer.child(fb_button(
                btn_id,
                label,
                FbButtonKind::Primary,
                true,
                on_click,
            )),
            MessageBoxButtonStyle::Default => footer.child(fb_button(
                btn_id,
                label,
                FbButtonKind::Default,
                true,
                on_click,
            )),
            MessageBoxButtonStyle::Destructive => footer.child(
                div()
                    .id(btn_id)
                    .flex()
                    .items_center()
                    .justify_center()
                    .h(px(30.0))
                    .min_w(px(76.0))
                    .px(px(12.0))
                    .rounded_md()
                    .border(px(1.0))
                    .border_color(Colors::with_alpha(Colors::status_error(), 0.45))
                    .bg(Colors::with_alpha(Colors::status_error(), 0.12))
                    .text_size(px(11.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(Colors::status_error())
                    .cursor(gpui::CursorStyle::PointingHand)
                    .hover(|s| s.bg(Colors::with_alpha(Colors::status_error(), 0.2)))
                    .on_click(on_click)
                    .child(label),
            ),
        };
    }

    div()
        .flex()
        .flex_col()
        .flex_1()
        .child(content_row)
        .child(footer)
}

pub struct MessageBoxWindow {
    options: MessageBoxOptions,
    on_response: ResponseCb,
    focus_handle: FocusHandle,
    responded: bool,
}

impl MessageBoxWindow {
    pub fn new(
        options: MessageBoxOptions,
        on_response: ResponseCb,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            options,
            on_response,
            focus_handle: cx.focus_handle(),
            responded: false,
        }
    }

    fn finish(&mut self, response: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.responded {
            return;
        }
        self.responded = true;
        let cb = self.on_response.clone();
        window.remove_window();
        cb(MessageBoxResult { response }, window, cx);
    }

    fn cancel_response_index(&self) -> usize {
        let len = normalized_buttons(&self.options).len();
        clamp_index(self.options.cancel_id, len)
            .or_else(|| clamp_index(Some(self.options.default_id), len))
            .unwrap_or(0)
    }

    fn handle_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let len = normalized_buttons(&self.options).len();
        match event.keystroke.key.as_str() {
            "escape" => {
                let response = self.cancel_response_index();
                self.finish(response, window, cx);
            }
            "enter" | "numpad_enter" => {
                let response = clamp_index(Some(self.options.default_id), len).unwrap_or(0);
                self.finish(response, window, cx);
            }
            _ => {}
        }
    }
}

impl Render for MessageBoxWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = if self.options.title.is_empty() {
            "Futureboard Studio".to_string()
        } else {
            self.options.title.clone()
        };
        let target = cx.entity().clone();

        div()
            .flex()
            .flex_col()
            .size_full()
            .font_family(theme::FONT_FAMILY)
            .bg(Colors::surface_base())
            .overflow_hidden()
            .rounded_lg()
            .border(px(1.0))
            .border_color(Colors::border_subtle())
            .shadow(vec![gpui::BoxShadow {
                color: Colors::surface_overlay().into(),
                offset: gpui::point(px(0.0), px(12.0)),
                blur_radius: px(36.0),
                spread_radius: px(0.0),
            }])
            .capture_key_down({
                let target = target.clone();
                move |event, window, cx| {
                    let _ = target.update(cx, |this, cx| this.handle_key(event, window, cx));
                }
            })
            .child(div().w(px(0.0)).h(px(0.0)).track_focus(&self.focus_handle))
            .child(external_window_titlebar(title, "message-box-close", {
                let target = target.clone();
                move |window, cx| {
                    let _ = target.update(cx, |this, cx| {
                        this.finish(this.cancel_response_index(), window, cx);
                    });
                }
            }))
            .child(message_box_body(
                &self.options,
                Arc::new({
                    let target = target.clone();
                    move |result, window, cx| {
                        let _ = target.update(cx, |this, cx| {
                            this.finish(result.response, window, cx);
                        });
                    }
                }),
            ))
    }
}

/// Open a borderless message box centered over `owner_bounds`.
///
/// Windows only; returns an error on other platforms.
#[cfg(target_os = "windows")]
pub fn open_message_box_window(
    owner_bounds: Bounds<gpui::Pixels>,
    options: MessageBoxOptions,
    on_response: ResponseCb,
    cx: &mut App,
) -> Result<WindowHandle<MessageBoxWindow>, String> {
    let height = message_box_height(&options);
    let parent_x: f32 = owner_bounds.origin.x.into();
    let parent_y: f32 = owner_bounds.origin.y.into();
    let parent_w: f32 = owner_bounds.size.width.into();
    let parent_h: f32 = owner_bounds.size.height.into();
    let origin = Point {
        x: px(parent_x + ((parent_w - MESSAGE_BOX_WIDTH) / 2.0).max(24.0)),
        y: px(parent_y + ((parent_h - height) / 2.0).max(24.0)),
    };

    let mut window_options = crate::platform_chrome::external_dialog_window_options_partial();
    window_options.window_bounds = Some(WindowBounds::Windowed(Bounds {
        origin,
        size: size(px(MESSAGE_BOX_WIDTH), px(height)),
    }));
    window_options.kind = WindowKind::Floating;
    window_options.is_resizable = false;
    window_options.is_minimizable = false;
    window_options.window_background = WindowBackgroundAppearance::Transparent;

    cx.open_window(window_options, move |_window, cx| {
        cx.new(|cx| MessageBoxWindow::new(options, on_response, cx))
    })
    .map_err(|e| e.to_string())
}

#[cfg(not(target_os = "windows"))]
pub fn open_message_box_window(
    _owner_bounds: Bounds<gpui::Pixels>,
    _options: MessageBoxOptions,
    _on_response: ResponseCb,
    _cx: &mut App,
) -> Result<WindowHandle<MessageBoxWindow>, String> {
    Err("native message box is only available on Windows".to_string())
}

/// Preset matching web unsaved-changes guard (`projectLifecycle.ts`).
#[cfg(target_os = "windows")]
pub fn unsaved_changes_options(project_name: &str, detail: &str) -> MessageBoxOptions {
    MessageBoxOptions {
        kind: MessageBoxKind::Warning,
        title: "Unsaved Changes".to_string(),
        message: format!("Save changes to \"{project_name}\"?"),
        detail: Some(detail.to_string()),
        buttons: vec![
            "Save".to_string(),
            "Don't Save".to_string(),
            "Cancel".to_string(),
        ],
        default_id: 0,
        cancel_id: Some(2),
    }
}

#[cfg(not(target_os = "windows"))]
pub fn unsaved_changes_options(project_name: &str, detail: &str) -> MessageBoxOptions {
    let _ = (project_name, detail);
    MessageBoxOptions::default()
}
