//! Inspector edit callbacks. Every mutation lands in the same `TimelineState`
//! the TrackHeader and Mixer read, so an Inspector edit is reflected
//! everywhere. Mirrors the structure of [`build_mixer_callbacks`].
//!
//! Dirty policy (see plan):
//! * name / color / mute / solo / arm / input-monitor → project dirty only.
//! * volume / pan → project dirty + one realtime `update_track_param`.
//! Selection-only changes never mark anything dirty.

use std::sync::Arc;

use gpui::{App, Entity, Window};

use crate::components::edit::EditCommand;
use crate::components::inspector_debug;
use crate::components::panel::{InspectorCallbacks, InspectorRoutingCombo};
use crate::components::plugin_picker::PluginInsertKind;
use crate::components::timeline::timeline_state::{
    clip_output_local_to_source_sample, AudioClipStretchState, TimelineState, TrackAudioFormat,
    TrackInputRouting, TrackMidiInputRouting, TrackOutputRouting, WarpMarker,
};
use crate::overlay::OverlayAnchor;

use super::engine_snapshot::volume_norm_to_linear;
use super::StudioLayout;

type StrCb = Arc<dyn Fn(&String, &mut Window, &mut App) + 'static>;
type StrF32Cb = Arc<dyn Fn(&(String, f32), &mut Window, &mut App) + 'static>;
type ColorCb = Arc<dyn Fn(&(String, gpui::Rgba), &mut Window, &mut App) + 'static>;
type InputRoutingCb = Arc<dyn Fn(&(String, TrackInputRouting), &mut Window, &mut App) + 'static>;
type OutputRoutingCb = Arc<dyn Fn(&(String, TrackOutputRouting), &mut Window, &mut App) + 'static>;
type AudioFormatCb = Arc<dyn Fn(&(String, TrackAudioFormat), &mut Window, &mut App) + 'static>;
type MidiInputCb = Arc<dyn Fn(&(String, TrackMidiInputRouting), &mut Window, &mut App) + 'static>;
type MidiChannelCb = Arc<dyn Fn(&(String, Option<u8>), &mut Window, &mut App) + 'static>;
type InsertPairCb = Arc<dyn Fn(&(String, String), &mut Window, &mut App) + 'static>;
type InsertOpenCb = Arc<dyn Fn(&(String, usize, String), &mut Window, &mut App) + 'static>;
type InsertMoveCb = Arc<dyn Fn(&(String, String, bool), &mut Window, &mut App) + 'static>;
type InsertReorderCb = Arc<dyn Fn(&(String, String, usize), &mut Window, &mut App) + 'static>;
type InsertPickerCb = Arc<dyn Fn(&(String, usize, bool), &mut Window, &mut App) + 'static>;
type ClipF32Cb = Arc<dyn Fn(&(String, f32), &mut Window, &mut App) + 'static>;
type ClipBoolCb = Arc<dyn Fn(&(String, bool), &mut Window, &mut App) + 'static>;
type ClipStretchCb = Arc<dyn Fn(&(String, AudioClipStretchState), &mut Window, &mut App) + 'static>;

fn clip_dsp_debug_enabled() -> bool {
    static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FLAG.get_or_init(|| std::env::var_os("FUTUREBOARD_CLIP_DSP_DEBUG").is_some())
}

fn stretch_commit_field(
    prev: &AudioClipStretchState,
    next: &AudioClipStretchState,
) -> &'static str {
    if (prev.stretch_ratio - next.stretch_ratio).abs() > f64::EPSILON {
        "ratio"
    } else if prev.mode != next.mode {
        "mode"
    } else if prev.algorithm != next.algorithm {
        "algorithm"
    } else if prev.preserve_pitch != next.preserve_pitch {
        "preserve_pitch"
    } else if (prev.pitch_shift_semitones - next.pitch_shift_semitones).abs() > f32::EPSILON {
        "pitch"
    } else if prev.reverse != next.reverse {
        "reverse"
    } else if prev.warp_markers.len() != next.warp_markers.len() {
        "warp_markers"
    } else {
        "other"
    }
}

