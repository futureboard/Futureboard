//! Split out of `timeline.rs`: `impl Render for Timeline` + scrollbar/overlay helpers.

use super::*;

impl Render for Timeline {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _tl_scope = crate::perf::PerfScope::enter("Timeline");
        let (viewport_w, viewport_h, (scroll_max_x, scroll_max_y)) = self.scroll_geometry(window);
        self.state.update_viewport_size(viewport_w, viewport_h);
        self.state.clamp_scroll(scroll_max_x, scroll_max_y);
        let scrolling = self.state.smooth_scroll_towards_target();
        if scrolling {
            cx.notify();
        }
        if self.focus_lost_subscription.is_none() {
            self.focus_lost_subscription = Some(cx.on_focus_lost(window, |this, _window, cx| {
                if this.range_select_drag.is_some()
                    || this.pen_clip_draw.is_some()
                    || this.erase_clip_drag.is_some()
                    || this.automation_drag.is_some()
                    || this.automation_marquee.is_some()
                    || this.tempo_drag.is_some()
                    || this.pan_last_position.is_some()
                {
                    if Self::input_debug_enabled() {
                        eprintln!("[selection] focus_lost_cancel");
                    }
                    this.cancel_active_gesture(cx);
                }
            }));
        }

        crate::perf::count(
            "clips",
            self.state
                .tracks
                .iter()
                .map(|t| t.clips.len() as u64)
                .sum::<u64>(),
        );

        let on_select_track = cx.listener(|this, track_id: &String, _window, cx| {
            this.state.select_track(track_id);
            cx.notify();
        });

        let on_select_clip = cx.listener(
            |this, (clip_id, additive, clone_drag): &(String, bool, bool), _window, cx| {
                if this.state.active_tool == TimelineTool::Pen {
                    let is_audio = this
                        .state
                        .find_clip(clip_id)
                        .map(|(_, clip)| matches!(clip.clip_type, ClipType::Audio { .. }))
                        .unwrap_or(false);
                    if is_audio {
                        if let Some((track_id, clip)) =
                            this.state.build_clip_duplicate_after(clip_id)
                        {
                            this.run_edit_command(EditCommand::CreateClip { track_id, clip }, cx);
                        }
                        return;
                    }
                }
                this.clip_clone_drag_id = clone_drag.then(|| clip_id.clone());
                if *additive {
                    this.state.select_clip_additive(clip_id);
                } else if this.state.selection.selected_clip_ids.len() > 1
                    && this
                        .state
                        .selection
                        .selected_clip_ids
                        .iter()
                        .any(|id| id == clip_id)
                {
                    this.state.arrangement_range = None;
                } else {
                    this.state.select_clip(clip_id);
                }
                cx.notify();
            },
        );

        let on_toggle_mute = cx.listener(|this, track_id: &String, _window, cx| {
            this.state.toggle_track_mute(track_id);
            this.mark_project_changed(cx);
            if let Some(track) = this.state.find_track(track_id) {
                if let Some(cb) = this.on_track_param_change.as_ref() {
                    cb(
                        track_id.clone(),
                        "mute".to_string(),
                        if track.muted { 1.0 } else { 0.0 },
                    );
                }
            }
            cx.notify();
        });

        let on_toggle_solo = cx.listener(|this, track_id: &String, _window, cx| {
            this.state.toggle_track_solo(track_id);
            this.mark_project_changed(cx);
            if let Some(track) = this.state.find_track(track_id) {
                if let Some(cb) = this.on_track_param_change.as_ref() {
                    cb(
                        track_id.clone(),
                        "solo".to_string(),
                        if track.solo { 1.0 } else { 0.0 },
                    );
                }
            }
            cx.notify();
        });

        let on_toggle_arm = cx.listener(|this, track_id: &String, _window, cx| {
            this.state.toggle_track_arm(track_id);
            this.mark_project_changed(cx);
            cx.notify();
        });

        let on_toggle_input = cx.listener(|this, track_id: &String, _window, cx| {
            this.state.cycle_track_input_monitor(track_id);
            this.mark_project_changed(cx);
            cx.notify();
        });

        // Automation mode toggle is UI-only: it selects the track and flips the
        // lane edit mode but never marks the project/engine dirty on its own.
        let on_toggle_automation = cx.listener(|this, track_id: &String, _window, cx| {
            this.state.select_track(track_id);
            this.state.toggle_track_lane_mode(track_id);
            cx.notify();
        });

        let on_delete_track = cx.listener(|this, track_id: &String, _window, cx| {
            this.state.tracks.retain(|t| t.id != *track_id);
            if this.state.selection.selected_track_id.as_ref() == Some(track_id) {
                this.state.selection.selected_track_id = None;
            }
            this.mark_project_changed(cx);
            cx.notify();
        });

        let on_volume_change =
            cx.listener(|this, (track_id, volume): &(String, f32), _window, cx| {
                this.state.set_track_volume(track_id, *volume);
                this.mark_project_changed(cx);
                if let Some(cb) = this.on_track_param_change.as_ref() {
                    cb(track_id.clone(), "volume".to_string(), *volume);
                }
                cx.notify();
            });

        let on_pan_change = cx.listener(|this, (track_id, pan): &(String, f32), _window, cx| {
            this.state.set_track_pan(track_id, *pan);
            this.mark_project_changed(cx);
            if let Some(cb) = this.on_track_param_change.as_ref() {
                cb(track_id.clone(), "pan".to_string(), *pan);
            }
            cx.notify();
        });

        let on_add_clip = cx.listener(|this, (track_id, beat): &(String, f32), _window, cx| {
            let track_type = this
                .state
                .tracks
                .iter()
                .find(|t| t.id == *track_id)
                .map(|t| t.track_type);
            match track_type {
                Some(TrackType::Audio) => {
                    if this.state.active_tool == TimelineTool::Pen {
                        let source_id = this
                            .state
                            .selection
                            .selected_clip_ids
                            .iter()
                            .find(|id| {
                                this.state
                                    .find_clip(id)
                                    .map(|(_, clip)| {
                                        matches!(clip.clip_type, ClipType::Audio { .. })
                                    })
                                    .unwrap_or(false)
                            })
                            .cloned();
                        if let Some(source_id) = source_id {
                            let start = this.snap_beat(*beat);
                            this.create_clip_clone_at(&source_id, track_id, start, cx);
                        }
                    }
                }
                Some(TrackType::Midi | TrackType::Instrument) => {
                    if this.state.active_tool == TimelineTool::Pen {
                        let start = this.snap_beat(*beat);
                        this.pen_clip_draw = Some(ClipDrawPreview {
                            track_id: track_id.clone(),
                            start_beat: start,
                            current_beat: start,
                            dragging: false,
                        });
                    }
                }
                Some(TrackType::Bus | TrackType::Return | TrackType::Master) | None => {}
            }
            cx.notify();
        });

        let on_range_start = cx.listener(
            |this, (track_id, beat, additive): &(String, f32, bool), _window, cx| {
                if this.state.active_tool == TimelineTool::Pointer {
                    if Self::input_debug_enabled() {
                        eprintln!(
                            "[selection] marquee_start_pending track={} beat={:.3} additive={}",
                            track_id, beat, additive
                        );
                    }
                    this.range_select_drag = Some(RangeSelectDrag {
                        start_beat: *beat,
                        current_beat: *beat,
                        start_track_id: track_id.clone(),
                        additive: *additive,
                        dragging: false,
                    });
                    this.state.arrangement_range = None;
                    cx.notify();
                }
            },
        );

        let on_erase_start = cx.listener(|this, beat: &f32, _window, cx| {
            this.begin_erase_at(*beat, None, cx);
        });

        let on_erase_clip = cx.listener(|this, clip_id: &String, _window, cx| {
            let beat = this
                .state
                .tracks
                .iter()
                .flat_map(|t| t.clips.iter())
                .find(|c| c.id == *clip_id)
                .map(|c| c.start_beat)
                .unwrap_or(0.0);
            this.begin_erase_at(beat, Some(clip_id.clone()), cx);
        });

