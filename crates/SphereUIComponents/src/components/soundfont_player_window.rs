//! Floating "Soundfont Player" utility window.
//!
//! Hosts the built-in Soundfont Player instrument (see
//! `TrackState::builtin_soundfont_player`) in a simple floating utility window.
//! The player panel fills the window directly — no nested MDI/document chrome.
//!
//! This window owns a real [`SoundfontPlayer`] instance (control/offline
//! side only — see that crate's doc comment). Loading a `.sf2`, browsing its
//! real presets, and switching preset/volume/reverb/polyphony all mutate the
//! live instance. There is no audio device attached to this preview window,
//! so none of this produces sound yet; wiring the built-in player into the
//! audio engine as a real track instrument is a separate, larger task.

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    div, px, size, App, AppContext, Bounds, Context, FocusHandle, InteractiveElement, IntoElement,
    ParentElement, Render, Styled, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle,
    WindowKind,
};

use crate::components::soundfont_player_mdi::{
    soundfont_player_panel, SoundfontPlayerCallbacks, SoundfontPlayerPanelState,
    SOUNDFONT_PLAYER_MDI_TITLE,
};
use crate::components::title_bar::external_window_titlebar;
use crate::soundfont_player::{
    default_soundfont_player_settings, SoundfontPlayer, SoundfontPlayerError,
    SoundfontPlayerSettings,
};
use crate::theme::Colors;

pub const SOUNDFONT_PLAYER_WINDOW_WIDTH: f32 = 640.0;
pub const SOUNDFONT_PLAYER_WINDOW_HEIGHT: f32 = 460.0;
pub const SOUNDFONT_PLAYER_WINDOW_MIN_WIDTH: f32 = 480.0;
pub const SOUNDFONT_PLAYER_WINDOW_MIN_HEIGHT: f32 = 360.0;

const PREVIEW_MIDI_CHANNEL: u8 = 0;

#[derive(Debug, Clone)]
pub struct SoundfontPlayerTrackUpdate {
    pub track_id: String,
    pub path: Option<String>,
    pub preset: Option<(i32, i32)>,
    pub volume: f32,
    pub reverb_chorus: bool,
    pub polyphony: usize,
}

pub struct SoundfontPlayerWindow {
    track_id: String,
    on_close: Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>,
    on_update_track: Arc<dyn Fn(SoundfontPlayerTrackUpdate, &mut App) + Send + Sync>,
    focus_handle: FocusHandle,
    player: Option<SoundfontPlayer>,
    loaded_path: Option<PathBuf>,
    panel: SoundfontPlayerPanelState,
}