impl StudioLayout {
    pub(crate) fn build_inspector_callbacks(&self, owner: Entity<Self>) -> InspectorCallbacks {
        let audio_engine = self.audio_bridge.engine.clone();
        let timeline_vol = self.timeline.clone();
        let owner_vol = owner.clone();
        let on_volume: StrF32Cb = Arc::new(move |(id, v): &(String, f32), _w, cx| {
            let id = id.clone();
            let v = *v;
            let old = timeline_vol
                .read(cx)
                .state
                .find_track(&id)
                .map(|t| t.volume)
                .unwrap_or(0.0);
            timeline_vol.update(cx, |t, cx| {
                t.state.set_track_volume(&id, v);
                cx.notify();
            });
            inspector_debug(&format!("edit track volume old={old:.3} new={v:.3}"));
            StudioLayout::defer_update(&owner_vol, cx, |this, cx| {
                this.mark_dirty();
                this.push_mixer_snapshot_to_window(cx);
            });
            if let Some(engine) = audio_engine.as_ref() {
                let _ = engine.update_track_param(&id, "volume", volume_norm_to_linear(v) as f64);
            }
        });

        let audio_engine = self.audio_bridge.engine.clone();
        let timeline_pan = self.timeline.clone();
        let owner_pan = owner.clone();
        let on_pan: StrF32Cb = Arc::new(move |(id, v): &(String, f32), _w, cx| {
            let id = id.clone();
            let v = *v;
            timeline_pan.update(cx, |t, cx| {
                t.state.set_track_pan(&id, v);
                cx.notify();
            });
            inspector_debug(&format!("edit track pan track={id} new={v:.3}"));
            StudioLayout::defer_update(&owner_pan, cx, |this, cx| {
                this.mark_dirty();
                this.push_mixer_snapshot_to_window(cx);
            });
            if let Some(engine) = audio_engine.as_ref() {
                let _ = engine.update_track_param(&id, "pan", v as f64);
            }
        });

        let timeline_auto = self.timeline.clone();
        let owner_auto = owner.clone();
        let on_toggle_volume_automation_read: StrCb = Arc::new(move |id: &String, _w, cx| {
            let id = id.clone();
            let changed = timeline_auto.update(cx, |t, cx| {
                let read = t
                    .state
                    .find_track(&id)
                    .map(|track| !track.volume_automation_read)
                    .unwrap_or(true);
                let changed = t.state.set_track_volume_automation_read(&id, read);
                if changed {
                    // Refresh the effective preview at the current playhead so the
                    // fader/readout update immediately on toggle.
                    let beat = t.state.transport.playhead_beats;
                    t.state.recompute_effective_volumes(beat, "point_edit");
                    cx.notify();
                }
                changed
            });
            if changed {
                inspector_debug(&format!("toggle volume automation read track={id}"));
                // The flag is not persisted (so the project is not marked dirty),
                // but the runtime must learn whether the volume lane should drive
                // audio: resync the engine snapshot once on toggle (a control
                // action, not a per-tick event) so playback honors read on/off,
                // and refresh the mixer view.
                StudioLayout::defer_update(&owner_auto, cx, |this, cx| {
                    this.audio_bridge.project_dirty = true;
                    this.schedule_audio_project_sync(cx, false, "inspector_volume_automation_read");
                    this.push_mixer_snapshot_to_window(cx);
                });
            }
        });

        let on_toggle_mute = self.track_toggle_cb(owner.clone(), TrackToggle::Mute);
        let on_toggle_solo = self.track_toggle_cb(owner.clone(), TrackToggle::Solo);
        let on_toggle_arm = self.track_toggle_cb(owner.clone(), TrackToggle::Arm);
        let on_toggle_input = self.track_toggle_cb(owner.clone(), TrackToggle::Input);
        let on_set_input_routing = self.input_routing_cb(owner.clone());
        let on_set_output_routing = self.output_routing_cb(owner.clone());
        let on_set_audio_format = self.audio_format_cb(owner.clone());
        let on_set_midi_input = self.midi_input_cb(owner.clone());
        let on_set_midi_channel = self.midi_channel_cb(owner.clone());
        let on_open_insert_picker = self.insert_picker_cb(owner.clone());
        let on_remove_insert = self.remove_insert_cb(owner.clone());
        let on_toggle_insert_bypass = self.toggle_insert_bypass_cb(owner.clone());
        let on_toggle_insert_enabled = self.toggle_insert_enabled_cb(owner.clone());
        let on_move_insert = self.move_insert_cb(owner.clone());
        let on_reorder_insert = self.reorder_insert_cb(owner.clone());
        let on_open_insert_editor = self.open_insert_editor_cb(owner.clone());
        let on_set_clip_start = self.set_clip_start_cb(owner.clone());
        let on_set_clip_length = self.set_clip_length_cb(owner.clone());
        let on_set_clip_gain = self.set_clip_gain_cb(owner.clone());
        let on_set_clip_muted = self.set_clip_muted_cb(owner.clone());
        let on_set_clip_stretch = self.set_clip_stretch_cb(owner.clone());
        let on_clip_warp_add_at_playhead = self.clip_warp_add_at_playhead_cb(owner.clone());
        let on_clip_warp_clear = self.clip_warp_clear_cb(owner.clone());
        let on_open_clip_bottom_editor = self.open_clip_bottom_editor_cb(owner.clone());
        let on_open_clip_external_midi_editor =
            self.open_clip_external_midi_editor_cb(owner.clone());

        let open_routing_combo = self.overlay.inspector_routing_combo;
        let owner_routing_combo = owner.clone();
        let on_toggle_routing_combo: Arc<
            dyn Fn(InspectorRoutingCombo, Option<OverlayAnchor>, &mut Window, &mut App) + 'static,
        > = Arc::new(
            move |combo: InspectorRoutingCombo, anchor: Option<OverlayAnchor>, _w, cx| {
                StudioLayout::defer_update(&owner_routing_combo, cx, move |this, cx| {
                    if this.overlay.inspector_routing_combo == Some(combo) {
                        this.overlay.inspector_routing_combo = None;
                        this.overlay.inspector_routing_combo_anchor = None;
                    } else {
                        this.overlay.inspector_routing_combo = Some(combo);
                        this.overlay.inspector_routing_combo_anchor = anchor;
                    }
                    cx.notify();
                });
            },
        );

        let timeline_color = self.timeline.clone();
        let owner_color = owner.clone();
        let on_set_color: ColorCb = Arc::new(move |(id, color): &(String, gpui::Rgba), _w, cx| {
            let id = id.clone();
            let color = *color;
            let changed = timeline_color.update(cx, |t, cx| {
                let changed = t.state.set_track_color(&id, color);
                if changed {
                    cx.notify();
                }
                changed
            });
            if changed {
                inspector_debug(&format!("edit track color track={id}"));
                StudioLayout::defer_update(&owner_color, cx, |this, cx| {
                    this.mark_dirty();
                    this.push_mixer_snapshot_to_window(cx);
                });
            }
        });

        InspectorCallbacks {
            on_volume,
            on_toggle_volume_automation_read,
            on_pan,
            on_toggle_mute,
            on_toggle_solo,
            on_toggle_arm,
            on_toggle_input,
            on_set_color,
            on_set_input_routing,
            on_set_output_routing,
            on_set_audio_format,
            on_set_midi_input,
            on_set_midi_channel,
            on_open_insert_picker,
            on_remove_insert,
            on_toggle_insert_bypass,
            on_toggle_insert_enabled,
            on_move_insert,
            on_reorder_insert,
            on_open_insert_editor,
            on_set_clip_start,
            on_set_clip_length,
            on_set_clip_gain,
            on_set_clip_muted,
            on_set_clip_stretch,
            on_clip_warp_add_at_playhead,
            on_clip_warp_clear,
            on_open_clip_bottom_editor,
            on_open_clip_external_midi_editor,
            open_routing_combo,
            on_toggle_routing_combo,
        }
    }

