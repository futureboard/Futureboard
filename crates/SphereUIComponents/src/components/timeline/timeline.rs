use gpui::{div, px, svg, Context, ExternalPaths, InteractiveElement, IntoElement, ParentElement, Render, Styled, Window, StatefulInteractiveElement};
use crate::theme::Colors;
use crate::assets;
use crate::components::timeline::timeline_state::{TimelineState, TimelineTool, SnapDivision, TrackState, TrackType, ClipState, ClipType, MidiNoteState, HEADER_WIDTH, RULER_HEIGHT};
use crate::components::timeline::timeline_ruler::timeline_ruler;
use crate::components::timeline::track_list::track_list;
use crate::components::timeline::floating_tools_bar::floating_tools_bar;
use crate::components::timeline::waveform_cache;

/// Sidebar width to subtract when translating window-space x coordinates
/// into timeline content-space x. Kept in sync with `sidebar.rs`.
const SIDEBAR_WIDTH: f32 = 272.0;
/// App chrome (top titlebar/menu strip) — used to convert window-space y into
/// the timeline track area. Mirrors the value used by app_chrome.
const APP_CHROME_HEIGHT: f32 = 36.0;

fn is_supported_audio_ext(path: &std::path::Path) -> bool {
    matches!(
        path.extension().and_then(|s| s.to_str()).map(|s| s.to_ascii_lowercase()).as_deref(),
        Some("wav") | Some("mp3") | Some("flac") | Some("ogg")
    )
}

pub struct Timeline {
    pub state: TimelineState,
    /// Window-space position of the last drag-move event while files are
    /// being dragged. We need this because `on_drop::<ExternalPaths>` does
    /// not carry the drop position itself — gpui translates the submit into
    /// a synthetic MouseUp, so we have to remember the last cursor position
    /// observed during the drag.
    last_drag_position: Option<gpui::Point<gpui::Pixels>>,
}

impl Timeline {
    pub fn new() -> Self {
        Self {
            state: TimelineState::default(),
            last_drag_position: None,
        }
    }
}

impl Render for Timeline {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let on_select_track = cx.listener(|this, track_id: &String, _window, cx| {
            this.state.select_track(track_id);
            cx.notify();
        });

        let on_select_clip = cx.listener(|this, clip_id: &String, _window, cx| {
            this.state.select_clip(clip_id);
            cx.notify();
        });

        let on_toggle_mute = cx.listener(|this, track_id: &String, _window, cx| {
            this.state.toggle_track_mute(track_id);
            cx.notify();
        });

        let on_toggle_solo = cx.listener(|this, track_id: &String, _window, cx| {
            this.state.toggle_track_solo(track_id);
            cx.notify();
        });

        let on_toggle_arm = cx.listener(|this, track_id: &String, _window, cx| {
            this.state.toggle_track_arm(track_id);
            cx.notify();
        });

        let on_toggle_input = cx.listener(|this, track_id: &String, _window, cx| {
            this.state.toggle_track_input_monitor(track_id);
            cx.notify();
        });

        let on_delete_track = cx.listener(|this, track_id: &String, _window, cx| {
            this.state.tracks.retain(|t| t.id != *track_id);
            if this.state.selection.selected_track_id.as_ref() == Some(track_id) {
                this.state.selection.selected_track_id = None;
            }
            cx.notify();
        });

        let on_volume_change = cx.listener(|this, (track_id, volume): &(String, f32), _window, cx| {
            this.state.set_track_volume(track_id, *volume);
            cx.notify();
        });

        let on_pan_change = cx.listener(|this, (track_id, pan): &(String, f32), _window, cx| {
            this.state.set_track_pan(track_id, *pan);
            cx.notify();
        });

        let on_add_clip = cx.listener(|this, (track_id, beat): &(String, f32), _window, cx| {
            if let Some(t) = this.state.tracks.iter_mut().find(|t| t.id == *track_id) {
                let name = match t.track_type {
                    TrackType::Audio => "vocals_harmony_new.wav".to_string(),
                    _ => "midi_clip_new.mid".to_string(),
                };
                let duration = 4.0;
                let clip_type = match t.track_type {
                    TrackType::Audio => ClipType::Audio { file_id: "new-file".to_string(), source_path: None },
                    _ => ClipType::Midi {
                        notes: vec![
                            MidiNoteState { pitch: 60, start: 0.0, duration: 1.0 },
                            MidiNoteState { pitch: 64, start: 1.0, duration: 1.0 },
                            MidiNoteState { pitch: 67, start: 2.0, duration: 2.0 },
                        ],
                    },
                };
                let clip_id = format!("clip-{}-{}", t.clips.len() + 1, beat);
                t.clips.push(ClipState {
                    id: clip_id,
                    name,
                    start_beat: *beat,
                    duration_beats: duration,
                    offset_beats: 0.0,
                    gain: 1.0,
                    clip_type,
                    muted: false,
                });
            }
            cx.notify();
        });