impl SoundfontPlayerWindow {
    pub fn new(
        track_id: String,
        on_close: Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>,
        on_update_track: Arc<dyn Fn(SoundfontPlayerTrackUpdate, &mut App) + Send + Sync>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            track_id,
            on_close,
            on_update_track,
            focus_handle: cx.focus_handle(),
            player: None,
            loaded_path: None,
            panel: SoundfontPlayerPanelState::default(),
        }
    }

    /// Kept for the existing Inspector open path. The window now contains one
    /// direct panel, so focusing the OS window is handled by the caller.
    pub fn focus_soundfont_player(&mut self, track_id: String) {
        self.track_id = track_id;
    }

    fn notify_track_update(&self, app: &mut App) {
        (self.on_update_track)(
            SoundfontPlayerTrackUpdate {
                track_id: self.track_id.clone(),
                path: self
                    .loaded_path
                    .as_ref()
                    .map(|path| path.to_string_lossy().into_owned()),
                preset: self.panel.selected_preset,
                volume: self.panel.master_volume,
                reverb_chorus: self.panel.reverb_chorus,
                polyphony: self.panel.polyphony,
            },
            app,
        );
    }

    fn browse_soundfont(&mut self, cx: &mut Context<Self>) {
        self.panel.loading = true;
        self.panel.status = Some("Loading SoundFont…".into());
        cx.notify();
        #[cfg(feature = "native-dialogs")]
        {
            let entity = cx.entity().clone();
            cx.spawn(async move |_this, cx| {
                let result = rfd::AsyncFileDialog::new()
                    .set_title("Load SoundFont")
                    .add_filter("SoundFont", &["sf2"])
                    .pick_file()
                    .await;
                let Some(handle) = result else {
                    let _ = entity.update(cx, |this, cx| {
                        this.panel.loading = false;
                        this.panel.status = None;
                        cx.notify();
                    });
                    return;
                };
                let path = handle.path().to_path_buf();
                let settings = default_soundfont_player_settings(44_100);
                let load_result = SoundfontPlayer::from_path(&path, settings);
                let _ = entity.update(cx, |this, cx| {
                    this.apply_loaded_player(path, load_result);
                    this.notify_track_update(cx);
                    cx.notify();
                });
            })
            .detach();
        }
        #[cfg(not(feature = "native-dialogs"))]
        {
            self.panel.loading = false;
            self.panel.status = Some("Native file dialogs are unavailable in this build.".into());
            cx.notify();
        }
    }

    fn apply_loaded_player(
        &mut self,
        path: PathBuf,
        result: Result<SoundfontPlayer, SoundfontPlayerError>,
    ) {
        match result {
            Ok(mut player) => {
                self.panel.loading = false;
                self.panel.bank_name = Some(player.bank_name().to_string());
                self.panel.presets = player.list_presets();
                self.panel.file_name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned());
                self.panel.selected_preset = None;
                self.panel.status = None;
                if let Some(first) = self.panel.presets.first() {
                    match player.select_preset(PREVIEW_MIDI_CHANNEL, first.bank, first.patch) {
                        Ok(()) => self.panel.selected_preset = Some((first.bank, first.patch)),
                        Err(error) => {
                            self.panel.status =
                                Some(format!("Default preset select failed: {error}"));
                        }
                    }
                }
                self.panel.master_volume = player.master_volume();
                self.panel.reverb_chorus = player.enable_reverb_and_chorus();
                self.panel.polyphony = player.maximum_polyphony();
                self.player = Some(player);
                self.loaded_path = Some(path);
            }
            Err(error) => {
                self.panel.loading = false;
                self.panel.status = Some(format!("Load failed: {error}"));
            }
        }
    }

    fn toggle_preset_list(&mut self) {
        self.panel.preset_list_open = !self.panel.preset_list_open;
    }

    fn select_preset(&mut self, bank: i32, patch: i32) {
        let Some(player) = self.player.as_mut() else {
            return;
        };
        match player.select_preset(PREVIEW_MIDI_CHANNEL, bank, patch) {
            Ok(()) => {
                self.panel.selected_preset = Some((bank, patch));
                self.panel.preset_list_open = false;
                self.panel.status = None;
            }
            Err(error) => {
                self.panel.status = Some(format!("Preset select failed: {error}"));
            }
        }
    }

    fn set_volume(&mut self, value: f32) {
        let value = value.clamp(0.0, 1.0);
        self.panel.master_volume = value;
        if let Some(player) = self.player.as_mut() {
            player.set_master_volume(value);
        }
    }

    /// Reverb/chorus and polyphony are `SynthesizerSettings` fixed at
    /// creation in RustySynth — no live setter exists, so applying either
    /// reloads the same file (a control/offline operation, same as the
    /// initial load) and reapplies volume + the selected preset.
    fn reload_with_settings(&mut self) {
        let Some(path) = self.loaded_path.clone() else {
            return;
        };
        let sample_rate = self
            .player
            .as_ref()
            .map(SoundfontPlayer::sample_rate)
            .unwrap_or(44_100);
        let settings = SoundfontPlayerSettings {
            sample_rate,
            block_size: 0,
            maximum_polyphony: self.panel.polyphony,
            enable_reverb_and_chorus: self.panel.reverb_chorus,
        };
        match SoundfontPlayer::from_path(&path, settings) {
            Ok(mut player) => {
                player.set_master_volume(self.panel.master_volume);
                if let Some((bank, patch)) = self.panel.selected_preset {
                    if let Err(error) = player.select_preset(PREVIEW_MIDI_CHANNEL, bank, patch) {
                        self.panel.status = Some(format!("Preset reselect failed: {error}"));
                    }
                }
                self.player = Some(player);
            }
            Err(error) => {
                self.panel.status = Some(format!("Reload failed: {error}"));
            }
        }
    }

    fn toggle_reverb_chorus(&mut self) {
        self.panel.reverb_chorus = !self.panel.reverb_chorus;
        self.reload_with_settings();
    }

    fn set_polyphony(&mut self, value: usize) {
        self.panel.polyphony = value;
        self.reload_with_settings();
    }
}