    fn set_clip_start_cb(&self, owner: Entity<Self>) -> ClipF32Cb {
        let timeline = self.timeline.clone();
        Arc::new(move |(clip_id, start): &(String, f32), _w, cx| {
            let clip_id = clip_id.clone();
            let start = *start;
            let old = timeline
                .read(cx)
                .state
                .find_clip(&clip_id)
                .map(|(_, clip)| clip.start_beat);
            let changed = timeline.update(cx, |t, cx| {
                let changed = t.state.set_clip_start(&clip_id, start);
                if changed {
                    cx.notify();
                }
                changed
            });
            if changed {
                inspector_debug(&format!(
                    "clip start clip={clip_id} old={old:?} new={start:.3}"
                ));
                StudioLayout::defer_update(&owner, cx, |this, cx| {
                    this.mark_dirty();
                    this.mark_engine_media_dirty();
                    this.schedule_audio_project_sync(cx, false, "inspector_clip_start");
                    cx.notify();
                });
            }
        })
    }

    fn set_clip_length_cb(&self, owner: Entity<Self>) -> ClipF32Cb {
        let timeline = self.timeline.clone();
        Arc::new(move |(clip_id, length): &(String, f32), _w, cx| {
            let clip_id = clip_id.clone();
            let length = *length;
            let old = timeline
                .read(cx)
                .state
                .find_clip(&clip_id)
                .map(|(_, clip)| clip.duration_beats);
            let changed = timeline.update(cx, |t, cx| {
                let changed = t.state.set_clip_length(&clip_id, length);
                if changed {
                    cx.notify();
                }
                changed
            });
            if changed {
                inspector_debug(&format!(
                    "clip length clip={clip_id} old={old:?} new={length:.3}"
                ));
                StudioLayout::defer_update(&owner, cx, |this, cx| {
                    this.mark_dirty();
                    this.mark_engine_media_dirty();
                    this.schedule_audio_project_sync(cx, false, "inspector_clip_length");
                    cx.notify();
                });
            }
        })
    }

