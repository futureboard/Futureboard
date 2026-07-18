//! Split out of `piano_roll.rs` (god-file decomposition). These are
//! `impl PianoRoll` extension blocks; `use super::*` pulls in the shared
//! piano-roll vocabulary (struct fields via the type, consts, free fns).

use super::*;

impl PianoRoll {
    pub(super) fn cc_view_size(&self) -> (f32, f32) {
        match self.cc_bounds.get() {
            Some(b) => (
                f32::from(b.size.width).max(1.0),
                f32::from(b.size.height).max(1.0),
            ),
            None => (600.0, LANE_H),
        }
    }

    pub(super) fn cc_local(&self, window_pos: gpui::Point<Pixels>) -> Option<(f32, f32)> {
        let b = self.cc_bounds.get()?;
        let ox: f32 = b.origin.x.into();
        let oy: f32 = b.origin.y.into();
        let x: f32 = window_pos.x.into();
        let y: f32 = window_pos.y.into();
        Some((x - ox, y - oy))
    }

    /// Begin a CC paint (`erase = false`) or erase (`erase = true`) gesture:
    /// ensure the active lane, snapshot its points for undo, and apply the first
    /// edit at the cursor.
    pub(super) fn begin_cc_paint(
        &mut self,
        erase: bool,
        lx: f32,
        ly: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let kind = self.active_cc;
        self.timeline.update(cx, |tl, _| {
            tl.state.ensure_controller_lane(&clip_id, kind);
        });
        self.cc_edit_prev = Some(
            self.timeline
                .read(cx)
                .state
                .controller_points_snapshot(&clip_id, kind),
        );
        self.cc_edit_target = Some((clip_id.clone(), kind));
        self.drag = PianoDrag::CcPaint { erase };
        self.cc_paint_at(lx, ly, erase, cx);
        cx.notify();
    }

    /// Apply one CC edit at a local strip coordinate (live, not yet committed).
    pub(super) fn cc_paint_at(&mut self, lx: f32, ly: f32, erase: bool, cx: &mut Context<Self>) {
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let kind = self.active_cc;
        let beat = self.snap_beats(self.x_to_beat(lx));
        let (_, cc_h) = self.cc_view_size();
        let value = (1.0 - (ly / cc_h.max(1.0))).clamp(0.0, 1.0);
        self.drag_value_status = Some(format!(
            "{}: {}",
            cc_kind_label(kind),
            controller_display_value(kind, value)
        ));
        let tol = (self.step_beats() * 0.5).max(1.0e-3);
        self.timeline.update(cx, |tl, tcx| {
            if erase {
                tl.state
                    .delete_controller_points_near(&clip_id, kind, beat, tol);
            } else {
                tl.state.put_controller_point(&clip_id, kind, beat, value);
            }
            tcx.notify();
        });
    }

    /// Hit-test the active lane's points; return the id of one within ~6 px of
    /// the local strip coordinate.
    pub(super) fn cc_point_at(
        &self,
        cx: &Context<Self>,
        clip_id: &str,
        lx: f32,
        ly: f32,
    ) -> Option<u64> {
        let (_, cc_h) = self.cc_view_size();
        let kind = self.active_cc;
        let tl = self.timeline.read(cx);
        let points = tl.state.controller_lane_points(clip_id, kind)?;
        const R: f32 = 6.0;
        points.iter().find_map(|p| {
            let x = self.beat_to_x(p.beat);
            let y = Self::controller_y_for_value(p.value, cc_h);
            ((lx - x).abs() <= R && (ly - y).abs() <= R).then_some(p.id)
        })
    }