impl Render for SoundfontPlayerWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focus_handle.is_focused(window) {
            self.focus_handle.focus(window, cx);
        }
        let on_close = self.on_close.clone();
        let entity = cx.entity().clone();

        let panel_callbacks = SoundfontPlayerCallbacks {
            on_browse: Arc::new({
                let entity = entity.clone();
                move |_window, app: &mut App| {
                    let _ = entity.update(app, |this, cx| {
                        this.browse_soundfont(cx);
                    });
                }
            }),
            on_toggle_preset_list: Arc::new({
                let entity = entity.clone();
                move |_window, app: &mut App| {
                    let _ = entity.update(app, |this, cx| {
                        this.toggle_preset_list();
                        cx.notify();
                    });
                }
            }),
            on_select_preset: Arc::new({
                let entity = entity.clone();
                move |(bank, patch): &(i32, i32), _window, app: &mut App| {
                    let (bank, patch) = (*bank, *patch);
                    let _ = entity.update(app, |this, cx| {
                        this.select_preset(bank, patch);
                        this.notify_track_update(cx);
                        cx.notify();
                    });
                }
            }),
            on_set_volume: Arc::new({
                let entity = entity.clone();
                move |value: &f32, _window, app: &mut App| {
                    let value = *value;
                    let _ = entity.update(app, |this, cx| {
                        this.set_volume(value);
                        this.notify_track_update(cx);
                        cx.notify();
                    });
                }
            }),
            on_toggle_reverb_chorus: Arc::new({
                let entity = entity.clone();
                move |_window, app: &mut App| {
                    let _ = entity.update(app, |this, cx| {
                        this.toggle_reverb_chorus();
                        this.notify_track_update(cx);
                        cx.notify();
                    });
                }
            }),
            on_set_polyphony: Arc::new(move |value: &usize, _window, app: &mut App| {
                let value = *value;
                let _ = entity.update(app, |this, cx| {
                    this.set_polyphony(value);
                    this.notify_track_update(cx);
                    cx.notify();
                });
            }),
        };
        let panel = self.panel.clone();

        div()
            .flex()
            .flex_col()
            .size_full()
            .font(crate::theme::ui_font())
            .bg(Colors::surface_window())
            .overflow_hidden()
            .child(div().w(px(0.0)).h(px(0.0)).track_focus(&self.focus_handle))
            .child(external_window_titlebar(
                SOUNDFONT_PLAYER_MDI_TITLE,
                "soundfont-player-window-close",
                move |window, cx| {
                    on_close(window, cx);
                    window.remove_window();
                },
            ))
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .relative()
                    .child(soundfont_player_panel(&panel, panel_callbacks)),
            )
    }
}

pub fn open_soundfont_player_window(
    owner_bounds: Option<Bounds<gpui::Pixels>>,
    track_id: String,
    on_close: Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>,
    on_update_track: Arc<dyn Fn(SoundfontPlayerTrackUpdate, &mut App) + Send + Sync>,
    cx: &mut App,
) -> Result<WindowHandle<SoundfontPlayerWindow>, String> {
    let window_bounds = crate::window_position::centered_window_bounds(
        owner_bounds,
        size(
            px(SOUNDFONT_PLAYER_WINDOW_WIDTH),
            px(SOUNDFONT_PLAYER_WINDOW_HEIGHT),
        ),
        cx,
    );
    let mut options = crate::platform_chrome::external_dialog_window_options_partial();
    options.window_bounds = Some(WindowBounds::Windowed(window_bounds));
    options.kind = WindowKind::Floating;
    options.is_resizable = true;
    options.is_minimizable = true;
    options.window_background = WindowBackgroundAppearance::Opaque;
    options.window_min_size = Some(size(
        px(SOUNDFONT_PLAYER_WINDOW_MIN_WIDTH),
        px(SOUNDFONT_PLAYER_WINDOW_MIN_HEIGHT),
    ));
    crate::window_position::apply_owner_display(&mut options, owner_bounds, cx);

    cx.open_window(options, move |_window, cx| {
        cx.new(|cx| SoundfontPlayerWindow::new(track_id, on_close, on_update_track, cx))
    })
    .map_err(|error| error.to_string())
}