    fn set_clip_gain_cb(&self, owner: Entity<Self>) -> ClipF32Cb {
        let timeline = self.timeline.clone();
        Arc::new(move |(clip_id, gain): &(String, f32), _w, cx| {
            let clip_id = clip_id.clone();
            let gain = *gain;
            let changed = timeline.update(cx, |t, cx| {
                let changed = t.state.set_clip_gain(&clip_id, gain);
                if changed {
                    cx.notify();
                }
                changed
            });
            if changed {
                inspector_debug(&format!("clip gain clip={clip_id} new={gain:.3}"));
                StudioLayout::defer_update(&owner, cx, |this, cx| {
                    this.mark_dirty();
                    this.mark_engine_media_dirty();
                    this.schedule_audio_project_sync(cx, false, "inspector_clip_gain");
                    cx.notify();
                });
            }
        })
    }

    fn set_clip_muted_cb(&self, owner: Entity<Self>) -> ClipBoolCb {
        let timeline = self.timeline.clone();
        Arc::new(move |(clip_id, muted): &(String, bool), _w, cx| {
            let clip_id = clip_id.clone();
            let muted = *muted;
            let changed = timeline.update(cx, |t, cx| {
                let changed = t.state.set_clip_muted(&clip_id, muted);
                if changed {
                    cx.notify();
                }
                changed
            });
            if changed {
                inspector_debug(&format!("clip muted clip={clip_id} muted={muted}"));
                StudioLayout::defer_update(&owner, cx, |this, cx| {
                    this.mark_dirty();
                    this.mark_engine_media_dirty();
                    this.schedule_audio_project_sync(cx, false, "inspector_clip_muted");
                    cx.notify();
                });
            }
        })
    }

    /// Apply a full replacement of a clip's stretch/pitch state as one undo
    /// entry. The inspector builds the mutated `AudioClipStretchState`; here we
    /// snapshot the previous value, apply, and record a reversible command.
    fn set_clip_stretch_cb(&self, owner: Entity<Self>) -> ClipStretchCb {
        let timeline = self.timeline.clone();
        Arc::new(
            move |(clip_id, next): &(String, AudioClipStretchState), _w, cx| {
                let clip_id = clip_id.clone();
                let next = next.clone();
                let changed = timeline.update(cx, |t, cx| {
                    let project_bpm = t.state.bpm as f64;
                    let Some(prev) = t.state.clip_stretch(&clip_id).cloned() else {
                        return false;
                    };
                    if prev == next {
                        return false;
                    }
                    let changed_field = stretch_commit_field(&prev, &next);
                    let prev_len = t.state.clip_duration_beats(&clip_id).unwrap_or(0.0);
                    // Couple the clip's timeline length to the time-stretch ratio
                    // so the visual, audible, and exported lengths stay equal
                    // (spec §10). Only the ratio component scales length; toggles
                    // like reverse/fade leave it unchanged.
                    let old_ratio = prev.effective_time_ratio(project_bpm);
                    let new_ratio = next.effective_time_ratio(project_bpm);
                    let next_len = if old_ratio > 1e-6 && (old_ratio - new_ratio).abs() > 1e-9 {
                        (prev_len as f64 * (new_ratio / old_ratio)) as f32
                    } else {
                        prev_len
                    };
                    if clip_dsp_debug_enabled() {
                        eprintln!(
                            "[clip-dsp][inspector-commit] clip_id={} field={} old_ratio={:.3} new_ratio={:.3} old_duration={:.3} new_duration={:.3} snapshot_rebuild=true speed_ratio={:.6} pitch_shift={:+.2} pitch_ratio={:.4} preserve_pitch={}",
                            clip_id,
                            changed_field,
                            old_ratio,
                            new_ratio,
                            prev_len,
                            next_len,
                            next.resample_speed_ratio(project_bpm),
                            next.pitch_shift_semitones,
                            AudioClipStretchState::pitch_ratio_from_semitones(
                                next.pitch_shift_semitones
                            ),
                            next.preserve_pitch,
                        );
                    }
                    t.state.set_clip_stretch(&clip_id, next.clone());
                    if (next_len - prev_len).abs() > 1e-4 {
                        t.state.set_clip_length(&clip_id, next_len);
                    }
                    t.record_executed_command(
                        EditCommand::SetClipStretch {
                            clip_id: clip_id.clone(),
                            prev,
                            next: next.clone(),
                            prev_duration_beats: prev_len,
                            next_duration_beats: next_len,
                        },
                        cx,
                    );
                    true
                });
                if changed {
                    inspector_debug(&format!(
                        "clip stretch clip={clip_id} mode={:?} ratio={:.3}",
                        next.mode, next.stretch_ratio
                    ));
                    StudioLayout::defer_update(&owner, cx, |this, cx| {
                        this.mark_dirty();
                        this.mark_engine_media_dirty();
                        this.schedule_audio_project_sync(cx, false, "inspector_clip_stretch");
                        cx.notify();
                    });
                }
            },
        )
    }