        let on_edit_mouse_move = cx.listener(|this, event: &gpui::MouseMoveEvent, _window, cx| {
            if Self::input_debug_enabled() {
                eprintln!(
                    "[timeline-input] mouse-move pressed={:?} range_drag={} ctrl={} platform={} shift={}",
                    event.pressed_button,
                    this.range_select_drag.is_some(),
                    event.modifiers.control,
                    event.modifiers.platform,
                    event.modifiers.shift,
                );
            }
            if event.pressed_button.is_none()
                && (this.pen_clip_draw.is_some()
                    || this.range_select_drag.is_some()
                    || this.erase_clip_drag.is_some()
                    || this.automation_drag.is_some()
                    || this.automation_marquee.is_some()
                    || this.tempo_drag.is_some()
                    || this.ts_drag.is_some()
                    || this.pan_last_position.is_some())
            {
                this.reset_input_state();
                cx.notify();
                return;
            }
            if event.pressed_button == Some(gpui::MouseButton::Left)
                && (this.automation_drag.is_some()
                    || this.automation_marquee.is_some()
                    || this.tempo_drag.is_some()
                    || this.ts_drag.is_some())
            {
                if this.tempo_drag.is_some() {
                    this.update_tempo_track_interaction(
                        event.position.x.into(),
                        event.position.y.into(),
                        cx,
                    );
                } else if this.ts_drag.is_some() {
                    this.update_time_signature_track_interaction(
                        event.position.x.into(),
                        event.position.y.into(),
                        cx,
                    );
                } else {
                    this.update_automation_interaction(
                        event.position.x.into(),
                        event.position.y.into(),
                        cx,
                    );
                }
                return;
            }
            if event.pressed_button == Some(gpui::MouseButton::Right)
                && this.erase_clip_drag.is_some()
            {
                let beat = this.snap_beat(this.beat_from_window_x(event.position.x.into()));
                this.update_erase_clip_drag(beat, cx);
            } else if event.pressed_button == Some(gpui::MouseButton::Left)
                && this.range_select_drag.is_some()
            {
                let beat = this.snap_beat(this.beat_from_window_x(event.position.x.into()));
                let lane_y = this.track_area_y_from_window(event.position);
                let current_track_id = this.state.lane_y_to_track_id(lane_y);
                let mut overlay: Option<TimelineRangeSelection> = None;
                if let Some(drag) = this.range_select_drag.as_mut() {
                    drag.current_beat = beat;
                    let dx = this.state.beats_to_x(beat) - this.state.beats_to_x(drag.start_beat);
                    let dy_tracks = current_track_id
                        .as_ref()
                        .and_then(|id| {
                            let start_idx = this
                                .state
                                .tracks
                                .iter()
                                .position(|track| track.id == drag.start_track_id)?;
                            let current_idx = this
                                .state
                                .tracks
                                .iter()
                                .position(|track| track.id == *id)?;
                            Some(((current_idx as isize - start_idx as isize).abs() as f32) * TRACK_HEIGHT)
                        })
                        .unwrap_or(0.0);
                    if !drag.dragging && (dx * dx + dy_tracks * dy_tracks).sqrt() >= MARQUEE_DRAG_THRESHOLD {
                        drag.dragging = true;
                        if Self::input_debug_enabled() {
                            eprintln!("[selection] marquee_start additive={}", drag.additive);
                        }
                    }
                    if drag.dragging {
                        let (lo, hi) = normalize_range(drag.start_beat, beat);
                        let end_track_id = current_track_id.unwrap_or_else(|| drag.start_track_id.clone());
                        overlay = Some(TimelineRangeSelection::new(
                            lo as f64,
                            hi as f64,
                            this.state.track_ids_between(&drag.start_track_id, &end_track_id),
                        ));
                    }
                }
                this.state.arrangement_range = overlay;
                if Self::input_debug_enabled() {
                    if let Some(drag) = this.range_select_drag.as_ref() {
                        eprintln!(
                            "[selection] marquee_update dragging={} beat={:.3}",
                            drag.dragging, drag.current_beat
                        );
                    }
                }
                cx.notify();
            } else if event.pressed_button == Some(gpui::MouseButton::Left)
                && this.state.active_tool == TimelineTool::Pen
                && this.pen_clip_draw.is_some()
            {
                // Live MIDI clip draw: track the snapped cursor beat so the ghost
                // preview expands/shrinks in real time. No project mutation —
                // the real clip is created once on release.
                let beat = this.snap_beat(this.beat_from_window_x(event.position.x.into()));
                if let Some(preview) = this.pen_clip_draw.as_mut() {
                    if (beat - preview.current_beat).abs() > f32::EPSILON {
                        preview.current_beat = beat;
                        if (beat - preview.start_beat).abs() > f32::EPSILON {
                            preview.dragging = true;
                        }
                        cx.notify();
                    }
                }
            }
        });

        let on_pen_mouse_up = cx.listener(|this, event: &gpui::MouseUpEvent, _window, cx| {
            this.log_input_state("mouse-up-left");
            let finished_tempo = this.finish_tempo_track_interaction(cx);
            let finished_ts = this.finish_time_signature_track_interaction(cx);
            let finished_automation = this.finish_automation_interaction(cx);
            if !finished_tempo && !finished_ts && !finished_automation {
                let beat = this.snap_beat(this.beat_from_window_x(event.position.x.into()));
                if this.state.active_tool == TimelineTool::Pen && this.pen_clip_draw.is_some() {
                    this.finish_pen_midi_clip(beat, cx);
                } else if this.state.active_tool == TimelineTool::Pointer
                    && this.range_select_drag.is_some()
                {
                    this.finish_range_select(beat, cx);
                } else if this.erase_clip_drag.is_some() {
                    this.finish_erase_clip_drag(cx);
                }
            }
            this.reset_input_state();
            debug_assert!(this.range_select_drag.is_none());
            cx.notify();
        });
        let on_pen_mouse_up_out = cx.listener(|this, event: &gpui::MouseUpEvent, _window, cx| {
            this.log_input_state("mouse-up-left-out");
            let finished_tempo = this.finish_tempo_track_interaction(cx);
            let finished_ts = this.finish_time_signature_track_interaction(cx);
            let finished_automation = this.finish_automation_interaction(cx);
            if !finished_tempo && !finished_ts && !finished_automation {
                let beat = this.snap_beat(this.beat_from_window_x(event.position.x.into()));
                if this.state.active_tool == TimelineTool::Pen && this.pen_clip_draw.is_some() {
                    this.finish_pen_midi_clip(beat, cx);
                } else if this.state.active_tool == TimelineTool::Pointer
                    && this.range_select_drag.is_some()
                {
                    this.finish_range_select(beat, cx);
                } else if this.erase_clip_drag.is_some() {
                    this.finish_erase_clip_drag(cx);
                }
            }
            this.reset_input_state();
            debug_assert!(this.range_select_drag.is_none());
            cx.notify();
        });

        let on_add_track = cx.listener(|this, _: &(), window, cx| {
            if let Some(callback) = this.on_add_track.as_ref() {
                callback(
                    &TimelineAddTrackRequest {
                        track_count: this.state.tracks.len(),
                        has_master_track: this
                            .state
                            .tracks
                            .iter()
                            .any(|track| track.track_type == TrackType::Master),
                    },
                    window,
                    cx,
                );
            } else {
                let id = this.state.create_audio_track();
                this.state.select_track(&id);
                cx.notify();
            }
        });

        let on_toggle_snap = cx.listener(|this, _: &(), _window, cx| {
            this.state.snap_to_grid = !this.state.snap_to_grid;
            cx.notify();
        });

        let on_cycle_grid = cx.listener(|this, _: &(), _window, cx| {
            this.state.grid_division = match this.state.grid_division {
                SnapDivision::Auto => SnapDivision::Off,
                SnapDivision::Off => SnapDivision::Bar1,
                SnapDivision::Bar1 => SnapDivision::Div1_1,
                SnapDivision::Div1_1 => SnapDivision::Div1_2,
                SnapDivision::Div1_2 => SnapDivision::Div1_4,
                SnapDivision::Div1_4 => SnapDivision::Div1_8,
                SnapDivision::Div1_8 => SnapDivision::Div1_16,
                SnapDivision::Div1_16 => SnapDivision::Div1_32,
                SnapDivision::Div1_32 => SnapDivision::Div1_64,
                SnapDivision::Div1_64 => SnapDivision::Auto,
            };
            cx.notify();
        });

        let on_seek = cx.listener(|this, click_x: &f32, _window, cx| {
            let beats = this.state.x_to_beats(*click_x);
            this.seek_to_beat(beats, cx);
        });

        let on_select_tool = cx.listener(|this, tool: &TimelineTool, _window, cx| {
            this.log_input_state("tool-change");
            this.reset_input_state();
            this.state.active_tool = *tool;
            cx.notify();
        });

