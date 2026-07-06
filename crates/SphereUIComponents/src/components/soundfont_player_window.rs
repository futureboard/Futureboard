//! Floating "Soundfont Player" utility window.
//!
//! Hosts the [`crate::components::mdi`] workspace so the built-in Soundfont
//! Player instrument (see `TrackState::builtin_soundfont_player`) has a real
//! window to open from the Inspector. The MDI workspace state lives here,
//! not in `StudioLayout` — this window is the only place that reads or
//! mutates it, so there is no cross-entity update to worry about.
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

use crate::components::mdi::{MdiWorkspaceCallbacks, MdiWorkspaceState};
use crate::components::soundfont_player_mdi::{
    ensure_soundfont_player_document, soundfont_player_mdi_workspace, SoundfontPlayerCallbacks,
    SoundfontPlayerPanelState, SOUNDFONT_PLAYER_MDI_TITLE,
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

pub struct SoundfontPlayerWindow {
    workspace: MdiWorkspaceState,
    on_close: Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>,
    focus_handle: FocusHandle,
    player: Option<SoundfontPlayer>,
    loaded_path: Option<PathBuf>,
    panel: SoundfontPlayerPanelState,
}

impl SoundfontPlayerWindow {
    pub fn new(
        on_close: Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut workspace = MdiWorkspaceState::default();
        ensure_soundfont_player_document(&mut workspace);
        Self {
            workspace,
            on_close,
            focus_handle: cx.focus_handle(),
            player: None,
            loaded_path: None,
            panel: SoundfontPlayerPanelState::default(),
        }
    }

    /// Focus (or re-open, if somehow closed) the Soundfont Player document.
    /// Called whenever the Inspector's Open button is clicked while this
    /// window is already up, so repeated clicks always bring it to front.
    pub fn focus_soundfont_player(&mut self) {
        ensure_soundfont_player_document(&mut self.workspace);
    }

    fn browse_soundfont(&mut self, cx: &mut Context<Self>) {
        #[cfg(feature = "native-dialogs")]
        {
            let entity = cx.entity().clone();
            cx.spawn(async move |_this, cx| {
                let result = rfd::AsyncFileDialog::new()
                    .set_title("Load SoundFont")
                    .add_filter("SoundFont", &["sf2"])
                    .pick_file()
                    .await;
                let Some(handle) = result else { return };
                let path = handle.path().to_path_buf();
                let settings = default_soundfont_player_settings(44_100);
                let load_result = SoundfontPlayer::from_path(&path, settings);
                let _ = entity.update(cx, |this, cx| {
                    this.apply_loaded_player(path, load_result);
                    cx.notify();
                });
            })
            .detach();
        }
        #[cfg(not(feature = "native-dialogs"))]
        {
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
            Ok(player) => {
                self.panel.bank_name = Some(player.bank_name().to_string());
                self.panel.presets = player.list_presets();
                self.panel.file_name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned());
                self.panel.selected_preset = None;
                self.panel.master_volume = player.master_volume();
                self.panel.reverb_chorus = player.enable_reverb_and_chorus();
                self.panel.polyphony = player.maximum_polyphony();
                self.panel.status = None;
                self.player = Some(player);
                self.loaded_path = Some(path);
            }
            Err(error) => {
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

        let callbacks = {
            let entity = entity.clone();
            MdiWorkspaceCallbacks {
                on_focus: Arc::new({
                    let entity = entity.clone();
                    move |id: &String, _window, app: &mut App| {
                        let id = id.clone();
                        let _ = entity.update(app, |this, cx| {
                            this.workspace.focus_document(&id);
                            cx.notify();
                        });
                    }
                }),
                on_close: Arc::new({
                    let entity = entity.clone();
                    move |id: &String, _window, app: &mut App| {
                        let id = id.clone();
                        let _ = entity.update(app, |this, cx| {
                            this.workspace.close_document(&id);
                            cx.notify();
                        });
                    }
                }),
                on_minimize: Arc::new({
                    let entity = entity.clone();
                    move |id: &String, _window, app: &mut App| {
                        let id = id.clone();
                        let _ = entity.update(app, |this, cx| {
                            this.workspace.minimize_document(&id);
                            cx.notify();
                        });
                    }
                }),
                on_restore: Arc::new(move |id: &String, _window, app: &mut App| {
                    let id = id.clone();
                    let _ = entity.update(app, |this, cx| {
                        this.workspace.restore_document(&id);
                        cx.notify();
                    });
                }),
            }
        };

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
                        cx.notify();
                    });
                }
            }),
            on_toggle_reverb_chorus: Arc::new({
                let entity = entity.clone();
                move |_window, app: &mut App| {
                    let _ = entity.update(app, |this, cx| {
                        this.toggle_reverb_chorus();
                        cx.notify();
                    });
                }
            }),
            on_set_polyphony: Arc::new(move |value: &usize, _window, app: &mut App| {
                let value = *value;
                let _ = entity.update(app, |this, cx| {
                    this.set_polyphony(value);
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
                div().flex_1().min_h(px(0.0)).relative().child(
                    soundfont_player_mdi_workspace(&self.workspace, callbacks, &panel, panel_callbacks),
                ),
            )
    }
}

pub fn open_soundfont_player_window(
    owner_bounds: Option<Bounds<gpui::Pixels>>,
    on_close: Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>,
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
        cx.new(|cx| SoundfontPlayerWindow::new(on_close, cx))
    })
    .map_err(|error| error.to_string())
}