    /// Append a warp marker at the current playhead (clamped within the clip),
    /// mapping the timeline beat to a source-sample position across the active
    /// source window. Stored only — segment-warp playback is pending.
    fn clip_warp_add_at_playhead_cb(&self, owner: Entity<Self>) -> StrCb {
        let timeline = self.timeline.clone();
        Arc::new(move |clip_id: &String, _w, cx| {
            let clip_id = clip_id.clone();
            let changed = timeline.update(cx, |t, cx| {
                let playhead = t.state.transport.playhead_beats as f64;
                let Some((prev, start, dur)) = t.state.find_clip(&clip_id).map(|(_, c)| {
                    (
                        c.stretch.clone(),
                        c.start_beat as f64,
                        c.duration_beats as f64,
                    )
                }) else {
                    return false;
                };
                let clip_end = start + dur.max(0.0);
                if playhead < start || playhead > clip_end || dur <= 0.0 {
                    return false;
                }
                let local_frac = ((playhead - start) / dur.max(f64::EPSILON)).clamp(0.0, 1.0);
                let output_len = (prev.source_len_samples() as f64)
                    * prev.effective_time_ratio(t.state.bpm as f64);
                let source_sample = clip_output_local_to_source_sample(
                    local_frac * output_len,
                    prev.source_start_samples,
                    prev.source_end_samples,
                    prev.effective_time_ratio(t.state.bpm as f64),
                    prev.reverse,
                )
                .round() as u64;
                let id = prev.warp_markers.iter().map(|m| m.id).max().unwrap_or(0) + 1;
                let mut next = prev.clone();
                next.warp_markers.push(WarpMarker {
                    id,
                    source_sample,
                    timeline_beat: playhead,
                    locked: false,
                });
                next.warp_markers
                    .sort_by(|a, b| a.timeline_beat.total_cmp(&b.timeline_beat));
                next.dirty = true;
                let len = t.state.clip_duration_beats(&clip_id).unwrap_or(0.0);
                t.state.set_clip_stretch(&clip_id, next.clone());
                t.record_executed_command(
                    EditCommand::SetClipStretch {
                        clip_id: clip_id.clone(),
                        prev,
                        next,
                        prev_duration_beats: len,
                        next_duration_beats: len,
                    },
                    cx,
                );
                true
            });
            if changed {
                inspector_debug(&format!("clip warp add clip={clip_id}"));
                StudioLayout::defer_update(&owner, cx, |this, cx| {
                    this.mark_dirty();
                    this.mark_engine_media_dirty();
                    this.schedule_audio_project_sync(cx, false, "inspector_clip_warp_add");
                    cx.notify();
                });
            }
        })
    }

    /// Remove every warp marker from a clip (one undo entry).
    fn clip_warp_clear_cb(&self, owner: Entity<Self>) -> StrCb {
        let timeline = self.timeline.clone();
        Arc::new(move |clip_id: &String, _w, cx| {
            let clip_id = clip_id.clone();
            let changed = timeline.update(cx, |t, cx| {
                let Some(prev) = t.state.clip_stretch(&clip_id).cloned() else {
                    return false;
                };
                if prev.warp_markers.is_empty() {
                    return false;
                }
                let mut next = prev.clone();
                next.warp_markers.clear();
                next.dirty = true;
                let len = t.state.clip_duration_beats(&clip_id).unwrap_or(0.0);
                t.state.set_clip_stretch(&clip_id, next.clone());
                t.record_executed_command(
                    EditCommand::SetClipStretch {
                        clip_id: clip_id.clone(),
                        prev,
                        next,
                        prev_duration_beats: len,
                        next_duration_beats: len,
                    },
                    cx,
                );
                true
            });
            if changed {
                inspector_debug(&format!("clip warp clear clip={clip_id}"));
                StudioLayout::defer_update(&owner, cx, |this, cx| {
                    this.mark_dirty();
                    this.mark_engine_media_dirty();
                    this.schedule_audio_project_sync(cx, false, "inspector_clip_warp_clear");
                    cx.notify();
                });
            }
        })
    }

    fn open_clip_bottom_editor_cb(&self, owner: Entity<Self>) -> StrCb {
        Arc::new(move |clip_id: &String, _w, cx| {
            let clip_id = clip_id.clone();
            StudioLayout::defer_update(&owner, cx, move |this, cx| {
                let _ = this.timeline.update(cx, |timeline, cx| {
                    timeline.state.select_clip(&clip_id);
                    cx.notify();
                });
                inspector_debug(&format!("open_midi_bottom_editor clip={clip_id}"));
                this.open_midi_editor_bottom_panel(cx);
            });
        })
    }

    fn open_clip_external_midi_editor_cb(&self, owner: Entity<Self>) -> StrCb {
        Arc::new(move |clip_id: &String, window, cx| {
            let clip_id = clip_id.clone();
            let bounds = window.bounds();
            StudioLayout::defer_update(&owner, cx, move |this, cx| {
                let _ = this.timeline.update(cx, |timeline, cx| {
                    timeline.state.select_clip(&clip_id);
                    cx.notify();
                });
                inspector_debug(&format!("open_midi_editor clip={clip_id}"));
                this.open_midi_editor_external_window(Some(bounds), cx);
            });
        })
    }

