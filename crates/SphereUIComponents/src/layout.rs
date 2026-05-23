use gpui::{div, AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Window};

use std::path::PathBuf;

use crate::components;
use crate::components::file_browser::FileBrowserState;
use crate::components::mixer_panel::MixerCallbacks;
use crate::components::timeline::timeline_state::TrackState;
use crate::components::timeline::waveform_cache;
use crate::components::{BottomPanelResizeDrag, BottomPanelState};
use crate::theme::{self, Colors};

/// Flip to `true` to seed the studio with demo tracks/clips at startup.
/// Production builds must keep this `false` — the real app starts empty.
const USE_DEMO_PROJECT: bool = false;

/// Top-menu open state. `open_menu_id` is the manifest menu id currently
/// showing its dropdown; `anchor_x` is the click x position used to align
/// the dropdown panel underneath the clicked label.
#[derive(Debug, Clone, Default)]
pub struct MenuBarUiState {
    pub open_menu_id: Option<String>,
    pub anchor_x: f32,
    /// Nested submenu ids open underneath the root dropdown. `path[0]` is
    /// the submenu open in the root panel, `path[1]` in *that* submenu's
    /// panel, etc.
    pub submenu_path: Vec<String>,
}

pub struct StudioLayout {
    active_bottom_tab: components::BottomTab,
    bottom_panel_state: BottomPanelState,
    timeline: Entity<components::timeline::Timeline>,
    file_browser: FileBrowserState,
    menu_bar: MenuBarUiState,
}

impl StudioLayout {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let timeline = cx.new(|_| {
            if USE_DEMO_PROJECT {
                components::timeline::Timeline::with_demo_content()
            } else {
                components::timeline::Timeline::new()
            }
        });
        Self {
            active_bottom_tab: components::BottomTab::Mixer,
            bottom_panel_state: BottomPanelState::default(),
            timeline,
            file_browser: FileBrowserState::default(),
            menu_bar: MenuBarUiState::default(),
        }
    }
}