        // Smooth, continuous zoom factor — small per-click multiplier so the
        // px/bt label changes feel like a real ramp rather than a jump.
        // Anchor at the viewport center (no cursor info here) so zoom stays
        // visually stable when driven from the buttons.
        let on_zoom_in = cx.listener(|this, _: &(), window, cx| {
            let viewport_w: f32 = window.bounds().size.width.into();
            let anchor = ((viewport_w - SIDEBAR_WIDTH - HEADER_WIDTH) * 0.5).max(0.0);
            this.state.zoom_by(1.10, anchor);
            cx.notify();
        });

        let on_zoom_out = cx.listener(|this, _: &(), window, cx| {
            let viewport_w: f32 = window.bounds().size.width.into();
            let anchor = ((viewport_w - SIDEBAR_WIDTH - HEADER_WIDTH) * 0.5).max(0.0);
            this.state.zoom_by(1.0 / 1.10, anchor);
            cx.notify();
        });

        // Wrap callbacks in std::sync::Arc to allow easy cloning when passing down to sub-elements
        let on_select_track: std::sync::Arc<
            dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_select_track);
        let on_select_clip: std::sync::Arc<
            dyn Fn(&(String, bool, bool), &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_select_clip);
        let on_toggle_mute: std::sync::Arc<
            dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_toggle_mute);
        let on_toggle_solo: std::sync::Arc<
            dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_toggle_solo);
        let on_toggle_arm: std::sync::Arc<
            dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_toggle_arm);
        let on_toggle_input: std::sync::Arc<
            dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_toggle_input);
        let on_toggle_automation: std::sync::Arc<
            dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_toggle_automation);
        let on_delete_track: std::sync::Arc<
            dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_delete_track);
        let on_volume_change: std::sync::Arc<
            dyn Fn(&(String, f32), &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_volume_change);
        let _on_pan_change: std::sync::Arc<
            dyn Fn(&(String, f32), &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_pan_change);
        let on_add_clip: std::sync::Arc<
            dyn Fn(&(String, f32), &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_add_clip);
        let on_add_track: std::sync::Arc<dyn Fn(&(), &mut gpui::Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(on_add_track);
        let on_toggle_snap: std::sync::Arc<
            dyn Fn(&(), &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_toggle_snap);
        let on_cycle_grid: std::sync::Arc<
            dyn Fn(&(), &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_cycle_grid);
        let on_seek: std::sync::Arc<dyn Fn(&f32, &mut gpui::Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(on_seek);

        // Right-click on the ruler → position-aware tempo menu. Converts the
        // markings-area-local x to a beat and forwards the screen position so
        // the overlay anchors under the cursor.
        let on_ruler_context = cx.listener(
            |this, payload: &(f32, f32, f32), window: &mut gpui::Window, cx| {
                let (click_x, sx, sy) = *payload;
                let beat = this.state.x_to_beats(click_x).max(0.0) as f64;
                if let Some(cb) = this.on_context_menu.clone() {
                    cb(&(TimelineContextTarget::Ruler(beat), sx, sy), window, cx);
                }
            },
        );
        let on_region_drag = cx.listener(|this, update: &TimelineRegionDragUpdate, _window, cx| {
            if this
                .state
                .update_region_range(&update.region_id, update.start_beat, update.end_beat)
            {
                this.mark_project_changed(cx);
                cx.notify();
            }
        });
        let on_region_drag: std::sync::Arc<
            dyn Fn(&TimelineRegionDragUpdate, &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_region_drag);
        let on_loop_drag = cx.listener(|this, update: &TimelineLoopDragUpdate, _window, cx| {
            let start = update.start_beat.min(update.end_beat).max(0.0);
            let end = update.start_beat.max(update.end_beat).max(start + 1.0e-3);
            let transport = &mut this.state.transport;
            if (transport.loop_start_beats - start).abs() > f32::EPSILON
                || (transport.loop_end_beats - end).abs() > f32::EPSILON
            {
                transport.loop_start_beats = start;
                transport.loop_end_beats = end;
                transport.loop_enabled = true;
                this.mark_loop_changed(cx);
                cx.notify();
            }
        });
        let on_loop_drag: std::sync::Arc<
            dyn Fn(&TimelineLoopDragUpdate, &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_loop_drag);
        let on_ruler_context: std::sync::Arc<
            dyn Fn(&(f32, f32, f32), &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_ruler_context);
        let on_select_tool: std::sync::Arc<
            dyn Fn(&TimelineTool, &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_select_tool);
        let on_range_start: std::sync::Arc<
            dyn Fn(&(String, f32, bool), &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_range_start);
        let on_erase_start: std::sync::Arc<
            dyn Fn(&f32, &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_erase_start);
        let on_erase_clip: std::sync::Arc<
            dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_erase_clip);
        let on_zoom_in: std::sync::Arc<dyn Fn(&(), &mut gpui::Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(on_zoom_in);
        let on_zoom_out: std::sync::Arc<dyn Fn(&(), &mut gpui::Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(on_zoom_out);
        let on_timeline_context = self.on_context_menu.clone();
        let on_track_context_menu = self.on_context_menu.clone().map(|cb| {
            std::sync::Arc::new(
                move |(track_id, x, y): &(String, f32, f32),
                      window: &mut gpui::Window,
                      cx: &mut gpui::App| {
                    cb(
                        &(TimelineContextTarget::TrackHeader(track_id.clone()), *x, *y),
                        window,
                        cx,
                    );
                },
            )
                as std::sync::Arc<
                    dyn Fn(&(String, f32, f32), &mut gpui::Window, &mut gpui::App) + 'static,
                >
        });
        let on_clip_context_menu = self.on_context_menu.clone().map(|cb| {
            std::sync::Arc::new(
                move |(clip_id, x, y): &(String, f32, f32),
                      window: &mut gpui::Window,
                      cx: &mut gpui::App| {
                    cb(
                        &(TimelineContextTarget::Clip(clip_id.clone()), *x, *y),
                        window,
                        cx,
                    );
                },
            )
                as std::sync::Arc<
                    dyn Fn(&(String, f32, f32), &mut gpui::Window, &mut gpui::App) + 'static,
                >
        });

        let on_automation_down =
            cx.listener(|this, payload: &(String, f32, f32, bool), _window, cx| {
                let (track_id, beat, value, additive) =
                    (payload.0.clone(), payload.1, payload.2, payload.3);
                this.begin_automation_interaction(&track_id, beat, value, additive, cx);
            });
        let on_automation_down: crate::components::timeline::track_lane::AutomationDownCallback =
            std::sync::Arc::new(on_automation_down);

        // Cycle the automation target (in-lane target chip). Committed edit —
        // changes the focused lane and which lane persists.
        let on_automation_cycle = cx.listener(|this, track_id: &String, _window, cx| {
            if this.state.cycle_automation_target(track_id).is_some() {
                this.mark_project_changed(cx);
                cx.notify();
            }
        });
        let on_automation_cycle: crate::components::timeline::track_lane::AutomationCycleCallback =
            std::sync::Arc::new(on_automation_cycle);

        let on_tempo_down = cx.listener(
            |this, payload: &(f64, f64, Option<String>, bool, u32), _window, cx| {
                let (beat, bpm, point_id, _additive, click_count) = (
                    payload.0,
                    payload.1,
                    payload.2.clone(),
                    payload.3,
                    payload.4,
                );
                this.begin_tempo_track_interaction(beat, bpm, point_id, click_count, cx);
            },
        );
        let on_tempo_down: crate::components::timeline::tempo_track::TempoTrackDownCallback =
            std::sync::Arc::new(on_tempo_down);

        let on_tempo_context = self.on_context_menu.clone().map(|cb| {
            std::sync::Arc::new(
                move |(beat, bpm, point_id, x, y): &(f64, f64, Option<String>, f32, f32),
                      window: &mut gpui::Window,
                      cx: &mut gpui::App| {
                    cb(
                        &(
                            TimelineContextTarget::TempoTrack {
                                beat: *beat,
                                bpm: *bpm,
                                point_id: point_id.clone(),
                            },
                            *x,
                            *y,
                        ),
                        window,
                        cx,
                    );
                },
            ) as crate::components::timeline::tempo_track::TempoTrackContextCallback
        });

        let on_tempo_add = cx.listener(|this, _: &(), _window, cx| {
            this.add_tempo_point_at_playhead_from_header(cx);
        });
        let on_tempo_add: crate::components::timeline::tempo_track::GlobalLaneVoidCallback =
            std::sync::Arc::new(on_tempo_add);

        let on_tempo_header_menu = self.on_context_menu.clone().map(|cb| {
            std::sync::Arc::new(
                move |pos: &(f32, f32), window: &mut gpui::Window, cx: &mut gpui::App| {
                    cb(
                        &(TimelineContextTarget::TempoLaneHeader, pos.0, pos.1),
                        window,
                        cx,
                    );
                },
            ) as crate::components::timeline::tempo_track::GlobalLaneMenuCallback
        });

        let on_tempo_hide = cx.listener(|this, _: &(), _window, cx| {
            this.state.hide_tempo_track_lane();
            cx.notify();
        });
        let on_tempo_hide: crate::components::timeline::tempo_track::GlobalLaneVoidCallback =
            std::sync::Arc::new(on_tempo_hide);

        let on_tempo_toggle_collapsed = cx.listener(|this, _: &(), _window, cx| {
            this.state.tempo_track_collapsed = !this.state.tempo_track_collapsed;
            cx.notify();
        });
        let on_tempo_toggle_collapsed: crate::components::timeline::tempo_track::GlobalLaneVoidCallback =
            std::sync::Arc::new(on_tempo_toggle_collapsed);

        let on_ts_down = cx.listener(
            |this, payload: &(f64, Option<String>, bool, u32), _window, cx| {
                let (beat, point_id, _additive, click_count) =
                    (payload.0, payload.1.clone(), payload.2, payload.3);
                this.begin_time_signature_track_interaction(beat, point_id, click_count, cx);
            },
        );
        let on_ts_down: crate::components::timeline::time_signature_track::TimeSignatureTrackDownCallback =
            std::sync::Arc::new(on_ts_down);

        let on_ts_context = self.on_context_menu.clone().map(|cb| {
            std::sync::Arc::new(
                move |(beat, point_id, x, y): &(f64, Option<String>, f32, f32),
                      window: &mut gpui::Window,
                      cx: &mut gpui::App| {
                    cb(
                        &(
                            TimelineContextTarget::TimeSignatureTrack {
                                beat: *beat,
                                point_id: point_id.clone(),
                            },
                            *x,
                            *y,
                        ),
                        window,
                        cx,
                    );
                },
            ) as crate::components::timeline::time_signature_track::TimeSignatureTrackContextCallback
        });

        let on_ts_add = cx.listener(|this, _: &(), _window, cx| {
            this.add_time_signature_marker_at_playhead_from_header(cx);
        });
        let on_ts_add: crate::components::timeline::time_signature_track::GlobalLaneVoidCallback =
            std::sync::Arc::new(on_ts_add);

        let on_ts_header_menu = self.on_context_menu.clone().map(|cb| {
            std::sync::Arc::new(
                move |pos: &(f32, f32), window: &mut gpui::Window, cx: &mut gpui::App| {
                    cb(
                        &(TimelineContextTarget::TimeSignatureLaneHeader, pos.0, pos.1),
                        window,
                        cx,
                    );
                },
            )
                as crate::components::timeline::time_signature_track::GlobalLaneMenuCallback
        });

        let on_ts_hide = cx.listener(|this, _: &(), _window, cx| {
            this.state.hide_time_signature_track_lane();
            cx.notify();
        });
        let on_ts_hide: crate::components::timeline::time_signature_track::GlobalLaneVoidCallback =
            std::sync::Arc::new(on_ts_hide);

        let on_ts_toggle_collapsed = cx.listener(|this, _: &(), _window, cx| {
            this.state.time_signature_track_collapsed = !this.state.time_signature_track_collapsed;
            cx.notify();
        });
        let on_ts_toggle_collapsed: crate::components::timeline::time_signature_track::GlobalLaneVoidCallback =
            std::sync::Arc::new(on_ts_toggle_collapsed);

        let header_callbacks = crate::components::timeline::track_header::TrackHeaderCallbacks {
            on_select_track: on_select_track.clone(),
            on_toggle_mute: on_toggle_mute.clone(),
            on_toggle_solo: on_toggle_solo.clone(),
            on_toggle_arm: on_toggle_arm.clone(),
            on_toggle_input: on_toggle_input.clone(),
            on_toggle_automation: on_toggle_automation.clone(),
            on_delete_track: on_delete_track.clone(),
            on_volume_change: on_volume_change.clone(),
            on_context_menu: on_track_context_menu.clone(),
        };

        let state = &self.state;
        let tempo_h = state.tempo_track_height();
        let ts_h = state.time_signature_track_height();
        let content_top = state.arrangement_content_top();
        // Live pen-draw ghost clip (built before the chain to keep the borrow of
        // `self.pen_clip_draw` separate from the render closures).
        let pen_preview_overlay = self
            .pen_clip_draw
            .as_ref()
            .and_then(|preview| pen_clip_draw_overlay(preview, state));
        let on_zoom_in_btn = on_zoom_in.clone();
        let on_zoom_out_btn = on_zoom_out.clone();

        // ── Scrollbar geometry ──────────────────────────────────────────
        // Computed once per render against the live window size. Both
        // tracks (visible bar) are 8 px wide and sit at the right/bottom
        // edges of the lane area. Clicking the track jumps the scroll
        // position to that point — gives a functional scrollbar without
        // needing a stateful drag.
        let content_w = self.timeline_content_width();
        let content_h = (self.state.tracks.len() as f32 * TRACK_HEIGHT).max(1.0);
        let lane_view_h = viewport_h.max(TRACK_HEIGHT);
        let lane_view_w = viewport_w.max(1.0);

        // ── Drag/drop import wiring ─────────────────────────────────────
        // Track the mouse position throughout an external file drag so that
        // when `on_drop` fires we can resolve the drop coordinates.
        let on_drag_track = cx.listener(
            |this, event: &gpui::DragMoveEvent<ExternalPaths>, _window, _cx| {
                this.last_drag_position = Some(event.event.position);
            },
        );

        let on_files_dropped = cx.listener(|this, paths: &ExternalPaths, _window, cx| {
            let mut any_imported = false;
            // Multi-file drops: the first file lands at the cursor; subsequent
            // files always land on a brand-new track (forced via y past the end).
            let mut force_new_track = false;
            for path in paths.paths().iter() {
                let imported =
                    this.import_midi_path_at_last_drag(path, force_new_track, _window, cx)
                        || this.import_audio_path_at_last_drag(path, force_new_track, _window, cx);
                any_imported |= imported;
                force_new_track |= imported;
            }
            if any_imported {
                this.last_drag_position = None;
                cx.notify();
            }
        });

        let on_browser_drag_track = cx.listener(
            |this, event: &gpui::DragMoveEvent<BrowserDragItem>, _window, _cx| {
                this.last_drag_position = Some(event.event.position);
            },
        );

        let on_browser_file_dropped = cx.listener(|this, item: &BrowserDragItem, window, cx| {
            if this.import_midi_path_at_last_drag(&item.path, false, window, cx)
                || this.import_audio_path_at_last_drag(&item.path, false, window, cx)
            {
                this.last_drag_position = None;
                cx.notify();
            }
        });

        let on_clip_drag_move = cx.listener(
            |this, event: &gpui::DragMoveEvent<ClipDragItem>, window, cx| {
                let drag = event.drag(cx).clone();
                this.last_drag_position = Some(event.event.position);
                if this.clip_clone_drag_id.as_deref() == Some(drag.clip_id.as_str()) {
                    let origin = *this.clip_drag_origin.get_or_insert(event.event.position);
                    let (target_index, _) =
                        this.resolve_clip_drag_target(&drag, origin, event.event.position);
                    this.clip_drag_target_track_index = Some(target_index);
                } else {
                    this.move_dragged_clip_to_position(&drag, event.event.position, window);
                }
                cx.notify();
            },
        );

        let on_clip_dropped = cx.listener(|this, drag: &ClipDragItem, _window, cx| {
            let target_index = this.clip_drag_target_track_index;
            if let Some(target_track_id) = target_index
                .and_then(|index| this.state.tracks.get(index))
                .map(|track| track.id.clone())
            {
                if this.clip_clone_drag_id.as_deref() == Some(drag.clip_id.as_str()) {
                    let origin = this
                        .clip_drag_origin
                        .unwrap_or_else(|| this.last_drag_position.unwrap_or_default());
                    let position = this.last_drag_position.unwrap_or(origin);
                    let (_, start_beat) = this.resolve_clip_drag_target(drag, origin, position);
                    this.create_clip_clone_at(&drag.clip_id, &target_track_id, start_beat, cx);
                } else {
                    let drag_ids = this.clip_drag_selection_ids(&drag.clip_id);
                    let resolved_target_index = target_index.unwrap_or_else(|| {
                        this.state
                            .tracks
                            .iter()
                            .position(|track| track.id == target_track_id)
                            .unwrap_or(0)
                    });
                    let source_index = this
                        .state
                        .tracks
                        .iter()
                        .position(|track| track.id == drag.source_track_id)
                        .unwrap_or(resolved_target_index);
                    let track_delta = resolved_target_index as isize - source_index as isize;
                    let max_index = this.state.tracks.len().saturating_sub(1) as isize;

                    for clip_id in &drag_ids {
                        let Some((track_index, current_start)) = this
                            .state
                            .tracks
                            .iter()
                            .enumerate()
                            .find_map(|(index, track)| {
                                track
                                    .clips
                                    .iter()
                                    .find(|clip| clip.id == *clip_id)
                                    .map(|clip| (index, clip.start_beat))
                            })
                        else {
                            continue;
                        };
                        let target_track_id = this
                            .state
                            .tracks
                            .get((track_index as isize + track_delta).clamp(0, max_index) as usize)
                            .map(|track| track.id.clone())
                            .unwrap_or_else(|| target_track_id.clone());
                        this.state
                            .move_clip_to_track(clip_id, &target_track_id, current_start);
                    }
                    this.restore_clip_drag_selection(
                        &drag.clip_id,
                        drag_ids,
                        Some(target_track_id),
                    );
                    this.mark_project_changed(cx);
                }
            }
            this.clip_drag_origin = None;
            this.clip_drag_target_track_index = None;
            this.clip_clone_drag_id = None;
            this.last_drag_position = None;
            cx.notify();
        });

        // Clip edge-resize: live-mutate the clip bounds on every drag move (no
        // dirty), then commit once on drop. `resize_clip` snaps internally.
        let on_clip_resize_move = cx.listener(
            |this, event: &gpui::DragMoveEvent<ClipResizeDrag>, _window, cx| {
                let drag = event.drag(cx).clone();
                let beat = this.beat_from_window_x(event.event.position.x.into());
                this.state.resize_clip(&drag.clip_id, drag.edge, beat);
                cx.notify();
            },
        );
        let on_clip_resize_drop = cx.listener(|this, _drag: &ClipResizeDrag, _window, cx| {
            this.mark_project_changed(cx);
            cx.notify();
        });

        let on_track_drag_move = cx.listener(
            |this, event: &gpui::DragMoveEvent<TrackDragItem>, _window, cx| {
                let drag = event.drag(cx).clone();
                let y = this.track_area_y_from_window(event.event.position);
                if this.state.dragging_track_id.as_deref() != Some(drag.track_id.as_str()) {
                    this.state
                        .begin_track_drag(&drag.track_id, drag.origin_index, y);
                }
                this.state.update_track_drag(y);
                cx.notify();
            },
        );

        let on_track_dropped = cx.listener(|this, drag: &TrackDragItem, _window, cx| {
            let target_index = this
                .state
                .drag_target_index
                .unwrap_or(drag.origin_index)
                .clamp(0, this.state.tracks.len());
            this.state.reorder_track(&drag.track_id, target_index);
            this.mark_project_changed(cx);
            cx.notify();
        });

        let on_middle_pan_start = cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
            this.pan_last_position = Some(event.position);
            window.prevent_default();
            cx.stop_propagation();
            cx.notify();
        });

        let on_middle_pan_move = cx.listener(|this, event: &gpui::MouseMoveEvent, window, cx| {
            if event.pressed_button != Some(gpui::MouseButton::Middle) {
                this.pan_last_position = None;
                return;
            }

            let Some(previous) = this.pan_last_position else {
                this.pan_last_position = Some(event.position);
                return;
            };

            let dx: f32 = (event.position.x - previous.x).into();
            let dy: f32 = (event.position.y - previous.y).into();
            let (max_x, max_y) = this.max_scroll_offsets(window);
            let next_x = this.state.viewport.scroll_x - dx;
            let next_y = this.state.viewport.scroll_y - dy;
            this.state
                .set_scroll_immediate(next_x, next_y, max_x, max_y);
            this.pan_last_position = Some(event.position);
            window.prevent_default();
            cx.stop_propagation();
            cx.notify();
        });

        let on_middle_pan_end = cx.listener(|this, _event: &gpui::MouseUpEvent, _window, cx| {
            this.pan_last_position = None;
            cx.notify();
        });

        let on_middle_pan_end_out =
            cx.listener(|this, _event: &gpui::MouseUpEvent, _window, cx| {
                this.pan_last_position = None;
                cx.notify();
            });

        let on_ctrl_wheel_zoom = cx.listener(|this, event: &gpui::ScrollWheelEvent, window, cx| {
            let delta = match event.delta {
                ScrollDelta::Pixels(p) => {
                    let x: f32 = p.x.into();
                    let y: f32 = p.y.into();
                    (x, y)
                }
                ScrollDelta::Lines(p) => (p.x * 36.0, p.y * 36.0),
            };

            if !event.modifiers.control {
                let (max_x, max_y) = this.max_scroll_offsets(window);
                let (scroll_x, scroll_y) = if event.modifiers.shift {
                    let horizontal = if delta.1.abs() > 0.01 {
                        delta.1
                    } else {
                        delta.0
                    };
                    (horizontal, 0.0)
                } else {
                    (delta.0, delta.1)
                };
                this.state.scroll_by(scroll_x, scroll_y, max_x, max_y);
                if scroll_x.abs() > 0.5 {
                    this.state.note_user_scrolled();
                }
                window.prevent_default();
                cx.stop_propagation();
                cx.notify();
                return;
            }

            window.prevent_default();
            cx.stop_propagation();

            if delta.1.abs() < 0.01 {
                return;
            }

            let x: f32 = event.position.x.into();
            let anchor = (x - SIDEBAR_WIDTH - HEADER_WIDTH).max(0.0);
            let factor = (1.0018_f32).powf(-delta.1);
            this.state.zoom_by(factor, anchor);
            let (max_x, max_y) = this.max_scroll_offsets(window);
            this.state.clamp_scroll(max_x, max_y);
            cx.notify();
        });

        let on_arrangement_context_menu = on_timeline_context.clone().map(|cb| {
            cx.listener(
                move |this, event: &gpui::MouseDownEvent, window: &mut gpui::Window, cx| {
                    let x: f32 = event.position.x.into();
                    let y: f32 = event.position.y.into();
                    let target = this.resolve_context_target_from_window_point(event.position);
                    cb(&(target, x, y), window, cx);
                },
            )
        });

        div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .bg(Colors::surface_base())
            .border_l(px(1.0))
            .border_r(px(1.0))
            .border_color(Colors::border_subtle())
            .relative()
            .capture_key_down(
                cx.listener(|this, event: &gpui::KeyDownEvent, _window, cx| {
                    if event.keystroke.key.as_str() == "escape"
                        && (this.range_select_drag.is_some()
                            || this.pen_clip_draw.is_some()
                            || this.erase_clip_drag.is_some()
                            || this.automation_drag.is_some()
                            || this.automation_marquee.is_some()
                            || this.pan_last_position.is_some())
                    {
                        cx.stop_propagation();
                        this.cancel_active_gesture(cx);
                    }
                }),
            )
            .on_drag_move::<ExternalPaths>(on_drag_track)
            .on_drop::<ExternalPaths>(on_files_dropped)
            .on_drag_move::<BrowserDragItem>(on_browser_drag_track)
            .on_drop::<BrowserDragItem>(on_browser_file_dropped)
            .on_drag_move::<ClipDragItem>(on_clip_drag_move)
            .on_drop::<ClipDragItem>(on_clip_dropped)
            .on_drag_move::<ClipResizeDrag>(on_clip_resize_move)
            .on_drop::<ClipResizeDrag>(on_clip_resize_drop)
            .on_drag_move::<TrackDragItem>(on_track_drag_move)
            .on_drop::<TrackDragItem>(on_track_dropped)
            .on_mouse_down(gpui::MouseButton::Middle, on_middle_pan_start)
            .when_some(on_arrangement_context_menu, |this, cb| {
                this.on_mouse_down(gpui::MouseButton::Right, cb)
            })
            .on_mouse_move(on_middle_pan_move)
            .on_mouse_move(on_edit_mouse_move)
            .on_mouse_up(gpui::MouseButton::Middle, on_middle_pan_end)
            .on_mouse_up_out(gpui::MouseButton::Middle, on_middle_pan_end_out)
            .on_mouse_up(gpui::MouseButton::Left, on_pen_mouse_up)
            .on_mouse_up_out(gpui::MouseButton::Left, on_pen_mouse_up_out)
            .on_mouse_up(
                gpui::MouseButton::Right,
                cx.listener(|this, _ev, _w, cx| {
                    this.log_input_state("mouse-up-right");
                    if this.erase_clip_drag.is_some() {
                        this.finish_erase_clip_drag(cx);
                    }
                    this.reset_input_state();
                    cx.notify();
                }),
            )
            .on_mouse_up_out(
                gpui::MouseButton::Right,
                cx.listener(|this, _ev, _w, cx| {
                    this.log_input_state("mouse-up-right-out");
                    if this.erase_clip_drag.is_some() {
                        this.finish_erase_clip_drag(cx);
                    }
                    this.reset_input_state();
                    cx.notify();
                }),
            )
            .on_scroll_wheel(on_ctrl_wheel_zoom)
            // 1. Timeline Ruler
            .child(timeline_ruler(
                state,
                on_add_track.clone(),
                on_toggle_snap.clone(),
                on_cycle_grid.clone(),
                on_seek.clone(),
                on_region_drag.clone(),
                on_loop_drag.clone(),
                on_ruler_context.clone(),
            ))
            // 1b. Global Tempo Track lane (below ruler, above tracks)
            .when(state.show_tempo_track, |this| {
                this.child(tempo_track_lane(
                    state,
                    tempo_h,
                    Some(on_tempo_down.clone()),
                    on_tempo_context.clone(),
                    Some(on_tempo_add.clone()),
                    on_tempo_header_menu.clone(),
                    Some(on_tempo_hide.clone()),
                    Some(on_tempo_toggle_collapsed.clone()),
                ))
            })
            .when(state.show_time_signature_track, |this| {
                this.child(time_signature_track_lane(
                    state,
                    ts_h,
                    Some(on_ts_down.clone()),
                    on_ts_context.clone(),
                    Some(on_ts_add.clone()),
                    on_ts_header_menu.clone(),
                    Some(on_ts_hide.clone()),
                    Some(on_ts_toggle_collapsed.clone()),
                ))
            })
            // 2. Track List Scroll Area
            .child(div().flex_1().min_h_0().relative().child(track_list(
                state,
                header_callbacks.clone(),
                on_select_track.clone(),
                on_select_clip.clone(),
                on_add_clip.clone(),
                on_track_context_menu.clone(),
                on_clip_context_menu.clone(),
                self.on_open_editor.clone(),
                Some(on_range_start.clone()),
                Some(on_erase_start.clone()),
                Some(on_erase_clip.clone()),
                Some(&self.erase_preview_ids),
                Some(on_automation_down.clone()),
                Some(on_automation_cycle.clone()),
                self.automation_marquee.as_ref(),
            )))
            .children(timeline_marker_region_overlay(state).map(|overlay| {
                div()
                    .absolute()
                    .left(px(HEADER_WIDTH))
                    .right_0()
                    .top(px(content_top))
                    .bottom_0()
                    .overflow_hidden()
                    .child(overlay)
            }))
            // 3. Playhead Overlay (frontmost timeline pass)
            // Render after ruler + content so grid/ruler/content never cover it.
            // Split into:
            // - head overlay (ruler strip only)
            // - body overlay (content strip only)
            // 2b. Arrangement range-selection overlay (UI-only). Drawn above the
            // lane content but below the playhead/tools so it never hides the
            // playhead. Follows zoom/scroll via the same lane coordinate space.
            .children(arrangement_range_overlay(state).map(|overlay| {
                div()
                    .absolute()
                    .left(px(HEADER_WIDTH))
                    .right_0()
                    .top(px(content_top))
                    .bottom_0()
                    .overflow_hidden()
                    .child(overlay)
            }))
            // Live pen-draw MIDI clip ghost preview (same lane coordinate space
            // as the arrangement overlay; above content, below the playhead).
            .children(pen_preview_overlay.map(|overlay| {
                div()
                    .absolute()
                    .left(px(HEADER_WIDTH))
                    .right_0()
                    .top(px(content_top))
                    .bottom_0()
                    .overflow_hidden()
                    .child(overlay)
            }))
            .child(
                div()
                    .absolute()
                    .left(px(HEADER_WIDTH))
                    .right_0()
                    .top_0()
                    .h(px(RULER_HEIGHT))
                    .overflow_hidden()
                    .child({
                        let playhead_x = state.beats_to_x(state.transport.playhead_beats);
                        if std::env::var_os("FUTUREBOARD_PLAYHEAD_DEBUG").is_some() {
                            eprintln!(
                                "[playhead x] beat={:.3} scroll_x={:.1} px_per_beat={:.3} x={:.1}",
                                state.transport.playhead_beats,
                                state.viewport.scroll_x,
                                state.viewport.pixels_per_beat,
                                playhead_x
                            );
                        }
                        crate::components::timeline::playhead::playhead_head_overlay_at(playhead_x)
                    }),
            )
            .child(
                div()
                    .absolute()
                    .left(px(HEADER_WIDTH))
                    .right_0()
                    .top(px(content_top))
                    .bottom_0()
                    .overflow_hidden()
                    .child({
                        let playhead_x = state.beats_to_x(state.transport.playhead_beats);
                        crate::components::timeline::playhead::playhead_body_overlay_at(playhead_x)
                    }),
            )
            // 4. Floating Tools Bar (above playhead)
            .child(
                div()
                    .absolute()
                    .bottom(px(16.0))
                    .left(px(16.0))
                    .child(floating_tools_bar(
                        state.active_tool,
                        on_select_tool.clone(),
                    )),
            )
            // 5. Vertical scrollbar (right edge, over the lane area)
            .child(vertical_scrollbar(
                cx,
                state.viewport.scroll_y,
                content_h,
                lane_view_h,
                scroll_max_y,
                content_top,
            ))
            // 6. Horizontal scrollbar (bottom edge, over the lane area)
            .child(horizontal_scrollbar(
                cx,
                state.viewport.scroll_x,
                content_w,
                lane_view_w,
                scroll_max_x,
            ))
            // 7. Zoom Controls
            .child(
                div()
                    .absolute()
                    .bottom(px(16.0))
                    .right(px(16.0))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(4.0))
                    .px(px(8.0))
                    .py(px(4.0))
                    .rounded_full()
                    .border(px(1.0))
                    .border_color(Colors::border_default())
                    .bg(Colors::surface_panel_alt())
                    .shadow_xl()
                    // Zoom Out Button
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .w(px(24.0))
                            .h(px(24.0))
                            .rounded_md()
                            .cursor(gpui::CursorStyle::PointingHand)
                            .text_color(Colors::text_secondary())
                            .id("zoom-out-btn")
                            .hover(|style| style.bg(Colors::surface_hover()))
                            .on_click(move |_, window, cx| {
                                on_zoom_out_btn(&(), window, cx);
                            })
                            .child(
                                svg()
                                    .path(assets::ICON_MINUS_PATH)
                                    .w(px(12.0))
                                    .h(px(12.0))
                                    .text_color(Colors::text_secondary()),
                            ),
                    )
                    // Zoom readout label
                    .child(
                        div()
                            .px(px(4.0))
                            .text_size(px(9.0))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(Colors::text_muted())
                            .child({
                                let ppb =
                                    state.viewport.pixels_per_second * state.seconds_per_beat();
                                if ppb >= 100.0 {
                                    format!("{:.0} px/bt", ppb)
                                } else {
                                    format!("{:.1} px/bt", ppb)
                                }
                            }),
                    )
                    // Zoom In Button
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .w(px(24.0))
                            .h(px(24.0))
                            .rounded_md()
                            .cursor(gpui::CursorStyle::PointingHand)
                            .text_color(Colors::text_secondary())
                            .id("zoom-in-btn")
                            .hover(|style| style.bg(Colors::surface_hover()))
                            .on_click(move |_, window, cx| {
                                on_zoom_in_btn(&(), window, cx);
                            })
                            .child(
                                svg()
                                    .path(assets::ICON_PLUS_PATH)
                                    .w(px(12.0))
                                    .h(px(12.0))
                                    .text_color(Colors::text_secondary()),
                            ),
                    ),
            )
    }
}