    fn insert_picker_cb(&self, owner: Entity<Self>) -> InsertPickerCb {
        Arc::new(
            move |(track_id, slot_index, instrument): &(String, usize, bool), window, cx| {
                let track_id = track_id.clone();
                let slot_index = *slot_index;
                let desired_kind = if *instrument {
                    PluginInsertKind::Instrument
                } else {
                    PluginInsertKind::Effect
                };
                StudioLayout::defer_update_in_window(
                    &owner,
                    window,
                    cx,
                    move |this, window, cx| {
                        inspector_debug(&format!(
                        "insert picker track={track_id} slot={slot_index} kind={desired_kind:?}"
                    ));
                        this.open_insert_picker_for(
                            &track_id,
                            Some(slot_index),
                            desired_kind,
                            window,
                            cx,
                        );
                    },
                );
            },
        )
    }

    fn remove_insert_cb(&self, owner: Entity<Self>) -> InsertPairCb {
        Arc::new(move |(track_id, insert_id): &(String, String), _w, cx| {
            let track_id = track_id.clone();
            let insert_id = insert_id.clone();
            StudioLayout::defer_update(&owner, cx, move |this, cx| {
                // Full RemoveInstrumentPlugin lifecycle: close editor, unload the
                // bridge-host instance, remove the engine sink, drop the slot,
                // re-sync the engine, and assert the instance is gone everywhere.
                this.remove_insert_fully(&track_id, &insert_id, cx, "inspector_remove_insert");
                inspector_debug(&format!(
                    "insert remove track={track_id} insert={insert_id}"
                ));
                this.push_mixer_snapshot_to_window(cx);
                cx.notify();
            });
        })
    }

    fn toggle_insert_bypass_cb(&self, owner: Entity<Self>) -> InsertPairCb {
        Arc::new(move |(track_id, insert_id): &(String, String), _w, cx| {
            let track_id = track_id.clone();
            let insert_id = insert_id.clone();
            StudioLayout::defer_update(&owner, cx, move |this, cx| {
                let bypassed = this.timeline.update(cx, |timeline, cx| {
                    let bypassed = timeline
                        .state
                        .toggle_insert_bypass(&track_id, &insert_id)
                        .unwrap_or(false);
                    cx.notify();
                    bypassed
                });
                inspector_debug(&format!(
                    "insert bypass track={track_id} insert={insert_id} bypass={bypassed}"
                ));
                this.mark_dirty();
                this.audio_bridge.project_dirty = true;
                this.push_mixer_snapshot_to_window(cx);
                cx.notify();
            });
        })
    }

    fn toggle_insert_enabled_cb(&self, owner: Entity<Self>) -> InsertPairCb {
        Arc::new(move |(track_id, insert_id): &(String, String), _w, cx| {
            let track_id = track_id.clone();
            let insert_id = insert_id.clone();
            StudioLayout::defer_update(&owner, cx, move |this, cx| {
                let enabled = this.timeline.update(cx, |timeline, cx| {
                    let enabled = timeline
                        .state
                        .toggle_insert_enabled(&track_id, &insert_id)
                        .unwrap_or(false);
                    cx.notify();
                    enabled
                });
                inspector_debug(&format!(
                    "insert enabled track={track_id} insert={insert_id} enabled={enabled}"
                ));
                this.mark_dirty();
                this.audio_bridge.project_dirty = true;
                this.push_mixer_snapshot_to_window(cx);
                cx.notify();
            });
        })
    }

    fn move_insert_cb(&self, owner: Entity<Self>) -> InsertMoveCb {
        Arc::new(
            move |(track_id, insert_id, up): &(String, String, bool), _w, cx| {
                let track_id = track_id.clone();
                let insert_id = insert_id.clone();
                let up = *up;
                StudioLayout::defer_update(&owner, cx, move |this, cx| {
                    let moved = this.timeline.update(cx, |timeline, cx| {
                        let moved = timeline.state.move_insert(&track_id, &insert_id, up);
                        if moved {
                            cx.notify();
                        }
                        moved
                    });
                    if moved {
                        inspector_debug(&format!(
                            "insert move track={track_id} insert={insert_id} up={up}"
                        ));
                        this.mark_dirty();
                        this.audio_bridge.project_dirty = true;
                        this.push_mixer_snapshot_to_window(cx);
                        cx.notify();
                    }
                });
            },
        )
    }