    /// Begin dragging an existing CC point (and any multi-selection that
    /// contains it). Ctrl/Cmd+click toggles selection without starting a drag
    /// when handled by the lane mouse-down path before this is called.
    pub(super) fn begin_cc_move(
        &mut self,
        id: u64,
        unsnap: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let kind = self.active_cc;
        if !self.cc_selection.contains(&id) {
            self.cc_selection = HashSet::from([id]);
        }
        let selected = self.cc_selection.clone();
        let prev: Vec<(u64, f32, f32)> = self
            .timeline
            .read(cx)
            .state
            .controller_lane_points(&clip_id, kind)
            .map(|pts| {
                pts.iter()
                    .filter(|p| selected.contains(&p.id))
                    .map(|p| (p.id, p.beat, p.value))
                    .collect()
            })
            .unwrap_or_default();
        let (anchor_beat, anchor_value) = prev
            .iter()
            .find(|(pid, _, _)| *pid == id)
            .map(|(_, b, v)| (*b, *v))
            .unwrap_or((0.0, 0.0));
        self.cc_edit_prev = Some(
            self.timeline
                .read(cx)
                .state
                .controller_points_snapshot(&clip_id, kind),
        );
        self.cc_edit_target = Some((clip_id.clone(), kind));
        let ids: Vec<u64> = prev.iter().map(|(pid, _, _)| *pid).collect();
        self.drag = PianoDrag::CcMove {
            ids,
            prev,
            anchor_beat,
            anchor_value,
            unsnap,
        };
        cx.notify();
    }

    /// Move every selected CC point by the same relative Δbeat/Δvalue from the
    /// grab anchor. Beat snaps unless Shift (`unsnap`) is held.
    pub(super) fn cc_move_selection_to(&mut self, lx: f32, ly: f32, cx: &mut Context<Self>) {
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let PianoDrag::CcMove {
            prev,
            anchor_beat,
            anchor_value,
            unsnap,
            ..
        } = &self.drag
        else {
            return;
        };
        let prev = prev.clone();
        let anchor_beat = *anchor_beat;
        let anchor_value = *anchor_value;
        let unsnap = *unsnap;
        let kind = self.active_cc;
        let cur_beat = self.snap_beats_live(self.x_to_beat(lx).max(0.0), unsnap);
        let (_, cc_h) = self.cc_view_size();
        let cur_value = (1.0 - (ly / cc_h.max(1.0))).clamp(0.0, 1.0);
        let d_beat = cur_beat - anchor_beat;
        let d_value = cur_value - anchor_value;
        let step = self.step_beats();
        self.drag_value_status = Some(if prev.len() == 1 {
            format!(
                "{}: {}",
                cc_kind_label(kind),
                controller_display_value(kind, cur_value)
            )
        } else {
            format!(
                "{} Δbeat {:+.2} · {} pts",
                cc_kind_label(kind),
                d_beat,
                prev.len()
            )
        });
        self.timeline.update(cx, |tl, tcx| {
            for (id, beat, value) in &prev {
                let raw = (*beat + d_beat).max(0.0);
                let next_beat = if unsnap || step <= 0.0 {
                    raw
                } else {
                    ((raw / step).round() * step).max(0.0)
                };
                let next_value = (*value + d_value).clamp(0.0, 1.0);
                tl.state
                    .set_controller_point(&clip_id, kind, *id, next_beat, next_value);
            }
            tcx.notify();
        });
    }