pub(crate) fn vertical_scrollbar(
    cx: &mut Context<Timeline>,
    scroll_y: f32,
    content_h: f32,
    view_h: f32,
    max_scroll: f32,
    content_top: f32,
) -> gpui::AnyElement {
    if max_scroll <= 0.5 || view_h <= 0.0 {
        return Empty.into_any_element();
    }
    let track_h = view_h;
    let thumb_h = ((view_h / content_h) * track_h).max(SCROLLBAR_MIN_THUMB);
    let progress = (scroll_y / max_scroll).clamp(0.0, 1.0);
    let thumb_top = progress * (track_h - thumb_h).max(0.0);

    let on_track_click = cx.listener(move |this, event: &gpui::MouseDownEvent, _w, cx| {
        // Position is in window space; convert to a fraction of the
        // scrollbar track. We approximate the track top as the click
        // y minus the thumb half-height when clicking above the thumb,
        // and snap the thumb center to the click otherwise.
        let click_y: f32 = event.position.y.into();
        // The scrollbar sits at top=RULER_HEIGHT inside the timeline.
        // Re-derive the local y by subtracting an estimated chrome
        // height; clamp with `max_scroll` so any over/under-estimate
        // still yields a valid scroll position.
        let local = (click_y - 36.0 - content_top).max(0.0);
        let frac = (local / track_h.max(1.0)).clamp(0.0, 1.0);
        this.state.set_scroll_immediate(
            this.state.viewport.scroll_x,
            (frac * max_scroll).clamp(0.0, max_scroll),
            f32::MAX,
            max_scroll,
        );
        cx.notify();
    });

    let on_thumb_drag = cx.listener(
        move |this, event: &gpui::DragMoveEvent<ScrollbarDrag>, _w, cx| {
            if event.drag(cx).axis != ScrollAxis::Vertical {
                return;
            }
            let y: f32 = event.event.position.y.into();
            let oy: f32 = event.bounds.origin.y.into();
            let track_range = (track_h - thumb_h).max(1.0);
            let local = (y - oy - thumb_h * 0.5).clamp(0.0, track_range);
            let frac = local / track_range;
            this.state.set_scroll_immediate(
                this.state.viewport.scroll_x,
                frac * max_scroll,
                f32::MAX,
                max_scroll,
            );
            cx.notify();
        },
    );

    div()
        .absolute()
        .top(px(content_top))
        .right(px(0.0))
        .bottom(px(0.0))
        .w(px(SCROLLBAR_THICKNESS))
        .id("timeline-vscroll")
        .on_mouse_down(gpui::MouseButton::Left, on_track_click)
        .on_drag(
            ScrollbarDrag {
                axis: ScrollAxis::Vertical,
            },
            |drag, _offset, _window, cx| cx.new(|_| drag.clone()),
        )
        .on_drag_move::<ScrollbarDrag>(on_thumb_drag)
        .child(
            div()
                .absolute()
                .top(px(thumb_top))
                .left(px(2.0))
                .right(px(2.0))
                .h(px(thumb_h))
                .rounded_full()
                .bg(Colors::with_alpha(Colors::text_primary(), 0.2)),
        )
        .into_any_element()
}