impl StudioLayout {
    /// Build the callback bundle used by the mixer. Every mutation lands in
    /// the same `TimelineState` instance owned by the Timeline entity, so the
    /// TrackHeader and Mixer always read identical values.
    fn build_mixer_callbacks(&self) -> MixerCallbacks {
        let timeline_select = self.timeline.clone();
        let on_select_track: std::sync::Arc<dyn Fn(&String, &mut Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(move |id: &String, _w, cx| {
                let id = id.clone();
                timeline_select.update(cx, |t, cx| {
                    t.state.select_track(&id);
                    cx.notify();
                });
            });

        let timeline_vol = self.timeline.clone();
        let on_volume_change: std::sync::Arc<dyn Fn(&(String, f32), &mut Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(move |(id, v): &(String, f32), _w, cx| {
                let id = id.clone();
                let v = *v;
                timeline_vol.update(cx, |t, cx| {
                    t.state.set_track_volume(&id, v);
                    cx.notify();
                });
            });

        let timeline_pan = self.timeline.clone();
        let on_pan_change: std::sync::Arc<dyn Fn(&(String, f32), &mut Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(move |(id, v): &(String, f32), _w, cx| {
                let id = id.clone();
                let v = *v;
                timeline_pan.update(cx, |t, cx| {
                    t.state.set_track_pan(&id, v);
                    cx.notify();
                });
            });

        let timeline_mute = self.timeline.clone();
        let on_toggle_mute: std::sync::Arc<dyn Fn(&String, &mut Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(move |id: &String, _w, cx| {
                let id = id.clone();
                timeline_mute.update(cx, |t, cx| {
                    t.state.toggle_track_mute(&id);
                    cx.notify();
                });
            });

        let timeline_solo = self.timeline.clone();
        let on_toggle_solo: std::sync::Arc<dyn Fn(&String, &mut Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(move |id: &String, _w, cx| {
                let id = id.clone();
                timeline_solo.update(cx, |t, cx| {
                    t.state.toggle_track_solo(&id);
                    cx.notify();
                });
            });

        let timeline_arm = self.timeline.clone();
        let on_toggle_arm: std::sync::Arc<dyn Fn(&String, &mut Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(move |id: &String, _w, cx| {
                let id = id.clone();
                timeline_arm.update(cx, |t, cx| {
                    t.state.toggle_track_arm(&id);
                    cx.notify();
                });
            });

        let timeline_input = self.timeline.clone();
        let on_toggle_input: std::sync::Arc<dyn Fn(&String, &mut Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(move |id: &String, _w, cx| {
                let id = id.clone();
                timeline_input.update(cx, |t, cx| {
                    t.state.toggle_track_input_monitor(&id);
                    cx.notify();
                });
            });

        let timeline_master = self.timeline.clone();
        let on_master_volume_change: std::sync::Arc<dyn Fn(&f32, &mut Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(move |v: &f32, _w, cx| {
                let v = *v;
                timeline_master.update(cx, |t, cx| {
                    t.state.set_master_volume(v);
                    cx.notify();
                });
            });

        MixerCallbacks {
            on_select_track,
            on_volume_change,
            on_pan_change,
            on_toggle_mute,
            on_toggle_solo,
            on_toggle_arm,
            on_toggle_input,
            on_master_volume_change,
        }
    }
}

impl Render for StudioLayout {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let on_tab_click = cx.listener(|this, tab: &components::BottomTab, _window, cx| {
            this.active_bottom_tab = *tab;
            cx.notify();
        });

        let on_resize_start = cx.listener(
            |this, event: &gpui::MouseDownEvent, window, cx| {
                let bs = &mut this.bottom_panel_state;
                bs.is_resizing = true;
                bs.resize_start_y = f32::from(event.position.y);
                bs.resize_start_height = bs.height_px;
                let window_h: f32 = window.bounds().size.height.into();
                bs.max_height_px = (window_h * 0.70).max(bs.min_height_px + 40.0);
                cx.notify();
            },
        );

        let on_resize_move = cx.listener(
            |this, event: &gpui::DragMoveEvent<BottomPanelResizeDrag>, _window, cx| {
                let bs = &mut this.bottom_panel_state;
                let cur_y: f32 = event.event.position.y.into();
                let delta = bs.resize_start_y - cur_y;
                let new_h = (bs.resize_start_height + delta).clamp(bs.min_height_px, bs.max_height_px);
                if (new_h - bs.height_px).abs() > 0.5 {
                    bs.height_px = new_h;
                    cx.notify();
                }
            },
        );

        // Pull the live track list and current selection out of the Timeline so
        // the Mixer and Inspector render against the same data the TrackHeader
        // sees. Cloning the Vec is cheap relative to a full render.
        let (tracks, master, selected_track_id, selected_clip_id) = {
            let t = self.timeline.read(cx);
            (
                t.state.tracks.clone(),
                t.state.master.clone(),
                t.state.selection.selected_track_id.clone(),
                t.state.selection.selected_clip_ids.first().cloned(),
            )
        };

        let panel_state = self.bottom_panel_state;
        let mixer_callbacks = self.build_mixer_callbacks();

        // ── File browser callbacks ──────────────────────────────────────
        let on_browser_navigate: std::sync::Arc<dyn Fn(&PathBuf, &mut Window, &mut gpui::App) + 'static> = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |path: &PathBuf, _w, cx| {
                let path = path.clone();
                this.update(cx, |this, cx| {
                    this.file_browser.navigate_to(path);
                    cx.notify();
                });
            })
        };
        let on_browser_select: std::sync::Arc<dyn Fn(&PathBuf, &mut Window, &mut gpui::App) + 'static> = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |path: &PathBuf, _w, cx| {
                let path = path.clone();
                this.update(cx, |this, cx| {
                    this.file_browser.select(path);
                    cx.notify();
                });
            })
        };
        let on_browser_up: std::sync::Arc<dyn Fn(&(), &mut Window, &mut gpui::App) + 'static> = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |_: &(), _w, cx| {
                this.update(cx, |this, cx| {
                    this.file_browser.navigate_up();
                    cx.notify();
                });
            })
        };
        // Double-click on an audio file imports it onto the timeline using the
        // existing waveform-cache + import_audio_at path.
        let on_browser_activate: std::sync::Arc<dyn Fn(&PathBuf, &mut Window, &mut gpui::App) + 'static> = {
            let timeline = self.timeline.clone();
            std::sync::Arc::new(move |path: &PathBuf, _w, cx| {
                let path = path.clone();
                timeline.update(cx, |t, cx| {
                    let decoded = waveform_cache::decode_and_cache_file(&path);
                    let duration = decoded
                        .as_ref()
                        .map(|p| p.duration_seconds)
                        .unwrap_or(0.0);
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "Imported Audio".to_string());
                    // Drop at scroll origin on whatever lane currently sits at
                    // y=0; if none, `import_audio_at` makes a new track.
                    t.state.import_audio_at(
                        path.to_string_lossy().to_string(),
                        name,
                        0.0,
                        1.0e9_f32,
                        duration,
                    );
                    cx.notify();
                });
            })
        };

        let file_browser = self.file_browser.clone();

        // ── Top-menu callbacks ─────────────────────────────────────────────
        let on_open_menu: std::sync::Arc<dyn Fn(&(String, f32), &mut Window, &mut gpui::App) + 'static> = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |(id, anchor_x): &(String, f32), _w, cx| {
                let id = id.clone();
                let anchor_x = *anchor_x;
                this.update(cx, |this, cx| {
                    if this.menu_bar.open_menu_id.as_deref() == Some(id.as_str()) {
                        this.menu_bar.open_menu_id = None;
                    } else {
                        this.menu_bar.open_menu_id = Some(id);
                        this.menu_bar.anchor_x = anchor_x;
                    }
                    this.menu_bar.submenu_path.clear();
                    cx.notify();
                });
            })
        };
        let on_close_menu: std::sync::Arc<dyn Fn(&(), &mut Window, &mut gpui::App) + 'static> = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |_: &(), _w, cx| {
                this.update(cx, |this, cx| {
                    this.menu_bar.open_menu_id = None;
                    this.menu_bar.submenu_path.clear();
                    cx.notify();
                });
            })
        };
        let on_toggle_submenu: std::sync::Arc<dyn Fn(&(usize, String), &mut Window, &mut gpui::App) + 'static> = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |(depth, id): &(usize, String), _w, cx| {
                let depth = *depth;
                let id = id.clone();
                this.update(cx, |this, cx| {
                    // Truncate the path to this depth, then toggle: if the
                    // requested id is already open at this depth, close it;
                    // otherwise open it (closing anything deeper).
                    let already_open = this.menu_bar.submenu_path.get(depth) == Some(&id);
                    this.menu_bar.submenu_path.truncate(depth);
                    if !already_open {
                        this.menu_bar.submenu_path.push(id);
                    }
                    cx.notify();
                });
            })
        };
        let on_menu_command: std::sync::Arc<dyn Fn(&String, &mut Window, &mut gpui::App) + 'static> = {
            std::sync::Arc::new(move |command: &String, _w, _cx| {
                eprintln!("[menu] command: {}", command);
            })
        };

        let open_menu_id = self.menu_bar.open_menu_id.clone();
        let menu_anchor_x = self.menu_bar.anchor_x;
        let submenu_path = self.menu_bar.submenu_path.clone();
        let viewport_width: f32 = window.bounds().size.width.into();

        let dropdown_overlay = open_menu_id.as_ref().and_then(|id| {
            let manifest = crate::menu::MenuManifest::load();
            manifest.menus.iter().find(|m| &m.id == id).map(|menu| {
                components::menu_dropdown::menu_dropdown(
                    menu,
                    menu_anchor_x,
                    viewport_width,
                    &submenu_path,
                    on_toggle_submenu.clone(),
                    on_menu_command.clone(),
                    on_close_menu.clone(),
                )
            })
        });

        div()
            .flex()
            .flex_col()
            .size_full()
            .relative()
            .bg(Colors::surface_base())
            .font_family(theme::FONT_FAMILY)
            .child(components::app_chrome(window, open_menu_id.as_deref(), on_open_menu))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .child(components::sidebar(
                        &file_browser,
                        on_browser_navigate,
                        on_browser_select,
                        on_browser_activate,
                        on_browser_up,
                    ))
                    .child(self.timeline.clone())
                    .child(crate::components::panel::inspector_panel(
                        &tracks,
                        selected_track_id.as_deref(),
                        selected_clip_id.as_deref(),
                        find_clip_summary(&tracks, selected_clip_id.as_deref()),
                    )),
            )
            .child(components::bottom_panel(
                self.active_bottom_tab,
                panel_state,
                &tracks,
                &master,
                selected_track_id.as_deref(),
                mixer_callbacks,
                on_tab_click,
                on_resize_start,
                on_resize_move,
            ))
            .child(components::status_bar())
            // Dropdown overlay — rendered last so it sits above every other
            // panel. The dropdown's own backdrop captures click-outside.
            .children(dropdown_overlay)
    }
}

fn find_clip_summary<'a>(
    tracks: &'a [TrackState],
    clip_id: Option<&str>,
) -> Option<crate::components::panel::SelectedClipSummary<'a>> {
    let id = clip_id?;
    for t in tracks {
        if let Some(c) = t.clips.iter().find(|c| c.id == id) {
            return Some(crate::components::panel::SelectedClipSummary {
                name: &c.name,
                start_beat: c.start_beat,
                duration_beats: c.duration_beats,
                kind: match &c.clip_type {
                    crate::components::timeline::timeline_state::ClipType::Audio { .. } => "Audio",
                    crate::components::timeline::timeline_state::ClipType::Midi { .. } => "MIDI",
                },
                track_name: &t.name,
            });
        }
    }
    None
}