    /// Generate a shaped CC curve over the selected points' beat span (or one
    /// bar from the click beat when nothing is selected). Replaces points in
    /// that span; commits as one `SetControllerPoints` undo entry.
    pub(super) fn apply_cc_curve(
        &mut self,
        kind: CcCurveKind,
        click_beat: f32,
        cx: &mut Context<Self>,
    ) {
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let controller = self.active_cc;
        let step = self.step_beats().max(1.0e-3);
        let existing = self
            .timeline
            .read(cx)
            .state
            .controller_points_snapshot(&clip_id, controller);
        let selected: Vec<&MidiControllerPoint> = existing
            .iter()
            .filter(|p| self.cc_selection.contains(&p.id))
            .collect();
        let (lo_beat, hi_beat, from, to) = if selected.len() >= 2 {
            let lo = selected
                .iter()
                .map(|p| p.beat)
                .fold(f32::INFINITY, f32::min);
            let hi = selected.iter().map(|p| p.beat).fold(0.0_f32, f32::max);
            let from = selected
                .iter()
                .min_by(|a, b| {
                    a.beat
                        .partial_cmp(&b.beat)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|p| p.value)
                .unwrap_or(0.0);
            let to = selected
                .iter()
                .max_by(|a, b| {
                    a.beat
                        .partial_cmp(&b.beat)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|p| p.value)
                .unwrap_or(1.0);
            (lo, hi.max(lo + step), from, to)
        } else if selected.len() == 1 {
            let p = selected[0];
            (p.beat, p.beat + 4.0, p.value, 1.0 - p.value)
        } else {
            let start = self.snap_beats(click_beat.max(0.0));
            (start, start + 4.0, 0.0, 1.0)
        };

        // Humanize: jitter existing points in-span rather than regenerating.
        let prev = existing.clone();
        let mut points: Vec<MidiControllerPoint> = existing
            .into_iter()
            .filter(|p| p.beat < lo_beat - 1.0e-4 || p.beat > hi_beat + 1.0e-4)
            .collect();

        if kind == CcCurveKind::Humanize {
            for p in prev
                .iter()
                .filter(|p| p.beat >= lo_beat - 1.0e-4 && p.beat <= hi_beat + 1.0e-4)
            {
                let jitter = (CcCurveKind::Humanize.sample(p.beat.fract(), 0.0, 1.0) - 0.5) * 0.12;
                points.push(MidiControllerPoint::new(
                    p.beat,
                    (p.value + jitter).clamp(0.0, 1.0),
                ));
            }
        } else {
            let span = (hi_beat - lo_beat).max(step);
            let count = (span / step).round().max(1.0) as i32;
            for i in 0..=count {
                let beat = (lo_beat + step * i as f32).min(hi_beat);
                let t = if span <= 1.0e-6 {
                    0.0
                } else {
                    (beat - lo_beat) / span
                };
                let value = kind.sample(t, from, to);
                points.push(MidiControllerPoint::new(beat, value));
            }
        }

        self.cc_edit_prev = Some(prev);
        self.cc_edit_target = Some((clip_id.clone(), controller));
        self.timeline.update(cx, |tl, tcx| {
            tl.state
                .set_controller_lane_points(&clip_id, controller, points);
            tcx.notify();
        });
        self.commit_cc_edit(cx);
        self.open_cc_curve_menu = None;
        cx.notify();
    }

    /// Begin a Shift+drag ramp: snapshot the lane for undo and anchor the line
    /// at the cursor. The line is rebuilt on every move from the pre-drag points.
    pub(super) fn begin_cc_line(
        &mut self,
        lx: f32,
        ly: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let kind = self.active_cc;
        self.timeline.update(cx, |tl, _| {
            tl.state.ensure_controller_lane(&clip_id, kind);
        });
        self.cc_edit_prev = Some(
            self.timeline
                .read(cx)
                .state
                .controller_points_snapshot(&clip_id, kind),
        );
        self.cc_edit_target = Some((clip_id.clone(), kind));
        let anchor_beat = self.snap_beats(self.x_to_beat(lx)).max(0.0);
        let (_, cc_h) = self.cc_view_size();
        let anchor_value = (1.0 - (ly / cc_h.max(1.0))).clamp(0.0, 1.0);
        self.drag = PianoDrag::CcLine {
            anchor_beat,
            anchor_value,
        };
        self.cc_line_to(anchor_beat, anchor_value, lx, ly, cx);
        cx.notify();
    }

    /// Rebuild the ramp from `anchor` to the cursor: keep pre-drag points outside
    /// the spanned beat range, then lay evenly-spaced points (one per grid step)
    /// along the straight line between the two endpoints.
    pub(super) fn cc_line_to(
        &mut self,
        anchor_beat: f32,
        anchor_value: f32,
        lx: f32,
        ly: f32,
        cx: &mut Context<Self>,
    ) {
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let Some(base) = self.cc_edit_prev.clone() else {
            return;
        };
        let kind = self.active_cc;
        let cur_beat = self.snap_beats(self.x_to_beat(lx)).max(0.0);
        let (_, cc_h) = self.cc_view_size();
        let cur_value = (1.0 - (ly / cc_h.max(1.0))).clamp(0.0, 1.0);
        self.drag_value_status = Some(format!(
            "{} line: {}→{}",
            cc_kind_label(kind),
            controller_display_value(kind, anchor_value),
            controller_display_value(kind, cur_value)
        ));

        // Orient the span left-to-right and pair values with the same orientation.
        let (lo_beat, hi_beat, lo_val, hi_val) = if anchor_beat <= cur_beat {
            (anchor_beat, cur_beat, anchor_value, cur_value)
        } else {
            (cur_beat, anchor_beat, cur_value, anchor_value)
        };
        const EPS: f32 = 1.0e-4;
        let mut points: Vec<MidiControllerPoint> = base
            .into_iter()
            .filter(|p| p.beat < lo_beat - EPS || p.beat > hi_beat + EPS)
            .collect();

        let step = self.step_beats().max(1.0e-3);
        let span = (hi_beat - lo_beat).max(0.0);
        let count = (span / step).round().max(0.0) as i32;
        for i in 0..=count {
            let beat = (lo_beat + step * i as f32).min(hi_beat);
            let t = if span <= 1.0e-6 {
                0.0
            } else {
                (beat - lo_beat) / span
            };
            let value = (lo_val + (hi_val - lo_val) * t).clamp(0.0, 1.0);
            points.push(MidiControllerPoint::new(beat, value));
        }

        self.timeline.update(cx, |tl, tcx| {
            tl.state.set_controller_lane_points(&clip_id, kind, points);
            tcx.notify();
        });
    }

    /// Commit a finished CC gesture as one undoable command (skips no-ops).
    pub(super) fn commit_cc_edit(&mut self, cx: &mut Context<Self>) {
        let Some(prev) = self.cc_edit_prev.take() else {
            self.cc_edit_target = None;
            return;
        };
        let Some((clip_id, kind)) = self.cc_edit_target.take() else {
            return;
        };
        let next = self
            .timeline
            .read(cx)
            .state
            .controller_points_snapshot(&clip_id, kind);
        if prev == next {
            return;
        }
        self.timeline.update(cx, |tl, tcx| {
            tl.record_executed_command(
                EditCommand::SetControllerPoints {
                    clip_id,
                    kind,
                    prev,
                    next,
                },
                tcx,
            );
        });
        if self.midi_editor_sink {
            crate::components::midi_editor_window::midi_editor_debug("edit command committed");
        }
    }

    pub(super) fn controller_y_for_value(value: f32, lane_h: f32) -> f32 {
        (1.0 - value.clamp(0.0, 1.0)) * (lane_h - 10.0) + 5.0
    }

    pub(super) fn build_cc_line(&self, cx: &Context<Self>, clip_id: &str) -> gpui::AnyElement {
        let (view_w, cc_h) = self.cc_view_size();
        let kind = self.active_cc;
        let points = self
            .timeline
            .read(cx)
            .state
            .controller_lane_points(clip_id, kind)
            .cloned()
            .unwrap_or_default();
        let default_value = controller_default_value(kind);
        let baseline_y = Self::controller_y_for_value(default_value, cc_h);
        let num_cols = view_w.ceil().max(1.0) as usize;
        let mut samples = Vec::with_capacity(num_cols + 1);
        for col in 0..=num_cols {
            let beat = self.x_to_beat(col as f32);
            let value = evaluate_controller_points(&points, beat, default_value);
            samples.push(Self::controller_y_for_value(value, cc_h));
        }
        let line_color = Colors::accent_primary();
        let baseline_color = Colors::with_alpha(Colors::text_primary(), 0.10);
        canvas(
            |_b, _w, _cx| {},
            move |bounds: Bounds<Pixels>, (), window, _cx| {
                let baseline = Bounds::new(
                    bounds.origin + point(px(0.0), px(baseline_y)),
                    size(px(view_w), px(1.0)),
                );
                window.paint_quad(fill(baseline, baseline_color));
                for col in 0..num_cols {
                    let y0 = samples[col];
                    let y1 = samples[col + 1];
                    let top = y0.min(y1);
                    let h = (y0 - y1).abs().max(1.6);
                    let rect = Bounds::new(
                        bounds.origin + point(px(col as f32), px(top)),
                        size(px(1.0), px(h)),
                    );
                    window.paint_quad(fill(rect, line_color));
                }
            },
        )
        .absolute()
        .inset_0()
        .into_any_element()
    }

    pub(super) fn build_cc_points(
        &self,
        cx: &Context<Self>,
        clip_id: &str,
    ) -> Vec<gpui::AnyElement> {
        let (view_w, cc_h) = self.cc_view_size();
        let kind = self.active_cc;
        let pts: Vec<(u64, f32, f32)> = self
            .timeline
            .read(cx)
            .state
            .controller_lane_points(clip_id, kind)
            .map(|ps| ps.iter().map(|p| (p.id, p.beat, p.value)).collect())
            .unwrap_or_default();
        let accent = Colors::accent_primary();
        pts.into_iter()
            .filter_map(|(id, beat, value)| {
                let x = self.beat_to_x(beat);
                if x < -6.0 || x > view_w + 6.0 {
                    return None;
                }
                let y = Self::controller_y_for_value(value, cc_h);
                let selected = self.cc_selection.contains(&id);
                Some(
                    div()
                        .id(("pr-cc-point", id as usize))
                        .absolute()
                        .left(px(x - 5.0))
                        .top(px(y - 5.0))
                        .w(px(10.0))
                        .h(px(10.0))
                        .cursor(gpui::CursorStyle::PointingHand)
                        .hover(|s| s.bg(Colors::with_alpha(Colors::accent_primary(), 0.08)))
                        .child(
                            div()
                                .absolute()
                                .left(px(1.0))
                                .top(px(1.0))
                                .w(px(8.0))
                                .h(px(8.0))
                                .rounded(px(4.0))
                                .border(px(1.0))
                                .border_color(if selected {
                                    Colors::accent_primary()
                                } else {
                                    Colors::text_primary()
                                })
                                .bg(if selected {
                                    Colors::with_alpha(accent, 1.0)
                                } else {
                                    accent
                                })
                                .when(selected, |el| {
                                    el.shadow(vec![gpui::BoxShadow {
                                        color: Colors::with_alpha(accent, 0.45).into(),
                                        offset: gpui::point(px(0.0), px(0.0)),
                                        blur_radius: px(6.0),
                                        spread_radius: px(0.0),
                                        inset: false,
                                    }])
                                }),
                        )
                        .into_any_element(),
                )
            })
            .collect()
    }

    /// The CC strip (right column) plus its captured bounds + interaction.
    pub(super) fn render_cc_lane(
        &mut self,
        cx: &mut Context<Self>,
        clip_id: &str,
        start_beat: f32,
        end_beat: f32,
        bpb: f32,
    ) -> impl IntoElement {
        let grid = self.build_velocity_grid(start_beat, end_beat, bpb);
        let line = self.build_cc_line(cx, clip_id);
        let points = self.build_cc_points(cx, clip_id);
        let is_empty = points.is_empty();
        let value_chip_el = matches!(
            self.drag,
            PianoDrag::CcPaint { .. } | PianoDrag::CcMove { .. } | PianoDrag::CcLine { .. }
        )
        .then(|| value_chip(self.drag_value_status.as_deref().unwrap_or("CC"), 8.0, 8.0));
        let empty_state = is_empty.then(|| {
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(9.0))
                .text_color(Colors::text_faint())
                .child("Click to draw · Shift-drag line · Right-click curves")
        });
        let curve_menu = self.build_cc_curve_menu(cx);
        let cc_bounds = self.cc_bounds.clone();
        let canvas = canvas(
            move |bounds, _w, _cx| cc_bounds.set(Some(bounds)),
            |_b, _r, _w, _cx| {},
        )
        .absolute()
        .inset_0();
        div()
            .id("piano-cc")
            .h(px(LANE_H))
            .w_full()
            .relative()
            .overflow_hidden()
            .border_t(px(1.0))
            .border_color(Colors::panel_border())
            .bg(Colors::surface_panel_alt())
            .cursor(gpui::CursorStyle::Crosshair)
            .child(canvas)
            .children(grid)
            .child(line)
            .children(points)
            .children(empty_state)
            .children(value_chip_el)
            .children(curve_menu)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.open_cc_curve_menu = None;
                    if let Some((lx, ly)) = this.cc_local(ev.position) {
                        // The Line tool draws a ramp; Shift is retained as the
                        // established temporary line gesture from other tools.
                        if this.tool == PianoTool::Line || ev.modifiers.shift {
                            this.begin_cc_line(lx, ly, window, cx);
                            return;
                        }
                        // Grab an existing point to move it; Ctrl/Cmd toggles
                        // multi-selection. Empty click clears selection and paints.
                        if let Some(cid) = this.editing_clip_id(cx) {
                            if let Some(id) = this.cc_point_at(cx, &cid, lx, ly) {
                                let additive = ev.modifiers.control || ev.modifiers.platform;
                                if additive {
                                    if this.cc_selection.contains(&id) {
                                        this.cc_selection.remove(&id);
                                    } else {
                                        this.cc_selection.insert(id);
                                    }
                                    cx.notify();
                                    return;
                                }
                                this.begin_cc_move(id, ev.modifiers.shift, window, cx);
                                return;
                            }
                        }
                        this.cc_selection.clear();
                        this.begin_cc_paint(false, lx, ly, window, cx);
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    window.focus(&this.focus, cx);
                    // Alt+right keeps the legacy erase paint; plain right-click
                    // opens the CC curve context menu (controller lane only).
                    if ev.modifiers.alt {
                        if let Some((lx, ly)) = this.cc_local(ev.position) {
                            this.begin_cc_paint(true, lx, ly, window, cx);
                        }
                        return;
                    }
                    if let Some((lx, ly)) = this.cc_local(ev.position) {
                        this.open_cc_curve_menu = Some((lx, ly));
                        cx.notify();
                    }
                }),
            )
    }

    fn build_cc_curve_menu(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let (lx, ly) = self.open_cc_curve_menu?;
        let click_beat = self.x_to_beat(lx);
        let mut panel = div()
            .absolute()
            .left(px(lx.clamp(4.0, 240.0)))
            .top(px(ly.clamp(4.0, 40.0)))
            .w(px(132.0))
            .max_h(px(LANE_H - 8.0))
            .id("pr-cc-curve-menu")
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .p(px(3.0))
            .gap(px(1.0))
            .rounded(px(6.0))
            .bg(Colors::surface_card())
            .border(px(1.0))
            .border_color(Colors::border_subtle())
            .shadow_lg()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _window, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(7.0))
                    .py(px(3.0))
                    .text_size(px(9.0))
                    .text_color(Colors::text_muted())
                    .child("Generate Curve"),
            );
        for (i, kind) in CcCurveKind::ALL.iter().enumerate() {
            let kind = *kind;
            panel = panel.child(
                div()
                    .id(("pr-cc-curve", i))
                    .flex()
                    .items_center()
                    .h(px(18.0))
                    .px(px(7.0))
                    .rounded(px(4.0))
                    .text_size(px(10.0))
                    .text_color(Colors::text_secondary())
                    .hover(|s| s.bg(Colors::surface_hover()))
                    .cursor(gpui::CursorStyle::PointingHand)
                    .child(kind.label())
                    .on_click(cx.listener(move |this, _ev, _w, cx| {
                        cx.stop_propagation();
                        this.apply_cc_curve(kind, click_beat, cx);
                    })),
            );
        }
        Some(
            deferred(panel.into_any_element())
                .with_priority(PIANO_ROLL_MENU_PRIORITY)
                .into_any_element(),
        )
    }
}