pub(crate) fn horizontal_scrollbar(
    cx: &mut Context<Timeline>,
    scroll_x: f32,
    content_w: f32,
    view_w: f32,
    max_scroll: f32,
) -> gpui::AnyElement {
    if max_scroll <= 0.5 || view_w <= 0.0 {
        return Empty.into_any_element();
    }
    let track_w = view_w;
    let thumb_w = ((view_w / content_w) * track_w).max(SCROLLBAR_MIN_THUMB);
    let progress = (scroll_x / max_scroll).clamp(0.0, 1.0);
    let thumb_left = progress * (track_w - thumb_w).max(0.0);

    let on_track_click = cx.listener(move |this, event: &gpui::MouseDownEvent, _w, cx| {
        let click_x: f32 = event.position.x.into();
        let local = (click_x - SIDEBAR_WIDTH - HEADER_WIDTH).max(0.0);
        let frac = (local / track_w.max(1.0)).clamp(0.0, 1.0);
        this.state.set_scroll_immediate(
            (frac * max_scroll).clamp(0.0, max_scroll),
            this.state.viewport.scroll_y,
            max_scroll,
            f32::MAX,
        );
        cx.notify();
    });

    let on_thumb_drag = cx.listener(
        move |this, event: &gpui::DragMoveEvent<ScrollbarDrag>, _w, cx| {
            if event.drag(cx).axis != ScrollAxis::Horizontal {
                return;
            }
            let x: f32 = event.event.position.x.into();
            let ox: f32 = event.bounds.origin.x.into();
            let track_range = (track_w - thumb_w).max(1.0);
            let local = (x - ox - thumb_w * 0.5).clamp(0.0, track_range);
            let frac = local / track_range;
            this.state.set_scroll_immediate(
                frac * max_scroll,
                this.state.viewport.scroll_y,
                max_scroll,
                f32::MAX,
            );
            cx.notify();
        },
    );

    div()
        .absolute()
        .bottom(px(0.0))
        .left(px(HEADER_WIDTH))
        .right(px(SCROLLBAR_THICKNESS))
        .h(px(SCROLLBAR_THICKNESS))
        .id("timeline-hscroll")
        .on_mouse_down(gpui::MouseButton::Left, on_track_click)
        .on_drag(
            ScrollbarDrag {
                axis: ScrollAxis::Horizontal,
            },
            |drag, _offset, _window, cx| cx.new(|_| drag.clone()),
        )
        .on_drag_move::<ScrollbarDrag>(on_thumb_drag)
        .child(
            div()
                .absolute()
                .left(px(thumb_left))
                .top(px(2.0))
                .bottom(px(2.0))
                .w(px(thumb_w))
                .rounded_full()
                .bg(Colors::with_alpha(Colors::text_primary(), 0.2)),
        )
        .into_any_element()
}