        let on_add_track = cx.listener(|this, _: &(), _window, cx| {
            this.state.create_audio_track();
            cx.notify();
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
            let snapped_sec = this.state.snap_time(beats * this.state.seconds_per_beat());
            this.state.transport.playhead_beats = snapped_sec / this.state.seconds_per_beat();
            cx.notify();
        });

        let on_select_tool = cx.listener(|this, tool: &TimelineTool, _window, cx| {
            this.state.active_tool = *tool;
            cx.notify();
        });

        let on_zoom_in = cx.listener(|this, _: &(), _window, cx| {
            this.state.viewport.pixels_per_second = (this.state.viewport.pixels_per_second * 1.33).min(4000.0);
            cx.notify();
        });

        let on_zoom_out = cx.listener(|this, _: &(), _window, cx| {
            this.state.viewport.pixels_per_second = (this.state.viewport.pixels_per_second * 0.75).max(4.0);
            cx.notify();
        });

        // Wrap callbacks in std::sync::Arc to allow easy cloning when passing down to sub-elements
        let on_select_track: std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static> = std::sync::Arc::new(on_select_track);
        let on_select_clip: std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static> = std::sync::Arc::new(on_select_clip);
        let on_toggle_mute: std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static> = std::sync::Arc::new(on_toggle_mute);
        let on_toggle_solo: std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static> = std::sync::Arc::new(on_toggle_solo);
        let on_toggle_arm: std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static> = std::sync::Arc::new(on_toggle_arm);
        let on_toggle_input: std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static> = std::sync::Arc::new(on_toggle_input);
        let on_delete_track: std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static> = std::sync::Arc::new(on_delete_track);
        let on_volume_change: std::sync::Arc<dyn Fn(&(String, f32), &mut gpui::Window, &mut gpui::App) + 'static> = std::sync::Arc::new(on_volume_change);
        let _on_pan_change: std::sync::Arc<dyn Fn(&(String, f32), &mut gpui::Window, &mut gpui::App) + 'static> = std::sync::Arc::new(on_pan_change);
        let on_add_clip: std::sync::Arc<dyn Fn(&(String, f32), &mut gpui::Window, &mut gpui::App) + 'static> = std::sync::Arc::new(on_add_clip);
        let on_add_track: std::sync::Arc<dyn Fn(&(), &mut gpui::Window, &mut gpui::App) + 'static> = std::sync::Arc::new(on_add_track);
        let on_toggle_snap: std::sync::Arc<dyn Fn(&(), &mut gpui::Window, &mut gpui::App) + 'static> = std::sync::Arc::new(on_toggle_snap);
        let on_cycle_grid: std::sync::Arc<dyn Fn(&(), &mut gpui::Window, &mut gpui::App) + 'static> = std::sync::Arc::new(on_cycle_grid);
        let on_seek: std::sync::Arc<dyn Fn(&f32, &mut gpui::Window, &mut gpui::App) + 'static> = std::sync::Arc::new(on_seek);
        let on_select_tool: std::sync::Arc<dyn Fn(&TimelineTool, &mut gpui::Window, &mut gpui::App) + 'static> = std::sync::Arc::new(on_select_tool);
        let on_zoom_in: std::sync::Arc<dyn Fn(&(), &mut gpui::Window, &mut gpui::App) + 'static> = std::sync::Arc::new(on_zoom_in);
        let on_zoom_out: std::sync::Arc<dyn Fn(&(), &mut gpui::Window, &mut gpui::App) + 'static> = std::sync::Arc::new(on_zoom_out);

        let header_callbacks = crate::components::timeline::track_header::TrackHeaderCallbacks {
            on_select_track: on_select_track.clone(),
            on_toggle_mute: on_toggle_mute.clone(),
            on_toggle_solo: on_toggle_solo.clone(),
            on_toggle_arm: on_toggle_arm.clone(),
            on_toggle_input: on_toggle_input.clone(),
            on_delete_track: on_delete_track.clone(),
            on_volume_change: on_volume_change.clone(),
        };

        let state = &self.state;
        let on_zoom_in_btn = on_zoom_in.clone();
        let on_zoom_out_btn = on_zoom_out.clone();