    /// Drag-reorder commit. The drop handler supplies the dragged
    /// `plugin_instance_id` and the insertion gap; we snapshot the current id
    /// order, compute the new order, and apply it as a single
    /// [`EditCommand::ReorderFxSlot`] so one drag is one undo entry. The command
    /// only reorders existing slots (never recreates an instance), so bypass /
    /// preset / parameter / editor / automation state follow each instance. A
    /// forced project sync rebuilds the engine's chain order (DSP order == UI
    /// order); editor windows are keyed by instance id, so they stay attached.
    fn reorder_insert_cb(&self, owner: Entity<Self>) -> InsertReorderCb {
        Arc::new(
            move |(track_id, insert_id, insertion_index): &(String, String, usize), _w, cx| {
                let track_id = track_id.clone();
                let insert_id = insert_id.clone();
                let insertion_index = *insertion_index;
                StudioLayout::defer_update(&owner, cx, move |this, cx| {
                    let changed = this.timeline.update(cx, |timeline, cx| {
                        let before = timeline.state.insert_order(&track_id);
                        let after = TimelineState::reordered_insert_ids(
                            &before,
                            &insert_id,
                            insertion_index,
                        );
                        if before == after {
                            return false;
                        }
                        timeline.run_edit_command(
                            EditCommand::ReorderFxSlot {
                                track_id: track_id.clone(),
                                before_order: before,
                                after_order: after,
                            },
                            cx,
                        );
                        true
                    });
                    if changed {
                        inspector_debug(&format!(
                            "insert reorder track={track_id} insert={insert_id} gap={insertion_index}"
                        ));
                        this.mark_dirty();
                        this.audio_bridge.project_dirty = true;
                        this.schedule_audio_project_sync(cx, true, "inspector_reorder_insert");
                        this.push_mixer_snapshot_to_window(cx);
                        cx.notify();
                    }
                });
            },
        )
    }

    fn open_insert_editor_cb(&self, owner: Entity<Self>) -> InsertOpenCb {
        Arc::new(
            move |(track_id, insert_index, insert_id): &(String, usize, String), window, cx| {
                let track_id = track_id.clone();
                let insert_index = *insert_index;
                let insert_id = insert_id.clone();
                StudioLayout::defer_update_in_window(
                    &owner,
                    window,
                    cx,
                    move |this, window, cx| {
                        inspector_debug(&format!(
                        "insert open_editor track={track_id} index={insert_index} insert={insert_id}"
                    ));
                        this.open_insert_editor(&track_id, insert_index, &insert_id, window, cx);
                    },
                );
            },
        )
    }

    fn input_routing_cb(&self, owner: Entity<Self>) -> InputRoutingCb {
        let timeline = self.timeline.clone();
        Arc::new(move |(id, input): &(String, TrackInputRouting), _w, cx| {
            let id = id.clone();
            let input = input.clone();
            let old = timeline
                .read(cx)
                .state
                .find_track(&id)
                .map(|track| track.routing.input.clone());
            let changed = timeline.update(cx, |t, cx| {
                let changed = t.state.set_track_input_routing(&id, input.clone());
                if changed {
                    cx.notify();
                }
                changed
            });
            if changed {
                inspector_debug(&format!(
                    "routing input track={id} old={:?} new={:?}",
                    old, input
                ));
                StudioLayout::defer_update(&owner, cx, |this, cx| {
                    this.mark_dirty();
                    cx.notify();
                });
            }
        })
    }

    fn output_routing_cb(&self, owner: Entity<Self>) -> OutputRoutingCb {
        let timeline = self.timeline.clone();
        Arc::new(move |(id, output): &(String, TrackOutputRouting), _w, cx| {
            let id = id.clone();
            let output = output.clone();
            let old = timeline
                .read(cx)
                .state
                .find_track(&id)
                .map(|track| track.routing.output.clone());
            let changed = timeline.update(cx, |t, cx| {
                let changed = t.state.set_track_output_routing(&id, output.clone());
                if changed {
                    cx.notify();
                }
                changed
            });
            if changed {
                inspector_debug(&format!(
                    "routing output track={id} old={:?} new={:?}",
                    old, output
                ));
                StudioLayout::defer_update(&owner, cx, |this, cx| {
                    this.mark_dirty();
                    this.push_mixer_snapshot_to_window(cx);
                });
            }
        })
    }

    fn audio_format_cb(&self, owner: Entity<Self>) -> AudioFormatCb {
        let timeline = self.timeline.clone();
        Arc::new(
            move |(id, audio_format): &(String, TrackAudioFormat), _w, cx| {
                let id = id.clone();
                let audio_format = *audio_format;
                let old = timeline
                    .read(cx)
                    .state
                    .find_track(&id)
                    .map(|track| track.routing.audio_format);
                let changed = timeline.update(cx, |t, cx| {
                    let changed = t.state.set_track_audio_format(&id, audio_format);
                    if changed {
                        cx.notify();
                    }
                    changed
                });
                if changed {
                    inspector_debug(&format!(
                        "routing audio_format track={id} old={:?} new={:?}",
                        old, audio_format
                    ));
                    StudioLayout::defer_update(&owner, cx, |this, cx| {
                        this.mark_dirty();
                        this.push_mixer_snapshot_to_window(cx);
                    });
                }
            },
        )
    }