/// Translucent arrangement range-selection rectangle. Pure render of
/// `state.arrangement_range` — UI-only, follows zoom/scroll, and never touches
/// the engine or marks the project dirty. Spans the affected tracks vertically
/// and the selected beat span horizontally. Non-interactive so it does not
/// intercept lane drags. Returns `None` when no range is active.
/// Resolve a pen-draw gesture's `(start_beat, end_beat)` into the final
/// `(clip_start, length_beats)` that will be committed — snapping the length to
/// the MIDI grid when snap is on and clamping to the minimum clip length. Shared
/// by the live ghost preview and the commit so they can never disagree.
pub(crate) fn compute_pen_clip_span(state: &TimelineState, start_beat: f32, end_beat: f32) -> (f32, f32) {
    use crate::components::timeline::timeline_state::{
        DEFAULT_MIDI_CLIP_BEATS, MIN_MIDI_CLIP_BEATS,
    };
    let (lo, hi) = normalize_range(start_beat, end_beat);
    let mut len = (hi - lo).max(DEFAULT_MIDI_CLIP_BEATS);
    if state.snap_to_grid {
        let step = state.midi_snap_step_beats().max(1.0e-3);
        len = ((len / step).ceil() * step).max(MIN_MIDI_CLIP_BEATS);
    } else {
        len = len.max(MIN_MIDI_CLIP_BEATS);
    }
    (lo, len)
}

