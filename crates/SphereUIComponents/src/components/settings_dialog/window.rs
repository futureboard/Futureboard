//! Split out of `settings_dialog.rs` (god-file decomposition). `use super::*`.

use super::*;

impl SettingsWindow {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        settings: Entity<SettingsModel>,
        available_inputs: Vec<String>,
        available_outputs: Vec<String>,
        available_backends: Vec<String>,
        available_input_channels: Vec<(String, u32)>,
        available_output_channels: Vec<(String, u32)>,
        latency_provider: AudioLatencySnapshotProvider,
        input_test_start: Option<InputTestStartFn>,
        input_test_stop: Option<InputTestStopFn>,
        input_test_level: Option<InputTestLevelFn>,
        on_update: OnSettingUpdate,
        cx: &mut Context<Self>,
    ) -> Self {
        let search_input = TextInputState::new("settings-search", cx.focus_handle())
            .with_placeholder("Search settings...");
        Self {
            settings,
            active_tab: SettingsTab::General,
            search_input,
            available_inputs,
            available_outputs,
            available_backends,
            available_input_channels,
            available_output_channels,
            latency_provider,
            input_test_start,
            input_test_stop,
            input_test_level,
            input_test_active: false,
            input_test_level_value: 0.0,
            input_test_error: None,
            open_hardware_combo: None,
            hardware_combo_anchor: None,
            midi_refresh_nonce: 0,
            on_update,
            focus_handle: cx.focus_handle(),
        }
    }

    pub(super) fn start_input_test(&mut self, cx: &mut Context<Self>) {
        let Some(start) = self.input_test_start.clone() else {
            self.input_test_error = Some("audio engine unavailable".to_string());
            cx.notify();
            return;
        };
        let device = self
            .settings
            .read(cx)
            .current
            .hardware
            .audio
            .device_in
            .clone();
        match start(Some(device)) {
            Ok(()) => {
                self.input_test_active = true;
                self.input_test_level_value = 0.0;
                self.input_test_error = None;
                self.schedule_input_test_poll(cx);
            }
            Err(error) => {
                self.input_test_active = false;
                self.input_test_level_value = 0.0;
                self.input_test_error = Some(error);
            }
        }
        cx.notify();
    }

    pub(super) fn stop_input_test(&mut self, cx: &mut Context<Self>) {
        if let Some(stop) = self.input_test_stop.as_ref() {
            stop();
        }
        self.input_test_active = false;
        self.input_test_level_value = 0.0;
        cx.notify();
    }

    pub(super) fn schedule_input_test_poll(&mut self, cx: &mut Context<Self>) {
        if !self.input_test_active {
            return;
        }
        let level_provider = self.input_test_level.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(50))
                .await;
            let _ = this.update(cx, |window, cx| {
                if !window.input_test_active {
                    return;
                }
                if window.active_tab != SettingsTab::Recording {
                    window.stop_input_test(cx);
                    return;
                }
                window.input_test_level_value = level_provider
                    .as_ref()
                    .map(|read| read().clamp(0.0, 1.0))
                    .unwrap_or(0.0);
                window.schedule_input_test_poll(cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn handle_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.key.as_str() == "escape" && self.open_hardware_combo.take().is_some() {
            self.hardware_combo_anchor = None;
            cx.notify();
            return;
        }

        let search_focused = self.search_input.is_focused(window);
        if search_focused {
            let action = self.search_input.handle_key_with_clipboard(event, Some(cx));
            match action {
                TextInputAction::Cancel => window.remove_window(),
                _ => {}
            }
            cx.notify();
            return;
        }
        let key = event.keystroke.key.as_str();
        let ctrl = event.keystroke.modifiers.control || event.keystroke.modifiers.platform;
        match key {
            "escape" => {
                self.stop_input_test(cx);
                window.remove_window();
            }
            "f" if ctrl => {
                self.search_input.focus_handle.focus(window, cx);
                cx.notify();
            }
            _ => {}
        }
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let schema = self.settings.read(cx).current.clone();
        let i18n = I18n::new(&schema.general.language);
        self.search_input.placeholder = Some(i18n.tr("search.settings.placeholder"));
        let target = cx.entity().clone();
        let on_update = self.on_update.clone();
        let search_focused = self.search_input.is_focused(window);

        let state = SettingsDialogState {
            is_open: true,
            active_tab: self.active_tab,
            search_query: self.search_input.value.clone(),
        };

        let callbacks = SettingsDialogCallbacks {
            on_close: Arc::new(|_: &(), window: &mut Window, _cx: &mut App| {
                window.remove_window();
            }),
            on_select_tab: Arc::new({
                let target = target.clone();
                move |tab: &SettingsTab, _w: &mut Window, cx: &mut App| {
                    let tab = *tab;
                    let _ = target.update(cx, |this, cx| {
                        if tab != SettingsTab::Recording && this.input_test_active {
                            this.stop_input_test(cx);
                        }
                        this.active_tab = tab;
                        this.open_hardware_combo = None;
                        this.hardware_combo_anchor = None;
                        cx.notify();
                    });
                }
            }),
            on_update_setting: Arc::new({
                let on_update = on_update.clone();
                let target = target.clone();
                move |updater: UpdateSettingFn, _w: &mut Window, cx: &mut App| {
                    (on_update)(updater, cx);
                    let _ = target.update(cx, |this, cx| {
                        this.open_hardware_combo = None;
                        this.hardware_combo_anchor = None;
                        cx.notify();
                    });
                }
            }),
            on_toggle_input_test: Arc::new({
                let target = target.clone();
                move |_: &(), _w: &mut Window, cx: &mut App| {
                    let _ = target.update(cx, |this, cx| {
                        if this.input_test_active {
                            this.stop_input_test(cx);
                        } else {
                            this.start_input_test(cx);
                        }
                    });
                }
            }),
            on_refresh_midi: Some(Arc::new({
                let target = target.clone();
                move |_w: &mut Window, cx: &mut App| {
                    // Manual refresh runs a real off-render MIDI scan, then a
                    // single notify so the cached list re-renders once (no loop).
                    let revision = crate::device_registry::scan_midi();
                    let _ = target.update(cx, |this, cx| {
                        this.midi_refresh_nonce = this.midi_refresh_nonce.wrapping_add(1);
                        if midi_settings_debug_enabled() {
                            eprintln!(
                                "[MIDI settings] refresh requested (nonce={} registry_revision={revision})",
                                this.midi_refresh_nonce
                            );
                        }
                        cx.notify();
                    });
                }
            })),
            open_hardware_combo: self.open_hardware_combo,
            on_toggle_hardware_combo: Arc::new({
                let target = target.clone();
                move |combo: HardwareCombo,
                      anchor: Option<OverlayAnchor>,
                      _w: &mut Window,
                      cx: &mut App| {
                    let _ = target.update(cx, |this, cx| {
                        if this.open_hardware_combo == Some(combo) {
                            this.open_hardware_combo = None;
                            this.hardware_combo_anchor = None;
                        } else {
                            this.open_hardware_combo = Some(combo);
                            this.hardware_combo_anchor = anchor;
                        }
                        cx.notify();
                    });
                }
            }),
        };

        let search_callbacks = TextInputCallbacks {
            on_context_menu: None,
            on_mouse: None,
        };

        let latency = (self.latency_provider)();
        let input_test = InputTestMeterState {
            active: self.input_test_active,
            level: self.input_test_level_value,
            error: self.input_test_error.clone(),
        };
        let _midi_refresh = self.midi_refresh_nonce;

        let (sidebar_items, sections) = build_settings_content(
            &state,
            &schema,
            &callbacks,
            &latency,
            &input_test,
            &self.available_inputs,
            &self.available_outputs,
            &self.available_backends,
            &self.available_input_channels,
            &self.available_output_channels,
        );

        let sw_target = target.clone();

        let combo_overlay = if let (Some(open_combo), Some(anchor)) =
            (self.open_hardware_combo, self.hardware_combo_anchor)
        {
            if !anchor_visible_in_window(anchor, window) {
                self.open_hardware_combo = None;
                self.hardware_combo_anchor = None;
                None
            } else {
                let close_target = sw_target.clone();
                let overlay_update = Arc::new({
                    let on_update = on_update.clone();
                    let target = sw_target.clone();
                    move |updater: UpdateSettingFn, _w: &mut Window, cx: &mut App| {
                        (on_update)(updater, cx);
                        let _ = target.update(cx, |this, cx| {
                            this.open_hardware_combo = None;
                            this.hardware_combo_anchor = None;
                            cx.notify();
                        });
                    }
                });
                Some(hardware_combo_overlay(
                    open_combo,
                    anchor,
                    window,
                    &schema,
                    &self.available_inputs,
                    &self.available_outputs,
                    &self.available_backends,
                    overlay_update,
                    close_target,
                ))
            }
        } else {
            None
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .relative()
            .font(theme::ui_font_for_language(&schema.general.language))
            .bg(Colors::surface_window())
            .overflow_hidden()
            .capture_key_down({
                let target = sw_target.clone();
                move |event, window, cx| {
                    let _ = target.update(cx, |this, cx| this.handle_key(event, window, cx));
                }
            })
            .child(div().w(px(0.0)).h(px(0.0)).track_focus(&self.focus_handle))
            .child(external_window_titlebar(
                i18n.tr("settings.title"),
                "settings-window-close",
                {
                    let target = sw_target.clone();
                    move |window, cx| {
                        let _ = target.update(cx, |this, cx| {
                            if this.input_test_active {
                                this.stop_input_test(cx);
                            }
                            this.open_hardware_combo = None;
                            this.hardware_combo_anchor = None;
                            cx.notify();
                        });
                        window.remove_window();
                    }
                },
            ))
            // Two-column body — DAW studio control center layout
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id("settings-sidebar")
                            .w(px(SETTINGS_SIDEBAR_WIDTH))
                            .flex_shrink_0()
                            .border_r(px(1.0))
                            .border_color(Colors::divider())
                            .bg(Colors::surface_panel_alt())
                            .overflow_y_scroll()
                            .py(px(6.0))
                            .flex()
                            .flex_col()
                            .children(sidebar_items),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .bg(Colors::surface_panel())
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .px(px(SETTINGS_CONTENT_PAD))
                                    .pt(px(10.0))
                                    .pb(px(8.0))
                                    .border_b(px(1.0))
                                    .border_color(Colors::divider())
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .items_start()
                                            .justify_between()
                                            .gap(px(12.0))
                                            .child(settings_page_header(
                                                i18n.tr(self.active_tab.label_key()),
                                                i18n.tr(self.active_tab.page_description_key()),
                                            ))
                                            .child(div().w(px(208.0)).flex_shrink_0().child(
                                                text_field_with_callbacks(
                                                    &self.search_input,
                                                    search_focused,
                                                    search_callbacks,
                                                ),
                                            )),
                                    ),
                            )
                            .child({
                                let scroll_close = sw_target.clone();
                                div()
                                    .id("settings-content-scroll")
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_y_scroll()
                                    .p(px(SETTINGS_CONTENT_PAD))
                                    .flex()
                                    .flex_col()
                                    .gap(px(10.0))
                                    .on_scroll_wheel(move |_, _window, cx| {
                                        let _ = scroll_close.update(cx, |this, cx| {
                                            if this.open_hardware_combo.take().is_some() {
                                                this.hardware_combo_anchor = None;
                                                cx.notify();
                                            }
                                        });
                                        cx.stop_propagation();
                                    })
                                    .children(sections)
                            }),
                    ),
            )
            .children(combo_overlay)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn open_settings_window(
    owner_bounds: Option<Bounds<gpui::Pixels>>,
    settings: Entity<SettingsModel>,
    available_inputs: Vec<String>,
    available_outputs: Vec<String>,
    available_backends: Vec<String>,
    available_input_channels: Vec<(String, u32)>,
    available_output_channels: Vec<(String, u32)>,
    latency_provider: AudioLatencySnapshotProvider,
    input_test_start: Option<InputTestStartFn>,
    input_test_stop: Option<InputTestStopFn>,
    input_test_level: Option<InputTestLevelFn>,
    on_update: OnSettingUpdate,
    cx: &mut App,
) -> Result<WindowHandle<SettingsWindow>, String> {
    let window_bounds = centered_window_bounds(
        owner_bounds,
        size(px(SETTINGS_WIDTH), px(SETTINGS_HEIGHT)),
        cx,
    );

    let mut options = crate::platform_chrome::external_dialog_window_options_partial();
    options.window_bounds = Some(WindowBounds::Windowed(window_bounds));
    options.kind = WindowKind::Floating;
    options.is_resizable = true;
    options.is_minimizable = false;
    options.window_background = WindowBackgroundAppearance::Transparent;
    options.window_min_size = Some(size(px(SETTINGS_WIDTH), px(SETTINGS_HEIGHT)));
    apply_owner_display(&mut options, owner_bounds, cx);

    cx.open_window(options, move |_window, cx| {
        cx.new(|cx| {
            SettingsWindow::new(
                settings,
                available_inputs,
                available_outputs,
                available_backends,
                available_input_channels,
                available_output_channels,
                latency_provider,
                input_test_start,
                input_test_stop,
                input_test_level,
                on_update,
                cx,
            )
        })
    })
    .map_err(|error| error.to_string())
}