    fn midi_input_cb(&self, owner: Entity<Self>) -> MidiInputCb {
        let timeline = self.timeline.clone();
        Arc::new(
            move |(id, midi_input): &(String, TrackMidiInputRouting), _w, cx| {
                let id = id.clone();
                let midi_input = midi_input.clone();
                let old = timeline
                    .read(cx)
                    .state
                    .find_track(&id)
                    .map(|track| track.routing.midi_input.clone());
                let changed = timeline.update(cx, |t, cx| {
                    let changed = t.state.set_track_midi_input(&id, midi_input.clone());
                    if changed {
                        cx.notify();
                    }
                    changed
                });
                if changed {
                    inspector_debug(&format!(
                        "routing midi_input track={id} old={:?} new={:?}",
                        old, midi_input
                    ));
                    StudioLayout::defer_update(&owner, cx, |this, cx| {
                        this.mark_dirty();
                        cx.notify();
                    });
                }
            },
        )
    }

    fn midi_channel_cb(&self, owner: Entity<Self>) -> MidiChannelCb {
        let timeline = self.timeline.clone();
        Arc::new(move |(id, channel): &(String, Option<u8>), _w, cx| {
            let id = id.clone();
            let channel = *channel;
            let old = timeline
                .read(cx)
                .state
                .find_track(&id)
                .map(|track| track.routing.midi_channel);
            let changed = timeline.update(cx, |t, cx| {
                let changed = t.state.set_track_midi_channel(&id, channel);
                if changed {
                    cx.notify();
                }
                changed
            });
            if changed {
                inspector_debug(&format!(
                    "routing midi_channel track={id} old={:?} new={:?}",
                    old, channel
                ));
                StudioLayout::defer_update(&owner, cx, |this, cx| {
                    this.mark_dirty();
                    this.push_mixer_snapshot_to_window(cx);
                });
            }
        })
    }

    /// Build one of the four M/S/R/I toggle callbacks. They share the engine /
    /// dirty / mixer-resync plumbing; only the state mutation + realtime param
    /// differ. Input-monitor has no realtime param (UI-only for now).
    fn track_toggle_cb(&self, owner: Entity<Self>, kind: TrackToggle) -> StrCb {
        let audio_engine = self.audio_bridge.engine.clone();
        let timeline = self.timeline.clone();
        Arc::new(move |id: &String, _w, cx: &mut App| {
            let id = id.clone();
            let mut value = false;
            let changed = timeline.update(cx, |t, cx| {
                let changed = match kind {
                    TrackToggle::Mute => t.state.toggle_track_mute(&id),
                    TrackToggle::Solo => t.state.toggle_track_solo(&id),
                    TrackToggle::Arm => t.state.toggle_track_arm(&id),
                    TrackToggle::Input => t.state.cycle_track_input_monitor(&id),
                };
                value = t
                    .state
                    .find_track(&id)
                    .map(|track| match kind {
                        TrackToggle::Mute => track.muted,
                        TrackToggle::Solo => track.solo,
                        TrackToggle::Arm => track.armed,
                        TrackToggle::Input => track.input_monitor.is_active(track.armed),
                    })
                    .unwrap_or(false);
                if changed {
                    cx.notify();
                }
                changed
            });
            if !changed {
                return;
            }
            inspector_debug(&format!(
                "edit track {} track={id} new={value}",
                kind.label()
            ));
            match kind {
                TrackToggle::Arm => {
                    eprintln!("[GPUI] SetTrackRecordArm track={id} armed={value}")
                }
                TrackToggle::Input => {
                    eprintln!("[GPUI] SetTrackMonitor track={id} enabled={value}")
                }
                _ => {}
            }
            StudioLayout::defer_update(&owner, cx, |this, cx| {
                this.mark_dirty();
                this.push_mixer_snapshot_to_window(cx);
            });
            if let Some(engine) = audio_engine.as_ref() {
                let param = match kind {
                    TrackToggle::Mute => Some("mute"),
                    TrackToggle::Solo => Some("solo"),
                    TrackToggle::Arm | TrackToggle::Input => None,
                };
                if let Some(param) = param {
                    let _ = engine.update_track_param(&id, param, if value { 1.0 } else { 0.0 });
                }
            }
        })
    }
}

#[derive(Clone, Copy)]
enum TrackToggle {
    Mute,
    Solo,
    Arm,
    Input,
}

impl TrackToggle {
    fn label(self) -> &'static str {
        match self {
            TrackToggle::Mute => "mute",
            TrackToggle::Solo => "solo",
            TrackToggle::Arm => "arm",
            TrackToggle::Input => "input-monitor",
        }
    }
}
