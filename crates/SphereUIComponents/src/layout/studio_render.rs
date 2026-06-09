use super::*;

impl Render for StudioLayout {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _root_scope = crate::perf::PerfScope::enter("StudioLayout");
        // Frame pacing tick. See FrameDiagnostics docs — only counts
        // real repaints, not display refreshes.
        let reason = self.frame_reason();
        let reason_static: &'static str = match reason {
            "transport" => "transport",
            "panel-resize" => "panel-resize",
            "menu" => "menu",
            _ => "idle/interaction",
        };
        self.frame_diag.tick(reason);
        crate::perf::tick_root_frame(reason_static);
        self.cached_studio_window_bounds = Some(window.bounds());

        // Keep the OS window title in sync with the project lifecycle state
        // (Part G/H), e.g. "Untitled Project — Unsaved" / "My Song — Saved".
        let title = self.window_title();
        if self.last_window_title.as_deref() != Some(title.as_str()) {
            window.set_window_title(&title);
            self.last_window_title = Some(title);
        }

        let on_tab_click = cx.listener(|this, tab: &components::BottomTab, _window, cx| {
            this.active_bottom_tab = *tab;
            cx.notify();
        });

        // Mixer scroll — updated by the mixer scroll-wheel handler.
        let mixer_scroll_x = self.mixer_scroll_x;
        // Approximate the scrollable channel area width: full window minus the
        // master strip (STRIP_WIDTH) plus gutter (1px) and a small margin.
        let window_w: f32 = window.bounds().size.width.into();
        let mixer_viewport_width = (window_w - 90.0).max(100.0);
        let on_mixer_scroll: std::sync::Arc<
            dyn Fn(f32, &mut gpui::Window, &mut gpui::App) + 'static,
        > = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |new_x: f32, _w, cx| {
                let _ = this.update(cx, |this, cx| {
                    if this.set_mixer_scroll_x(new_x, cx) {
                        this.push_mixer_snapshot_to_window(cx);
                        cx.notify();
                    }
                });
            })
        };

        let on_resize_start = cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
            let bs = &mut this.bottom_panel_state;
            bs.is_resizing = true;
            bs.resize_start_y = f32::from(event.position.y);
            bs.resize_start_height = bs.height_px;
            let window_h: f32 = window.bounds().size.height.into();
            bs.max_height_px = (window_h * 0.70).max(bs.min_height_px + 40.0);
            cx.notify();
        });

        let on_resize_move = cx.listener(
            |this, event: &gpui::DragMoveEvent<BottomPanelResizeDrag>, _window, cx| {
                let bs = &mut this.bottom_panel_state;
                let cur_y: f32 = event.event.position.y.into();
                let delta = bs.resize_start_y - cur_y;
                let new_h =
                    (bs.resize_start_height + delta).clamp(bs.min_height_px, bs.max_height_px);
                if (new_h - bs.height_px).abs() > 0.5 {
                    bs.height_px = new_h;
                    cx.notify();
                }
            },
        );
        let on_resize_end = cx.listener(|this, _event: &gpui::MouseUpEvent, _window, cx| {
            if this.bottom_panel_state.is_resizing {
                this.bottom_panel_state.is_resizing = false;
                cx.notify();
            }
        });

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
        let mixer_callbacks = self.build_mixer_callbacks(cx.entity().clone());
        let inspector_callbacks = self.build_inspector_callbacks(cx.entity().clone());

        // Enumerate the selected input device's channels only while the audio-input
        // combo is open (avoids per-frame device enumeration).
        let audio_input_device = if self.open_inspector_routing_combo
            == Some(crate::components::panel::InspectorRoutingCombo::AudioInput)
        {
            self.selected_input_device_channels(cx)
        } else {
            None
        };
        let audio_output_buses: Vec<(String, String)> = if self.open_inspector_routing_combo
            == Some(crate::components::panel::InspectorRoutingCombo::AudioOutput)
        {
            tracks
                .iter()
                .filter(|track| track.track_type.is_routing())
                .map(|track| (track.id.clone(), track.name.clone()))
                .collect()
        } else {
            Vec::new()
        };
        let audio_output_device = if self.open_inspector_routing_combo
            == Some(crate::components::panel::InspectorRoutingCombo::AudioOutput)
        {
            self.selected_output_device_channels(cx)
        } else {
            None
        };
        let inspector_routing_combo_overlay: Option<gpui::AnyElement> =
            if let (Some(combo), Some(anchor)) = (
                self.open_inspector_routing_combo,
                self.inspector_routing_combo_anchor,
            ) {
                selected_track_id.as_deref().and_then(|tid| {
                    tracks.iter().find(|t| t.id == tid).map(|track| {
                        let close = Arc::new({
                            let this = cx.entity().clone();
                            move |cx: &mut gpui::App| {
                                let _ = this.update(cx, |layout, cx| {
                                    layout.open_inspector_routing_combo = None;
                                    layout.inspector_routing_combo_anchor = None;
                                    cx.notify();
                                });
                            }
                        });
                        crate::components::panel::inspector_routing_combo_overlay(
                            track,
                            combo,
                            anchor,
                            window,
                            &inspector_callbacks,
                            close,
                            audio_input_device.clone(),
                            audio_output_buses.clone(),
                            audio_output_device.clone(),
                        )
                        .into_any_element()
                    })
                })
            } else {
                None
            };

        // Reconcile the Inspector name field with the current track selection.
        // Only reload when the bound track actually changes, so typing into the
        // field for the *selected* track is never clobbered mid-edit.
        if self.inspector_name_bound.as_deref() != selected_track_id.as_deref() {
            match selected_track_id
                .as_deref()
                .and_then(|tid| tracks.iter().find(|t| t.id == tid))
            {
                Some(t) => {
                    self.inspector_name_input.set_value(t.name.clone());
                    self.inspector_name_bound = Some(t.id.clone());
                }
                None => {
                    self.inspector_name_input.set_value("");
                    self.inspector_name_bound = None;
                }
            }
        }
        let inspector_name_focused = self.inspector_name_input.is_focused(window);
        if self.inspector_clip_name_bound.as_deref() != selected_clip_id.as_deref() {
            match selected_clip_id.as_deref().and_then(|cid| {
                tracks
                    .iter()
                    .find_map(|t| t.clips.iter().find(|c| c.id == cid))
            }) {
                Some(c) => {
                    self.inspector_clip_name_input.set_value(c.name.clone());
                    self.inspector_clip_name_bound = Some(c.id.clone());
                }
                None => {
                    self.inspector_clip_name_input.set_value("");
                    self.inspector_clip_name_bound = None;
                }
            }
        }
        let inspector_clip_name_focused = self.inspector_clip_name_input.is_focused(window);

        crate::perf::count("tracks", tracks.len() as u64);

        // ── File browser callbacks ──────────────────────────────────────
        let on_browser_search_context: std::sync::Arc<
            dyn Fn(&(f32, f32), &mut Window, &mut gpui::App) + 'static,
        > = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |(x, y): &(f32, f32), _w, cx| {
                let x = *x;
                let y = *y;
                let _ = this.update(cx, |this, cx| {
                    this.menu_bar.open_menu_id = None;
                    this.menu_bar.submenu_path.clear();
                    this.project_switcher.is_open = false;
                    this.text_context_menu = Some(TextContextMenu {
                        target: TextMenuTarget::BrowserSearch,
                        x,
                        y,
                    });
                    cx.notify();
                });
            })
        };

        let on_browser_toggle: std::sync::Arc<
            dyn Fn(&(String, Option<PathBuf>), &mut Window, &mut gpui::App) + 'static,
        > = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |(id, path): &(String, Option<PathBuf>), _w, cx| {
                let id = id.clone();
                let path = path.clone();
                let _ = this.update(cx, |this, cx| {
                    let expanded = this.file_browser.toggle_node(&id, path.as_deref());
                    if expanded {
                        // Drain any newly-expanded paths whose contents
                        // haven't been indexed yet and kick off a
                        // background load for each.
                        let pending = this.file_browser.paths_needing_load();
                        for p in pending {
                            this.file_browser.mark_loading(p.clone());
                            Self::spawn_directory_load(cx, p);
                        }
                    }
                    cx.notify();
                });
            })
        };
        let on_browser_select: std::sync::Arc<
            dyn Fn(&PathBuf, &mut Window, &mut gpui::App) + 'static,
        > = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |path: &PathBuf, _w, cx| {
                let path = path.clone();
                this.update(cx, |this, cx| {
                    this.file_browser.select(path);
                    cx.notify();
                });
            })
        };
        // Double-click on an audio file imports it onto the timeline using the
        // existing waveform-cache + import_audio_at path.
        let on_browser_activate: std::sync::Arc<
            dyn Fn(&PathBuf, &mut Window, &mut gpui::App) + 'static,
        > = {
            let timeline = self.timeline.clone();
            let layout = cx.entity().clone();
            std::sync::Arc::new(move |path: &PathBuf, _w, cx| {
                // Filter on extension before mutating timeline state so
                // double-clicking a non-audio file (e.g. .txt, .png) does
                // not create a phantom clip with the 8-bar fallback
                // duration that never resolves to real metadata.
                let ext = path
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_ascii_lowercase())
                    .unwrap_or_default();
                if !is_supported_audio_ext(&ext) {
                    eprintln!(
                        "[import] ignoring non-audio activation: ext='{}' path={}",
                        ext,
                        path.display()
                    );
                    return;
                }

                let path = path.clone();
                let path_for_decode = path.clone();
                let timeline_for_decode = timeline.clone();
                timeline.update(cx, |t, cx| {
                    let path_key = path.to_string_lossy().to_string();
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "Imported Audio".to_string());
                    t.state
                        .import_audio_to_selected_or_new_track(path_key, name);
                    cx.notify();
                });
                let _ = layout.update(cx, |this, cx| {
                    this.mark_dirty();
                    this.mark_engine_media_dirty();
                    this.schedule_audio_project_sync(cx, false, "timeline_audio_import");
                });
                let path_key = path_for_decode.to_string_lossy().to_string();
                let owner = layout.clone();
                let _ = layout.update(cx, move |_layout, cx| {
                    Self::spawn_timeline_audio_import_jobs(
                        cx,
                        owner,
                        timeline_for_decode,
                        path_for_decode,
                        path_key,
                    );
                });
            })
        };
        let on_browser_context: std::sync::Arc<
            dyn Fn(&(Option<PathBuf>, f32, f32), &mut Window, &mut gpui::App) + 'static,
        > = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |(path, x, y): &(Option<PathBuf>, f32, f32), _w, cx| {
                let path = path.clone();
                let x = *x;
                let y = *y;
                let _ = this.update(cx, |this, cx| {
                    this.menu_bar.open_menu_id = None;
                    this.menu_bar.submenu_path.clear();
                    this.project_switcher.is_open = false;
                    this.open_popover = Some(OpenPopover::Context {
                        target: ContextTarget::Browser(path),
                        x,
                        y,
                    });
                    cx.notify();
                });
            })
        };

        let file_browser = self.file_browser.clone();
        let browser_scroll = self.browser_scroll.clone();

        let on_timeline_context: components::timeline::timeline::TimelineContextMenuCb = {
            let this = cx.entity().clone();
            std::sync::Arc::new(
                move |(target, x, y): &(TimelineContextTarget, f32, f32), _w, cx| {
                    let target = target.clone();
                    let x = *x;
                    let y = *y;
                    let _ = this.update(cx, |this, cx| {
                        let context_target = match target {
                            TimelineContextTarget::TimelineEmpty => ContextTarget::TimelineEmpty,
                            TimelineContextTarget::TrackHeader(id) => {
                                let _ = this.timeline.update(cx, |timeline, cx| {
                                    timeline.state.select_track(&id);
                                    cx.notify();
                                });
                                ContextTarget::Track(id)
                            }
                            TimelineContextTarget::Clip(id) => {
                                let _ = this.timeline.update(cx, |timeline, cx| {
                                    timeline.state.select_clip(&id);
                                    cx.notify();
                                });
                                ContextTarget::Clip(id)
                            }
                            TimelineContextTarget::Ruler(beat) => {
                                ContextTarget::TimelineRuler { beat }
                            }
                            TimelineContextTarget::TempoTrack {
                                beat,
                                bpm,
                                point_id,
                            } => ContextTarget::TempoTrack {
                                beat,
                                bpm,
                                point_id,
                            },
                            TimelineContextTarget::TimeSignatureTrack { beat, point_id } => {
                                ContextTarget::TimeSignatureTrack { beat, point_id }
                            }
                            TimelineContextTarget::TempoLaneHeader => ContextTarget::Tempo,
                            TimelineContextTarget::TimeSignatureLaneHeader => {
                                ContextTarget::TimeSignature
                            }
                        };
                        this.menu_bar.open_menu_id = None;
                        this.menu_bar.submenu_path.clear();
                        this.project_switcher.is_open = false;
                        this.open_popover = Some(OpenPopover::Context {
                            target: context_target,
                            x,
                            y,
                        });
                        cx.notify();
                    });
                },
            )
        };
        let _ = self.timeline.update(cx, |timeline, _cx| {
            timeline.set_context_menu_callback(Some(on_timeline_context));
        });
        let on_add_track: components::timeline::timeline::TimelineAddTrackCb = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |request, _w, cx| {
                let request = *request;
                let _ = this.update(cx, |this, cx| {
                    // Timeline requests originate while Timeline may already be mid-update.
                    // Use the request context to avoid a nested `timeline.update(...)`.
                    this.open_add_track_external_window_with_context(
                        AddTrackKind::Audio,
                        request.track_count,
                        request.has_master_track,
                        None,
                        cx,
                    );
                });
            })
        };
        let _ = self.timeline.update(cx, |timeline, _cx| {
            timeline.set_add_track_callback(Some(on_add_track));
        });

        // ── Top-menu callbacks ─────────────────────────────────────────────
        let on_open_menu: std::sync::Arc<
            dyn Fn(&(String, f32), &mut Window, &mut gpui::App) + 'static,
        > = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |(id, anchor_x): &(String, f32), _w, cx| {
                let id = id.clone();
                let anchor_x = *anchor_x;
                this.update(cx, |this, cx| {
                    if this.menu_bar.open_menu_id.as_deref() == Some(id.as_str()) {
                        this.menu_bar.open_menu_id = None;
                    } else {
                        this.menu_bar.open_menu_id = Some(id);
                        this.menu_bar.anchor = titlebar_label_anchor(anchor_x);
                    }
                    this.menu_bar.submenu_path.clear();
                    this.open_popover = None;
                    this.project_switcher.is_open = false;
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
        let on_toggle_submenu: std::sync::Arc<
            dyn Fn(&(usize, String), &mut Window, &mut gpui::App) + 'static,
        > = {
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
        let on_menu_command: std::sync::Arc<
            dyn Fn(&String, &mut Window, &mut gpui::App) + 'static,
        > = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |command: &String, w, cx| {
                let command = command.clone();
                let _ = this.update(cx, |this, cx| {
                    this.dispatch_command_id_from_bounds(&command, Some(w.bounds()), cx);
                    this.open_popover = None;
                    this.project_switcher.is_open = false;
                    cx.notify();
                });
            })
        };
        let on_project_open: std::sync::Arc<dyn Fn(&f32, &mut Window, &mut gpui::App) + 'static> = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |anchor_x: &f32, w, cx| {
                let anchor_x = *anchor_x;
                let _ = this.update(cx, |this, cx| {
                    this.menu_bar.open_menu_id = None;
                    this.menu_bar.submenu_path.clear();
                    this.open_popover = None;
                    this.text_context_menu = None;
                    this.project_switcher.is_open = !this.project_switcher.is_open;
                    this.project_switcher.anchor = project_title_anchor(anchor_x);
                    if this.project_switcher.is_open {
                        this.project_switcher.query.clear();
                        this.project_switcher_search_input.set_value("");
                        this.project_switcher_search_input.focus_handle.focus(w, cx);
                        this.project_switcher.selected_index = 0;
                    }
                    cx.notify();
                });
            })
        };

        let open_menu_id = self.menu_bar.open_menu_id.clone();
        let menu_anchor = self.menu_bar.anchor;
        let submenu_path = self.menu_bar.submenu_path.clone();
        let viewport_width: f32 = window.bounds().size.width.into();
        let viewport_height: f32 = window.bounds().size.height.into();

        let chrome_policy = crate::platform_chrome::PlatformChromePolicy::current();
        let dropdown_overlay = if chrome_policy.show_in_window_menubar {
            open_menu_id.as_ref().and_then(|id| {
                if id == components::menu_bar::MENU_PICKER_ID {
                    Some(
                        components::menu_bar::menu_picker_dropdown(
                            menu_anchor,
                            viewport_width,
                            viewport_height,
                            on_open_menu.clone(),
                            on_close_menu.clone(),
                        )
                        .into_any_element(),
                    )
                } else {
                    let manifest = crate::menu::MenuManifest::load();
                    manifest.menus.iter().find(|m| &m.id == id).map(|menu| {
                        components::menu_dropdown::menu_dropdown(
                            menu,
                            menu_anchor,
                            viewport_width,
                            viewport_height,
                            &submenu_path,
                            on_toggle_submenu.clone(),
                            on_menu_command.clone(),
                            on_close_menu.clone(),
                        )
                        .into_any_element()
                    })
                }
            })
        } else {
            None
        };
        let on_close_popover: std::sync::Arc<dyn Fn(&(), &mut Window, &mut gpui::App) + 'static> = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |_: &(), _w, cx| {
                let _ = this.update(cx, |this, cx| {
                    this.open_popover = None;
                    this.project_switcher.is_open = false;
                    this.text_context_menu = None;
                    cx.notify();
                });
            })
        };
        let on_popover_command: std::sync::Arc<
            dyn Fn(&String, &mut Window, &mut gpui::App) + 'static,
        > = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |command: &String, w, cx| {
                let command = command.clone();
                let _ = this.update(cx, |this, cx| {
                    this.dispatch_command_id_from_bounds(&command, Some(w.bounds()), cx);
                    this.open_popover = None;
                    this.project_switcher.is_open = false;
                    cx.notify();
                });
            })
        };
        let popover_overlay = if self.project_switcher.is_open {
            let search_context_callbacks = TextInputCallbacks {
                on_context_menu: Some(Arc::new({
                    let this = cx.entity().clone();
                    move |(x, y): &(f32, f32), _w, cx| {
                        let x = *x;
                        let y = *y;
                        let _ = this.update(cx, |this, cx| {
                            this.text_context_menu = Some(TextContextMenu {
                                target: TextMenuTarget::ProjectSwitcherSearch,
                                x,
                                y,
                            });
                            cx.notify();
                        });
                    }
                })),
                on_mouse: None,
            };
            Some(
                components::project_switcher::project_switcher_popover(
                    &self.project_switcher,
                    &self.project_switcher_search_input,
                    self.project_switcher_search_input.is_focused(window),
                    search_context_callbacks,
                    viewport_width,
                    viewport_height,
                    on_popover_command.clone(),
                    on_close_popover.clone(),
                )
                .into_any_element(),
            )
        } else {
            match self.open_popover.clone() {
                Some(OpenPopover::Context { target, x, y }) => Some(
                    components::context_menu::context_menu_overlay(
                        self.context_entries(&target, cx),
                        x,
                        y,
                        viewport_width,
                        viewport_height,
                        on_popover_command.clone(),
                        on_close_popover.clone(),
                    )
                    .into_any_element(),
                ),
                None => None,
            }
        };
        // Settings is now an external window — no overlay needed.
        let settings_overlay: Option<gpui::AnyElement> = None;
        let text_context_overlay = self.text_context_menu.map(|menu| {
            let clipboard_has_text = cx
                .read_from_clipboard()
                .and_then(|item| item.text())
                .is_some_and(|text| !text.is_empty());
            let entries =
                text_input_context_entries(self.text_input(menu.target), clipboard_has_text);
            let command_target = cx.entity().clone();
            let close_target = cx.entity().clone();
            components::context_menu::context_menu_overlay(
                entries,
                menu.x,
                menu.y,
                viewport_width,
                viewport_height,
                Arc::new(move |command: &String, _window, cx| {
                    let command = command.clone();
                    let _ = command_target.update(cx, |this, cx| {
                        if let Some(menu) = this.text_context_menu {
                            let input = this.text_input_mut(menu.target);
                            let _ = input.apply_context_command(&command, cx);
                            this.sync_text_input_target(menu.target);
                        }
                        this.text_context_menu = None;
                        cx.notify();
                    });
                }),
                Arc::new(move |_: &(), _window, cx| {
                    let _ = close_target.update(cx, |this, cx| {
                        this.text_context_menu = None;
                        cx.notify();
                    });
                }),
            )
        });
        // Add Track moved to an external window.

        // Phase 2b insert plugin picker overlay.
        let plugin_picker_overlay_el: Option<gpui::AnyElement> = if self.plugin_picker.is_open {
            let search_context_callbacks = TextInputCallbacks {
                on_context_menu: Some(Arc::new({
                    let this = cx.entity().clone();
                    move |(x, y): &(f32, f32), _w, cx| {
                        let x = *x;
                        let y = *y;
                        let _ = this.update(cx, |this, cx| {
                            this.text_context_menu = Some(TextContextMenu {
                                target: TextMenuTarget::PluginPickerSearch,
                                x,
                                y,
                            });
                            cx.notify();
                        });
                    }
                })),
                on_mouse: None,
            };
            let picker_callbacks = PluginPickerCallbacks {
                on_close: Arc::new({
                    let this = cx.entity().clone();
                    move |_: &(), _w, cx| {
                        let _ = this.update(cx, |this, cx| {
                            this.plugin_picker = PluginPickerState::closed();
                            cx.notify();
                        });
                    }
                }),
                on_select: Arc::new({
                    let this = cx.entity().clone();
                    move |plugin_id: &String, _w, cx| {
                        let plugin_id = plugin_id.clone();
                        let _ = this.update(cx, |this, cx| {
                            if let Some(index) = this.plugin_search_index.as_ref() {
                                let result = compute_filter_result(
                                    index,
                                    &this.plugin_picker.query,
                                    &this.plugin_picker.filters,
                                    &this.plugin_picker_prefs,
                                    std::env::var_os("FUTUREBOARD_PLUGIN_PICKER_DEBUG").is_some(),
                                );
                                if let Some(highlight) = result.indices.iter().position(|&idx| {
                                    index.plugin_at(idx).is_some_and(|p| p.id == plugin_id)
                                }) {
                                    this.plugin_picker.highlighted_index = highlight;
                                }
                            }
                            this.plugin_picker.selected_id = Some(plugin_id);
                            cx.notify();
                        });
                    }
                }),
                on_select_filter: Arc::new({
                    let this = cx.entity().clone();
                    move |filter: &PickerFilter, _w, cx| {
                        let filter = filter.clone();
                        let _ = this.update(cx, |this, cx| {
                            this.plugin_picker.set_sidebar_filter(filter);
                            if let Some(index) = this.plugin_search_index.as_ref() {
                                ensure_default_highlight(
                                    &mut this.plugin_picker,
                                    index,
                                    &this.plugin_picker_prefs,
                                );
                            }
                            cx.notify();
                        });
                    }
                }),
                on_toggle_favorite: Arc::new({
                    let this = cx.entity().clone();
                    move |plugin_id: &String, _w, cx| {
                        let plugin_id = plugin_id.clone();
                        let _ = this.update(cx, |this, cx| {
                            this.plugin_picker_prefs.toggle_favorite(&plugin_id);
                            cx.notify();
                        });
                    }
                }),
                on_pick: Arc::new({
                    let this = cx.entity().clone();
                    move |plugin_id: &String, w, cx| {
                        let plugin_id = plugin_id.clone();
                        let _ = this.update(cx, |this, cx| {
                            if let Some((track_id, insert_index, insert_id)) =
                                this.apply_picked_insert(&plugin_id, cx)
                            {
                                this.open_insert_editor(&track_id, insert_index, &insert_id, w, cx);
                            }
                        });
                    }
                }),
                on_retry_load: Arc::new({
                    let this = cx.entity().clone();
                    move |_: &(), _w, cx| {
                        let _ = this.update(cx, |this, cx| {
                            this.available_plugins = None;
                            this.plugin_search_index = None;
                            this.plugin_catalog_status = PluginCatalogStatus::Loading;
                            this.arm_catalog_load(cx);
                            cx.notify();
                        });
                    }
                }),
                on_open_plugin_manager: Arc::new({
                    let this = cx.entity().clone();
                    move |_: &(), window, cx| {
                        let _ = this.update(cx, |this, cx| {
                            this.plugin_picker = PluginPickerState::closed();
                            let _ = window;
                            this.open_plugin_manager_external_window(None, cx);
                            cx.notify();
                        });
                    }
                }),
                on_rebuild_database: Arc::new({
                    let this = cx.entity().clone();
                    move |_: &(), _w, cx| {
                        let _ = this.update(cx, |this, cx| {
                            // Drop the SQLite file outright; next picker open
                            // reports MissingDatabase, prompting Scan Now.
                            let _ = sphere_plugin_host::plugin_db::delete_database_file();
                            this.available_plugins = None;
                            this.plugin_search_index = None;
                            this.plugin_catalog_status = PluginCatalogStatus::Loading;
                            this.arm_catalog_load(cx);
                            cx.notify();
                        });
                    }
                }),
            };
            let catalog_status = self.plugin_catalog_status.clone();
            Some(
                plugin_picker_overlay(
                    &self.plugin_picker,
                    self.plugin_search_index.as_ref(),
                    &self.plugin_picker_prefs,
                    catalog_status,
                    &self.plugin_picker_search_input,
                    self.plugin_picker_search_input.is_focused(window),
                    search_context_callbacks,
                    picker_callbacks,
                    self.plugin_picker_au_error.as_deref(),
                )
                .into_any_element(),
            )
        } else {
            None
        };

        self.prune_mixer_window(cx);
        self.prune_midi_editor_window(cx);

        let transport_chrome = self.transport_chrome_state(cx);
        let panel_chrome = self.panel_chrome_state(cx);
        let show_browser = self.panels.browser;
        let show_inspector = self.panels.inspector;
        let show_mixer_docked = self.panels.mixer_docked;

        // Push the real chrome metrics into Timeline so its scroll/grid
        // math knows the actual available body rect — accounts for the
        // current bottom panel height (vs. a hardcoded 220), and the
        // visibility of the browser/inspector side panels. Without this
        // the timeline grid stays at its old size after resize/maximize
        // and leaves blank space on the right or bottom.
        {
            const SIDEBAR_WIDTH: f32 = 272.0; // matches sidebar::SIDEBAR_WIDTH
            const INSPECTOR_WIDTH: f32 = 292.0; // matches inspector_shell().w(px(292.0))
            const STATUS_BAR_HEIGHT: f32 = 22.0; // matches title_bar::STATUSBAR_HEIGHT
            let metrics = components::timeline::TimelineChromeMetrics {
                browser_width: if show_browser { SIDEBAR_WIDTH } else { 0.0 },
                inspector_width: if show_inspector { INSPECTOR_WIDTH } else { 0.0 },
                bottom_panel_height: if show_mixer_docked {
                    self.bottom_panel_state.height_px
                } else {
                    0.0
                },
                status_bar_height: STATUS_BAR_HEIGHT,
            };
            let _ = self
                .timeline
                .update(cx, |timeline, _cx| timeline.set_chrome_metrics(metrics));
        }
        let project_chrome = components::ProjectChromeState {
            name: self.project_session.display_name().to_string(),
            is_dirty: self.project_session.is_dirty,
            on_open_project_menu: on_project_open,
        };
        let (status_left, status_right) = self.status_text();
        let shortcut_target = cx.entity().clone();
        // Docked MIDI editor — consulted in the key handler so Ctrl+A/C/V/X and
        // Delete route to the piano roll (its own `on_key_down`) when it holds
        // focus, instead of the global timeline clip commands.
        let midi_editor = self.piano_roll.clone();

        // Keep keyboard focus on our shortcut anchor so transport shortcuts
        // (Space, Enter, L, K, R, Home) reach `capture_key_down` below. GPUI
        // dispatches key events along the focused element's path; when focus is
        // None — OR stale (stuck on a search field whose overlay has since
        // closed, which GPUI still reports as "focused") — the dispatch path
        // falls back to the synthetic root node, which does NOT include this
        // div's `capture_key_down`, so every shortcut silently dies.
        //
        // Reclaim the anchor whenever it isn't focused and no *live* text field
        // is capturing the keyboard. This is intentionally stricter than
        // `window.focused().is_none()`: it also recovers from orphaned focus,
        // while never stealing focus from a field the user is actively typing in.
        if !self.focus_handle.is_focused(window) && !self.keyboard_text_capture_live(window) {
            self.focus_handle.focus(window, cx);
        }
        let focus_holder = self.focus_handle.clone();

        div()
            // NOTE: `track_focus` deliberately lives on the tiny invisible
            // `focus_holder` child below, NOT on this root. Putting it on
            // the root makes GPUI insert a full-window Normal hitbox
            // (see `should_insert_hitbox` — `tracked_focus_handle.is_some()`
            // triggers it). That hitbox is benign for click dispatch, but
            // on Windows it lands above the chrome's
            // `WindowControlArea::Drag` hitbox in the `mouse_hit_test.ids`
            // vector — which the NCHITTEST callback iterates in
            // window-control-vector order, not z-order — and the OS sees
            // a non-caption hit, refusing to start the window move.
            // Hoisting focus onto a 0×0 child preserves shortcut
            // delivery without adding the full-window hitbox.
            .flex()
            .flex_col()
            .size_full()
            .relative()
            .bg(Colors::surface_base())
            .font(theme::ui_font())
            .capture_key_down(move |event, window, cx| {
                let handled = shortcut_target.update(cx, |this, cx| {
                    let handled = this.handle_bpm_edit_key(event, window, cx)
                        || this.handle_ts_edit_key(event, window, cx)
                        || this.handle_settings_dialog_key(event, window, cx)
                        || this.handle_add_track_dialog_key(event, window, cx)
                        || this.handle_plugin_picker_key(event, window, cx)
                        || this.handle_project_switcher_key(event, window, cx)
                        || this.handle_inspector_key(event, window, cx)
                        || this.handle_browser_key(event, window, cx);
                    if handled {
                        cx.notify();
                    }
                    handled
                });
                if handled {
                    return;
                }
                let focus = FocusContext {
                    text_input_focused: shortcut_target.read(cx).text_input_has_focus(window),
                };
                if key_debug() {
                    eprintln!(
                        "[key] key={:?} text_input_focused={} held={} (plugin editor, when active, \
                         consumes keys before this handler)",
                        event.keystroke.key, focus.text_input_focused, event.is_held
                    );
                }
                if focus.text_input_focused && is_text_input_key(event) {
                    if key_debug() {
                        eprintln!(
                            "[key] ignored key={:?} reason=text-input-focused (typed into field)",
                            event.keystroke.key
                        );
                    }
                    return;
                }
                if event.keystroke.key.as_str() == "escape" {
                    let _ = shortcut_target.update(cx, |this, cx| {
                        // Cancel an active BPM scrub first, restoring the value
                        // captured at drag start.
                        this.cancel_bpm_drag(cx);
                        let _ = this.timeline.update(cx, |timeline, cx| {
                            timeline.reset_input_state();
                            cx.notify();
                        });
                        this.menu_bar.open_menu_id = None;
                        this.menu_bar.submenu_path.clear();
                        this.open_popover = None;
                        this.text_context_menu = None;
                        this.project_switcher.is_open = false;
                        cx.notify();
                    });
                    return;
                }
                let command_id = shortcut_target.read(cx).shortcut_command_id(event);
                if let Some(command_id) = command_id {
                    // MIDI editor focus gate: when the docked piano roll holds
                    // keyboard focus, the A/C/V/X/Delete family belongs to it.
                    // Skip global dispatch (which would target timeline clips and
                    // could nested-update) and let the event bubble to the piano
                    // roll's `on_key_down`. See PART D/E of the shortcuts task.
                    if is_midi_routable_edit_command(&normalize_command_id(&command_id))
                        && midi_editor.read(cx).is_focused(window)
                    {
                        if edit_command_debug() {
                            eprintln!(
                                "[edit-command] command={command_id} target=MidiEditor \
                                 reason=focus-passthrough (handled by piano roll)"
                            );
                        }
                        return;
                    }
                    // Transport shortcuts go through the same dispatcher as the
                    // chrome Play button (transport:play-pause → PlayPause), so
                    // Spacebar and the button are always one command. Only the
                    // focus gate differs between them.
                    let is_transport = transport_command_from_id(&command_id).is_some();
                    if is_transport && !should_handle_global_transport_shortcut(&focus) {
                        if key_debug() {
                            eprintln!(
                                "[key] ignored command={command_id} reason=global-transport-shortcut-suppressed"
                            );
                        }
                        return;
                    }
                    if command_id == "transport:play-pause"
                        && event.keystroke.key.eq_ignore_ascii_case("space")
                    {
                        eprintln!("[KeyCommand] Spacebar -> TransportTogglePlay");
                    }
                    if key_debug() {
                        eprintln!("[key] dispatched command={command_id}");
                    }
                    let _ = shortcut_target.update(cx, |this, cx| {
                        this.dispatch_command_id_from_bounds(&command_id, Some(window.bounds()), cx);
                        cx.notify();
                    });
                }
            })
            // Invisible focus anchor. 0×0 means no visible footprint and
            // an effectively unreachable hitbox; `track_focus` only needs
            // it to register the focus handle. The root's
            // `capture_key_down` still fires for any key while this
            // descendant is focused (capture phase: root → focused).
            .child(div().w(px(0.0)).h(px(0.0)).track_focus(&focus_holder))
            .child({
                let _s = crate::perf::PerfScope::enter("AppChrome");
                components::app_chrome(
                    window,
                    open_menu_id.as_deref(),
                    on_open_menu,
                    project_chrome,
                    transport_chrome,
                    panel_chrome,
                )
            })
            .child({
                let mut main_row = div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0();
                if show_browser {
                    main_row = main_row.child({
                        let _s = crate::perf::PerfScope::enter("Sidebar");
                        components::sidebar(
                            &file_browser,
                            browser_scroll,
                            &self.browser_search_input,
                            self.browser_search_input.is_focused(window),
                            on_browser_search_context,
                            on_browser_toggle,
                            on_browser_select,
                            on_browser_activate,
                            on_browser_context,
                        )
                    });
                }
                main_row = main_row.child(self.timeline.clone());
                if show_inspector {
                    main_row = main_row.child({
                        let _s = crate::perf::PerfScope::enter("Inspector");
                        crate::components::panel::inspector_panel(
                            &tracks,
                            selected_track_id.as_deref(),
                            selected_clip_id.as_deref(),
                            find_clip_summary(&tracks, selected_clip_id.as_deref()),
                            &self.inspector_name_input,
                            inspector_name_focused,
                            &self.inspector_clip_name_input,
                            inspector_clip_name_focused,
                            &inspector_callbacks,
                        )
                    });
                }
                main_row
            })
            .children(if show_mixer_docked {
                let _s = crate::perf::PerfScope::enter("BottomPanel");
                Some(
                    components::bottom_panel(
                        self.active_bottom_tab,
                        panel_state,
                        &tracks,
                        &master,
                        selected_track_id.as_deref(),
                        mixer_callbacks,
                        mixer_scroll_x,
                        mixer_viewport_width,
                        on_mixer_scroll,
                        Some(self.clip_editor_panel.clone().into_any_element()),
                        on_tab_click,
                        on_resize_start,
                        on_resize_move,
                        on_resize_end,
                    )
                    .into_any_element(),
                )
            } else {
                None
            })
            .child({
                let _s = crate::perf::PerfScope::enter("StatusBar");
                components::status_bar(status_left, status_right)
            })
            // Dropdown overlay — rendered last so it sits above every other
            // panel. The dropdown's own backdrop captures click-outside.
            .children(dropdown_overlay)
            .children(popover_overlay)
            .children(inspector_routing_combo_overlay)
            // Add Track moved to external window.
            .children(settings_overlay)
            .children(plugin_picker_overlay_el)
            .children(text_context_overlay)
    }
}
