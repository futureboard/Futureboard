//! SoundFont Player MDI document UI.
//!
//! Feature set (preset browser, volume, reverb/chorus, polyphony, keyboard
//! range) is modeled after General MIDI soundfont players like Fruity
//! Soundfont Player / LiveSynth Pro. The visual language is Futureboard's own
//! flat/dark/token-driven chrome — no skeuomorphic reproduction of another
//! plugin's skin (see `DESIGN.md` / `tasks/SKILL.md` "no copied plugin
//! branding"). All preset/bank data comes from
//! [`crate::soundfont_player::SoundfontPresetInfo`] (`SphereSoundfontPlayer`,
//! no gpui dependency) — this file only renders it.

use std::sync::Arc;

use gpui::{
    div, px, svg, AnyElement, App, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window,
};

use crate::assets;
use crate::components::controls::{fb_button, fb_checkbox, fb_stepper_button, FbButtonKind};
use crate::components::mdi::{
    mdi_workspace, MdiDocumentKind, MdiWorkspaceCallbacks, MdiWorkspaceState,
};
use crate::components::slider::slider;
use crate::soundfont_player::SoundfontPresetInfo;
use crate::theme::Colors;

pub const SOUNDFONT_PLAYER_MDI_TITLE: &str = "Soundfont Player";

/// Preset browser + engine control state for one Soundfont Player document.
/// Transient UI state — the real preset/bank data and player instance live in
/// [`crate::components::soundfont_player_window::SoundfontPlayerWindow`].
#[derive(Clone)]
pub struct SoundfontPlayerPanelState {
    pub file_name: Option<String>,
    pub bank_name: Option<String>,
    pub presets: Vec<SoundfontPresetInfo>,
    pub selected_preset: Option<(i32, i32)>,
    pub master_volume: f32,
    pub reverb_chorus: bool,
    pub polyphony: usize,
    pub preset_list_open: bool,
    pub loading: bool,
    pub status: Option<String>,
}

impl Default for SoundfontPlayerPanelState {
    fn default() -> Self {
        Self {
            file_name: None,
            bank_name: None,
            presets: Vec::new(),
            selected_preset: None,
            master_volume: 1.0,
            reverb_chorus: true,
            polyphony: 64,
            preset_list_open: false,
            loading: false,
            status: None,
        }
    }
}

type SoundfontVoidCb = Arc<dyn Fn(&mut Window, &mut App) + 'static>;
type SoundfontPresetCb = Arc<dyn Fn(&(i32, i32), &mut Window, &mut App) + 'static>;
type SoundfontF32Cb = Arc<dyn Fn(&f32, &mut Window, &mut App) + 'static>;
type SoundfontUsizeCb = Arc<dyn Fn(&usize, &mut Window, &mut App) + 'static>;

#[derive(Clone)]
pub struct SoundfontPlayerCallbacks {
    pub on_browse: SoundfontVoidCb,
    pub on_toggle_preset_list: SoundfontVoidCb,
    pub on_select_preset: SoundfontPresetCb,
    pub on_set_volume: SoundfontF32Cb,
    pub on_toggle_reverb_chorus: SoundfontVoidCb,
    pub on_set_polyphony: SoundfontUsizeCb,
}

const PRESET_LIST_MAX_H: f32 = 150.0;
const POLYPHONY_MIN: usize = 1;
const POLYPHONY_MAX: usize = 256;
const POLYPHONY_STEP: usize = 8;

pub fn ensure_soundfont_player_document(state: &mut MdiWorkspaceState) -> String {
    if let Some(existing) = state
        .documents
        .iter()
        .find(|doc| doc.kind == MdiDocumentKind::SoundfontPlayer)
        .map(|doc| doc.id.clone())
    {
        state.restore_document(&existing);
        return existing;
    }
    state.open_document(MdiDocumentKind::SoundfontPlayer, SOUNDFONT_PLAYER_MDI_TITLE)
}

pub fn soundfont_player_mdi_workspace(
    state: &MdiWorkspaceState,
    callbacks: MdiWorkspaceCallbacks,
    panel: &SoundfontPlayerPanelState,
    panel_callbacks: SoundfontPlayerCallbacks,
) -> AnyElement {
    mdi_workspace(state, callbacks, |doc| match doc.kind {
        MdiDocumentKind::SoundfontPlayer => soundfont_player_panel(panel, panel_callbacks.clone()),
        MdiDocumentKind::Generic => empty_document(),
    })
}

