use gpui::KeyDownEvent;

use crate::keymap::event_to_accel_string;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyRecorderState {
    pub armed: bool,
    pub captured: Option<String>,
    pub error: Option<String>,
}

impl KeyRecorderState {
    pub fn arm(&mut self) {
        self.armed = true;
        self.captured = None;
        self.error = None;
    }

    pub fn disarm(&mut self) {
        self.armed = false;
    }

    pub fn clear(&mut self) {
        self.captured = None;
        self.error = None;
        self.armed = false;
    }

    pub fn handle_key(&mut self, event: &KeyDownEvent) -> bool {
        if !self.armed {
            return false;
        }
        if event.is_held {
            return true;
        }
        let key = event.keystroke.key.as_str();
        if key == "escape" {
            self.disarm();
            return true;
        }
        if let Some(accel) = event_to_accel_string(event) {
            self.captured = Some(accel);
            self.armed = false;
            return true;
        }
        self.error = Some("Unsupported key chord".to_string());
        true
    }
}

pub fn key_recorder_field(
    state: &KeyRecorderState,
    placeholder: &str,
    armed: bool,
) -> gpui::AnyElement {
    use gpui::{div, px, IntoElement, ParentElement, Styled};
    use crate::theme::Colors;

    let label = state
        .captured
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            if armed {
                "Press keys…".to_string()
            } else {
                placeholder.to_string()
            }
        });

    div()
        .flex()
        .items_center()
        .h(px(28.0))
        .px(px(8.0))
        .rounded_md()
        .bg(Colors::surface_input())
        .border(px(1.0))
        .border_color(if armed {
            Colors::border_focus()
        } else {
            Colors::border_subtle()
        })
        .text_size(px(11.0))
        .text_color(if state.captured.is_some() {
            Colors::text_primary()
        } else {
            Colors::text_muted()
        })
        .child(label)
        .into_any_element()
}