/// Human-readable musical length, e.g. `1 bar`, `4 bars`, `2.5 bars`, `3.0 bt`.
pub(crate) fn format_clip_length(length_beats: f32, beats_per_bar: f32) -> String {
    let bpb = beats_per_bar.max(1.0);
    let bars = length_beats / bpb;
    if (bars - bars.round()).abs() < 1.0e-3 && bars >= 1.0 {
        let n = bars.round() as i32;
        format!("{} bar{}", n, if n == 1 { "" } else { "s" })
    } else if bars >= 1.0 {
        format!("{:.1} bars", bars)
    } else {
        format!("{:.1} bt", length_beats)
    }
}

pub(crate) fn format_arrangement_target_debug(target: &ArrangementHitTarget) -> String {
    match target {
        ArrangementHitTarget::EmptyArrangement {
            timeline_beat,
            track_id,
        } => format!("track_id={track_id:?}\ntimeline_beat={timeline_beat:.3}"),
        ArrangementHitTarget::TrackHeader { track_id } => format!("track_id={track_id}"),
        ArrangementHitTarget::TrackLane {
            track_id,
            timeline_beat,
        } => format!("track_id={track_id}\ntimeline_beat={timeline_beat:.3}"),
        ArrangementHitTarget::AudioClip {
            track_id,
            clip_id,
            timeline_beat,
            local_beat,
        }
        | ArrangementHitTarget::MidiClip {
            track_id,
            clip_id,
            timeline_beat,
            local_beat,
        } => format!(
            "track_id={track_id}\nclip_id={clip_id}\ntimeline_beat={timeline_beat:.3}\nlocal_beat={local_beat:.3}"
        ),
        ArrangementHitTarget::Ruler { timeline_beat } => {
            format!("timeline_beat={timeline_beat:.3}")
        }
        ArrangementHitTarget::Marker {
            marker_id,
            timeline_beat,
        } => format!("marker_id={marker_id}\ntimeline_beat={timeline_beat:.3}"),
        ArrangementHitTarget::AutomationLane {
            track_id,
            lane_id,
            timeline_beat,
        } => format!("track_id={track_id}\nlane_id={lane_id}\ntimeline_beat={timeline_beat:.3}"),
    }
}