        // ── Drag/drop import wiring ─────────────────────────────────────
        // Track the mouse position throughout an external file drag so that
        // when `on_drop` fires we can resolve the drop coordinates.
        let on_drag_track = cx.listener(|this, event: &gpui::DragMoveEvent<ExternalPaths>, _window, _cx| {
            this.last_drag_position = Some(event.event.position);
        });

        let on_files_dropped = cx.listener(|this, paths: &ExternalPaths, _window, cx| {
            let drop_pos = this.last_drag_position;
            let mut any_imported = false;
            // Multi-file drops: the first file lands at the cursor; subsequent
            // files always land on a brand-new track (forced via y past the end).
            let mut force_new_track = false;
            for path in paths.paths().iter() {
                if !is_supported_audio_ext(path) { continue; }

                // Decode (or pull from cache) — populates the path-keyed waveform
                // cache so the clip renders the real shape on next paint.
                let decoded = waveform_cache::decode_and_cache_file(path);
                let duration_seconds = decoded.as_ref().map(|p| p.duration_seconds).unwrap_or(0.0);
                let clip_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "Imported Audio".to_string());

                // Resolve drop coordinates relative to the track area.
                let (drop_x, drop_y) = match drop_pos {
                    Some(p) if !force_new_track => {
                        let x: f32 = p.x.into();
                        let y: f32 = p.y.into();
                        let lane_x = (x - SIDEBAR_WIDTH - HEADER_WIDTH).max(0.0);
                        let lane_y = (y - APP_CHROME_HEIGHT - RULER_HEIGHT).max(0.0);
                        (lane_x, lane_y)
                    }
                    // No drag tracking captured, or stacking a subsequent file:
                    // a y past the last track forces `import_audio_at` to make a new track.
                    _ => (0.0, 1.0e9_f32),
                };

                this.state.import_audio_at(
                    path.to_string_lossy().to_string(),
                    clip_name,
                    drop_x,
                    drop_y,
                    duration_seconds,
                );
                any_imported = true;
                force_new_track = true;
            }
            if any_imported {
                this.last_drag_position = None;
                cx.notify();
            }
        });

        div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .bg(Colors::surface_base())
            .relative()
            .on_drag_move::<ExternalPaths>(on_drag_track)
            .on_drop::<ExternalPaths>(on_files_dropped)
            // 1. Timeline Ruler
            .child(timeline_ruler(
                state,
                on_add_track.clone(),
                on_toggle_snap.clone(),
                on_cycle_grid.clone(),
                on_seek.clone(),
            ))
            // 2. Track List Scroll Area
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .child(
                        track_list(
                            state,
                            header_callbacks.clone(),
                            on_select_track.clone(),
                            on_select_clip.clone(),
                            on_add_clip.clone(),
                        )
                    )
            )
            // 3. Playhead Overlay (spanning both ruler and tracks)
            .child(
                div()
                    .absolute()
                    .left(px(crate::components::timeline::timeline_state::HEADER_WIDTH))
                    .right_0()
                    .top_0()
                    .bottom_0()
                    .child(crate::components::timeline::playhead::playhead(state))
            )
            // 3. Floating Tools Bar
            .child(
                div()
                    .absolute()
                    .bottom(px(16.0))
                    .left(px(16.0))
                    .child(floating_tools_bar(state.active_tool, on_select_tool.clone()))
            )
            // 4. Zoom Controls
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
                    .border_color(gpui::rgba(0xFFFFFF1A))
                    .bg(gpui::rgb(0x171b22))
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
                            .hover(|style| style.bg(gpui::rgba(0xFFFFFF0D)))
                            .on_click(move |_, window, cx| {
                                on_zoom_out_btn(&(), window, cx);
                            })
                            .child(
                                svg()
                                    .path(assets::ICON_MINUS_PATH)
                                    .w(px(12.0))
                                    .h(px(12.0))
                                    .text_color(Colors::text_secondary())
                            )
                    )
                    // Zoom readout label
                    .child(
                        div()
                            .px(px(4.0))
                            .text_size(px(9.0))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(Colors::text_muted())
                            .child(format!("{:.0} px/bt", state.viewport.pixels_per_second * state.seconds_per_beat()))
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
                            .hover(|style| style.bg(gpui::rgba(0xFFFFFF0D)))
                            .on_click(move |_, window, cx| {
                                on_zoom_in_btn(&(), window, cx);
                            })
                            .child(
                                svg()
                                    .path(assets::ICON_PLUS_PATH)
                                    .w(px(12.0))
                                    .h(px(12.0))
                                    .text_color(Colors::text_secondary())
                            )
                    )
            )
    }
}