pub fn soundfont_player_panel(
    panel: &SoundfontPlayerPanelState,
    cb: SoundfontPlayerCallbacks,
) -> AnyElement {
    let title = panel
        .bank_name
        .clone()
        .unwrap_or_else(|| "Soundfont Player".to_string());
    let subtitle = if panel.loading {
        "Loading…".to_string()
    } else {
        panel
            .file_name
            .clone()
            .unwrap_or_else(|| "No SoundFont loaded".to_string())
    };

    let mut column = div()
        .flex()
        .flex_col()
        .size_full()
        .bg(Colors::surface_window())
        .p(px(20.0))
        .gap(px(8.0))
        .child(header_row(&title, &subtitle))
        .child(soundfont_row(panel, &cb))
        .child(preset_row(panel, &cb));

    if panel.preset_list_open {
        column = column.child(preset_list(panel, &cb));
    }

    column = column
        .child(volume_row(panel, &cb))
        .child(engine_row(panel, &cb));

    if let Some(status) = panel.status.clone() {
        column = column.child(status_banner(status));
    }

    column
        .child(
            div()
                .mt(px(2.0))
                .h(px(1.0))
                .w_full()
                .bg(Colors::border_subtle()),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(5.0))
                .child(
                    div()
                        .text_size(px(10.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(Colors::text_faint())
                        .child("Keyboard Range"),
                )
                .child(keyboard_preview()),
        )
        .into_any_element()
}

fn header_row(title: &str, subtitle: &str) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(
            svg()
                .path(assets::ICON_MUSIC_PATH)
                .w(px(16.0))
                .h(px(16.0))
                .text_color(Colors::accent_primary()),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .min_w(px(0.0))
                .gap(px(2.0))
                .child(
                    div()
                        .truncate()
                        .text_size(px(12.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(Colors::text_primary())
                        .child(title.to_string()),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(px(10.5))
                        .text_color(Colors::text_muted())
                        .child(subtitle.to_string()),
                ),
        )
        .into_any_element()
}

fn field_row(
    label: &'static str,
    value: impl IntoElement,
    action: Option<AnyElement>,
) -> AnyElement {
    let mut row = div()
        .flex()
        .flex_row()
        .items_center()
        .min_h(px(26.0))
        .gap(px(8.0))
        .child(
            div()
                .w(px(64.0))
                .flex_shrink_0()
                .text_size(px(10.5))
                .text_color(Colors::text_muted())
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .h(px(24.0))
                .flex()
                .items_center()
                .truncate()
                .px(px(8.0))
                .rounded_md()
                .border(px(1.0))
                .border_color(Colors::border_subtle())
                .bg(Colors::surface_input())
                .text_size(px(10.5))
                .text_color(Colors::text_secondary())
                .child(value),
        );
    if let Some(action) = action {
        row = row.child(action);
    }
    row.into_any_element()
}

fn soundfont_row(panel: &SoundfontPlayerPanelState, cb: &SoundfontPlayerCallbacks) -> AnyElement {
    let browse = cb.on_browse.clone();
    let value = panel
        .file_name
        .clone()
        .unwrap_or_else(|| "No .sf2 loaded".to_string());
    field_row(
        "SoundFont",
        value,
        Some(
            fb_button(
                "soundfont-browse",
                "Browse…",
                FbButtonKind::Default,
                !panel.loading,
                move |_, w, cx| browse(w, cx),
            )
            .into_any_element(),
        ),
    )
}

fn preset_row(panel: &SoundfontPlayerPanelState, cb: &SoundfontPlayerCallbacks) -> AnyElement {
    let has_presets = !panel.presets.is_empty();
    let value = if let Some((bank, patch)) = panel.selected_preset {
        panel
            .presets
            .iter()
            .find(|preset| preset.bank == bank && preset.patch == patch)
            .map(|preset| format!("{} — Bank {} / Patch {}", preset.name, bank, patch))
            .unwrap_or_else(|| "—".to_string())
    } else if has_presets {
        "No preset selected".to_string()
    } else {
        "—".to_string()
    };
    let toggle = cb.on_toggle_preset_list.clone();
    let toggle_label = if panel.preset_list_open {
        "Close"
    } else {
        "Choose…"
    };
    field_row(
        "Preset",
        value,
        Some(
            fb_button(
                "soundfont-preset-toggle",
                toggle_label,
                FbButtonKind::Default,
                has_presets,
                move |_, w, cx| toggle(w, cx),
            )
            .into_any_element(),
        ),
    )
}

fn preset_list(panel: &SoundfontPlayerPanelState, cb: &SoundfontPlayerCallbacks) -> AnyElement {
    let select = cb.on_select_preset.clone();
    div()
        .id("soundfont-preset-list")
        .flex()
        .flex_col()
        .max_h(px(PRESET_LIST_MAX_H))
        .overflow_y_scroll()
        .rounded_md()
        .border(px(1.0))
        .border_color(Colors::border_subtle())
        .bg(Colors::surface_card())
        .children(panel.presets.iter().enumerate().map(|(index, preset)| {
            let active = panel.selected_preset == Some((preset.bank, preset.patch));
            let key = (preset.bank, preset.patch);
            let select = select.clone();
            div()
                .id(("soundfont-preset-item", index))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .h(px(24.0))
                .px(px(8.0))
                .cursor(gpui::CursorStyle::PointingHand)
                .bg(if active {
                    Colors::accent_muted()
                } else {
                    Colors::surface_card()
                })
                .hover(|s| s.bg(Colors::surface_hover()))
                .on_click(move |_, w, cx| select(&key, w, cx))
                .child(
                    div()
                        .w(px(64.0))
                        .flex_shrink_0()
                        .text_size(px(9.5))
                        .text_color(Colors::text_faint())
                        .child(format!("{}:{}", preset.bank, preset.patch)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .truncate()
                        .text_size(px(11.0))
                        .text_color(if active {
                            Colors::text_primary()
                        } else {
                            Colors::text_secondary()
                        })
                        .child(preset.name.clone()),
                )
        }))
        .into_any_element()
}

fn volume_row(panel: &SoundfontPlayerPanelState, cb: &SoundfontPlayerCallbacks) -> AnyElement {
    let on_change = cb.on_set_volume.clone();
    div()
        .flex()
        .flex_row()
        .items_center()
        .min_h(px(26.0))
        .gap(px(8.0))
        .child(
            div()
                .w(px(64.0))
                .flex_shrink_0()
                .text_size(px(10.5))
                .text_color(Colors::text_muted())
                .child("Volume"),
        )
        .child(slider(
            "soundfont-volume",
            panel.master_volume,
            Colors::accent_primary(),
            move |value, w, cx| on_change(value, w, cx),
        ))
        .child(
            div()
                .w(px(36.0))
                .flex_shrink_0()
                .text_size(px(10.0))
                .text_color(Colors::text_faint())
                .child(format!("{:.0}%", panel.master_volume * 100.0)),
        )
        .into_any_element()
}

fn engine_row(panel: &SoundfontPlayerPanelState, cb: &SoundfontPlayerCallbacks) -> AnyElement {
    let toggle_reverb = cb.on_toggle_reverb_chorus.clone();
    let set_polyphony_dec = cb.on_set_polyphony.clone();
    let set_polyphony_inc = cb.on_set_polyphony.clone();
    let polyphony = panel.polyphony;
    let dec_value = polyphony.saturating_sub(POLYPHONY_STEP).max(POLYPHONY_MIN);
    let inc_value = (polyphony + POLYPHONY_STEP).min(POLYPHONY_MAX);

    div()
        .flex()
        .flex_row()
        .items_center()
        .min_h(px(28.0))
        .gap(px(14.0))
        .child(fb_checkbox(
            "soundfont-reverb-chorus",
            "Reverb & Chorus",
            panel.reverb_chorus,
            !panel.loading,
            move |_, w, cx| toggle_reverb(w, cx),
        ))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(Colors::text_muted())
                        .child("Polyphony"),
                )
                .child(fb_stepper_button(
                    "soundfont-polyphony-dec",
                    "–",
                    move |_, w, cx| set_polyphony_dec(&dec_value, w, cx),
                ))
                .child(
                    div()
                        .w(px(32.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(11.0))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(Colors::text_primary())
                        .child(polyphony.to_string()),
                )
                .child(fb_stepper_button(
                    "soundfont-polyphony-inc",
                    "+",
                    move |_, w, cx| set_polyphony_inc(&inc_value, w, cx),
                )),
        )
        .into_any_element()
}

fn status_banner(message: String) -> AnyElement {
    div()
        .px(px(8.0))
        .py(px(5.0))
        .rounded_md()
        .border(px(1.0))
        .border_color(Colors::status_error())
        .bg(Colors::with_alpha(Colors::status_error(), 0.12))
        .text_size(px(10.5))
        .text_color(Colors::status_error())
        .child(message)
        .into_any_element()
}

fn keyboard_preview() -> AnyElement {
    let mut row = div()
        .flex()
        .flex_row()
        .h(px(48.0))
        .border(px(1.0))
        .border_color(Colors::border_subtle())
        .bg(Colors::surface_muted())
        .overflow_hidden()
        .rounded_md();

    for i in 0..18 {
        let is_dark = matches!(i % 7, 1 | 3 | 6);
        row = row.child(
            div()
                .flex_1()
                .h_full()
                .border_l(px(if i == 0 { 0.0 } else { 1.0 }))
                .border_color(Colors::border_subtle())
                .bg(if is_dark {
                    Colors::surface_base()
                } else {
                    Colors::surface_input()
                }),
        );
    }

    row.into_any_element()
}

fn empty_document() -> AnyElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(11.0))
        .text_color(Colors::text_muted())
        .child("Empty document")
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_soundfont_player_reuses_existing_document() {
        let mut state = MdiWorkspaceState::default();
        let first = ensure_soundfont_player_document(&mut state);
        let second = ensure_soundfont_player_document(&mut state);
        assert_eq!(first, second);
        assert_eq!(state.document_count(), 1);
    }
}