/// Live ghost-clip overlay for the in-flight pen MIDI clip draw. Translucent,
/// track-colored, with a pulsing outline and a floating length/range label so
/// the user sees the exact bounds and musical length before releasing.
pub(crate) fn pen_clip_draw_overlay(
    preview: &ClipDrawPreview,
    state: &TimelineState,
) -> Option<gpui::AnyElement> {
    let track_index = state.tracks.iter().position(|t| t.id == preview.track_id)?;
    let track_color = state.tracks[track_index].color;

    let (clip_start, length) =
        compute_pen_clip_span(state, preview.start_beat, preview.current_beat);
    let clip_end = clip_start + length;

    let x_lo = state.beats_to_x(clip_start);
    let width = (state.beats_to_x(clip_end) - x_lo).max(2.0);
    let pad = 7.0;
    let top = track_index as f32 * TRACK_HEIGHT - state.viewport.scroll_y + pad;
    let height = (TRACK_HEIGHT - pad * 2.0).max(1.0);

    let bpb = state.beats_per_bar();
    let length_label = format_clip_length(length, bpb);
    let range_label = format!(
        "{} → {}",
        state.format_bar_beat(clip_start),
        state.format_bar_beat(clip_end)
    );

    let ghost_fill = Colors::with_alpha(track_color, 0.16);
    let label_text = Colors::with_alpha(Colors::text_primary(), 0.92);

    // Ghost clip body — translucent track-colored fill with a pulsing outline so
    // it reads as "in creation". The pulse animates on its own frames, so it
    // stays alive even when the cursor is held still.
    let body = div()
        .absolute()
        .left(px(x_lo))
        .top(px(top))
        .w(px(width))
        .h(px(height))
        .rounded_md()
        .bg(ghost_fill)
        .border(px(1.0))
        .border_color(Colors::with_alpha(track_color, 0.85))
        .overflow_hidden()
        .flex()
        .flex_col()
        .justify_between()
        // Title placeholder.
        .child(
            div()
                .px(px(6.0))
                .pt(px(4.0))
                .text_size(px(9.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(label_text)
                .truncate()
                .child("New MIDI Clip"),
        )
        // Bottom length readout, mirroring the committed clip's label bar.
        .child(
            div()
                .h(px(14.0))
                .w_full()
                .bg(Colors::with_alpha(Colors::surface_panel_alt(), 0.85))
                .border_t(px(1.0))
                .border_color(Colors::divider())
                .px(px(6.0))
                .flex()
                .items_center()
                .justify_end()
                .text_size(px(8.0))
                .text_color(Colors::text_secondary())
                .child(format!("{:.1} bt", length)),
        )
        .with_animation(
            "pen-clip-draw-pulse",
            Animation::new(Duration::from_millis(1100))
                .repeat()
                .with_easing(pulsating_between(0.35, 0.85)),
            move |this, delta| this.border_color(Colors::with_alpha(track_color, delta)),
        );

    // Floating musical-length label, pinned just above the ghost clip (or below
    // it when the clip sits at the very top of the lane area).
    let label_below = top < 26.0;
    let label = div()
        .absolute()
        .left(px(x_lo + 2.0))
        .map(|el| {
            if label_below {
                el.top(px(top + height + 4.0))
            } else {
                el.top(px((top - 22.0).max(0.0)))
            }
        })
        .px(px(6.0))
        .py(px(2.0))
        .rounded_md()
        .bg(Colors::with_alpha(Colors::surface_panel(), 0.96))
        .border(px(1.0))
        .border_color(Colors::with_alpha(track_color, 0.6))
        .shadow_lg()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .text_size(px(9.0))
        .child(
            div()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(label_text)
                .child(length_label),
        )
        .child(div().text_color(Colors::text_muted()).child(range_label));

    // Subtle full-height guide at the clip end so the snapped end position reads
    // clearly against the grid.
    let end_x = state.beats_to_x(clip_end);
    let end_guide = div()
        .absolute()
        .left(px((end_x - 0.5).max(0.0)))
        .top_0()
        .bottom_0()
        .w(px(1.0))
        .bg(Colors::with_alpha(track_color, 0.45));

    Some(
        div()
            .absolute()
            .inset_0()
            .child(end_guide)
            .child(body)
            .child(label)
            .into_any_element(),
    )
}

pub(crate) fn arrangement_range_overlay(state: &TimelineState) -> Option<gpui::AnyElement> {
    let range = state.arrangement_range.as_ref()?;
    let (start_beat, end_beat) = range.as_f32_range();
    let (lo, hi) = normalize_range(start_beat, end_beat);
    let x_lo = state.beats_to_x(lo);
    let width = (state.beats_to_x(hi) - x_lo).max(1.0);

    // Vertical span follows the affected track ids; an empty set covers the
    // whole lane area (e.g. a horizontal-only time range).
    let (y_top, height) = {
        let mut min_idx = usize::MAX;
        let mut max_idx = 0usize;
        for (idx, track) in state.tracks.iter().enumerate() {
            if range.track_ids.iter().any(|id| id == &track.id) {
                min_idx = min_idx.min(idx);
                max_idx = max_idx.max(idx);
            }
        }
        if min_idx == usize::MAX {
            (0.0_f32, state.viewport.track_area_height.max(0.0))
        } else {
            let top = min_idx as f32 * TRACK_HEIGHT - state.viewport.scroll_y;
            let h = ((max_idx - min_idx + 1) as f32) * TRACK_HEIGHT;
            (top, h)
        }
    };

    Some(
        div()
            .absolute()
            .left(px(x_lo))
            .top(px(y_top))
            .w(px(width))
            .h(px(height))
            .bg(Colors::with_alpha(Colors::accent_primary(), 0.14))
            .border(px(1.0))
            .border_color(Colors::with_alpha(Colors::accent_primary(), 0.7))
            .into_any_element(),
    )
}

pub(crate) fn timeline_marker_region_overlay(state: &TimelineState) -> Option<gpui::AnyElement> {
    if state.markers.is_empty() && state.regions.is_empty() {
        return None;
    }

    let (visible_start, visible_end) = state.visible_beat_range(state.viewport.viewport_width);
    let body_height = state
        .viewport
        .track_area_height
        .max(state.viewport.viewport_height);
    let mut children: Vec<gpui::AnyElement> = Vec::new();

    for region in &state.regions {
        let (start, end) = region.normalized_range();
        if end < visible_start as f64 || start > visible_end as f64 {
            continue;
        }
        let x = state.beats_to_x(start as f32);
        let width = (state.beats_to_x(end as f32) - x).max(1.0);
        let color = crate::color::parse_hex_color(&region.color_hex)
            .unwrap_or_else(|_| Colors::accent_success());
        children.push(
            div()
                .absolute()
                .left(px(x))
                .top_0()
                .h(px(body_height))
                .w(px(width))
                .bg(Colors::with_alpha(color, 0.08))
                .border_l(px(1.0))
                .border_r(px(1.0))
                .border_color(Colors::with_alpha(color, 0.35))
                .into_any_element(),
        );
    }

    for marker in &state.markers {
        if marker.beat < visible_start as f64 || marker.beat > visible_end as f64 {
            continue;
        }
        let x = state.beats_to_x(marker.beat as f32);
        let color = crate::color::parse_hex_color(&marker.color_hex)
            .unwrap_or_else(|_| Colors::accent_primary());
        children.push(
            div()
                .absolute()
                .left(px(x))
                .top_0()
                .h(px(body_height))
                .w(px(1.0))
                .bg(Colors::with_alpha(color, 0.48))
                .into_any_element(),
        );
    }

    if children.is_empty() {
        return None;
    }

    Some(
        div()
            .absolute()
            .inset_0()
            .children(children)
            .into_any_element(),
    )
}
