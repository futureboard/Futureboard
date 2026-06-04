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

use crate::components::inspector_debug;
use crate::components::panel::InspectorCallbacks;
use crate::components::plugin_picker::PluginInsertKind;
use crate::components::timeline::timeline_state::{
    TrackAudioFormat, TrackInputRouting, TrackMidiInputRouting, TrackOutputRouting,
};

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
type InsertPickerCb = Arc<dyn Fn(&(String, usize, bool), &mut Window, &mut App) + 'static>;
type ClipF32Cb = Arc<dyn Fn(&(String, f32), &mut Window, &mut App) + 'static>;

impl StudioLayout {
    pub(crate) fn build_inspector_callbacks(&self, owner: Entity<Self>) -> InspectorCallbacks {
        let audio_engine = self.audio_engine.clone();
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
            let _ = owner_vol.update(cx, |this, cx| {
                this.mark_dirty();
                this.push_mixer_snapshot_to_window(cx);
            });
            if let Some(engine) = audio_engine.as_ref() {
                let _ = engine.update_track_param(&id, "volume", volume_norm_to_linear(v) as f64);
            }
        });

        let audio_engine = self.audio_engine.clone();
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
            let _ = owner_pan.update(cx, |this, cx| {
                this.mark_dirty();
                this.push_mixer_snapshot_to_window(cx);
            });
            if let Some(engine) = audio_engine.as_ref() {
                let _ = engine.update_track_param(&id, "pan", v as f64);
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
        let on_open_insert_editor = self.open_insert_editor_cb(owner.clone());
        let on_set_clip_start = self.set_clip_start_cb(owner.clone());
        let on_set_clip_length = self.set_clip_length_cb(owner.clone());
        let on_open_clip_bottom_editor = self.open_clip_bottom_editor_cb(owner.clone());
        let on_open_clip_external_midi_editor =
            self.open_clip_external_midi_editor_cb(owner.clone());

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
                let _ = owner_color.update(cx, |this, cx| {
                    this.mark_dirty();
                    this.push_mixer_snapshot_to_window(cx);
                });
            }
        });

        InspectorCallbacks {
            on_volume,
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
            on_open_insert_editor,
            on_set_clip_start,
            on_set_clip_length,
            on_open_clip_bottom_editor,
            on_open_clip_external_midi_editor,
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
                let _ = owner.update(cx, |this, cx| {
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
                let _ = owner.update(cx, |this, cx| {
                    this.mark_dirty();
                    this.mark_engine_media_dirty();
                    this.schedule_audio_project_sync(cx, false, "inspector_clip_length");
                    cx.notify();
                });
            }
        })
    }

    fn open_clip_bottom_editor_cb(&self, owner: Entity<Self>) -> StrCb {
        Arc::new(move |clip_id: &String, _w, cx| {
            let clip_id = clip_id.clone();
            let _ = owner.update(cx, |this, cx| {
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
            let _ = owner.update(cx, |this, cx| {
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
                let _ = owner.update(cx, |this, cx| {
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
                });
            },
        )
    }

    fn remove_insert_cb(&self, owner: Entity<Self>) -> InsertPairCb {
        Arc::new(move |(track_id, insert_id): &(String, String), _w, cx| {
            let track_id = track_id.clone();
            let insert_id = insert_id.clone();
            let _ = owner.update(cx, |this, cx| {
                this.close_insert_editor(&track_id, &insert_id, cx);
                this.timeline.update(cx, |timeline, cx| {
                    timeline.state.remove_insert(&track_id, &insert_id);
                    cx.notify();
                });
                inspector_debug(&format!(
                    "insert remove track={track_id} insert={insert_id}"
                ));
                this.mark_dirty();
                this.engine_project_dirty = true;
                this.push_mixer_snapshot_to_window(cx);
                cx.notify();
            });
        })
    }

    fn toggle_insert_bypass_cb(&self, owner: Entity<Self>) -> InsertPairCb {
        Arc::new(move |(track_id, insert_id): &(String, String), _w, cx| {
            let track_id = track_id.clone();
            let insert_id = insert_id.clone();
            let _ = owner.update(cx, |this, cx| {
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
                this.engine_project_dirty = true;
                this.push_mixer_snapshot_to_window(cx);
                cx.notify();
            });
        })
    }

    fn toggle_insert_enabled_cb(&self, owner: Entity<Self>) -> InsertPairCb {
        Arc::new(move |(track_id, insert_id): &(String, String), _w, cx| {
            let track_id = track_id.clone();
            let insert_id = insert_id.clone();
            let _ = owner.update(cx, |this, cx| {
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
                this.engine_project_dirty = true;
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
                let _ = owner.update(cx, |this, cx| {
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
                        this.engine_project_dirty = true;
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
                let _ = owner.update(cx, |this, cx| {
                    inspector_debug(&format!(
                        "insert open_editor track={track_id} index={insert_index} insert={insert_id}"
                    ));
                    this.open_insert_editor(&track_id, insert_index, &insert_id, window, cx);
                });
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
                let _ = owner.update(cx, |this, _cx| {
                    this.project_switcher.current_project.is_dirty = true;
                    this.project_switcher.current_project.subtitle = "Unsaved changes".to_string();
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
                let _ = owner.update(cx, |this, cx| {
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
                    let _ = owner.update(cx, |this, cx| {
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
                    let _ = owner.update(cx, |this, _cx| {
                        this.project_switcher.current_project.is_dirty = true;
                        this.project_switcher.current_project.subtitle =
                            "Unsaved changes".to_string();
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
                let _ = owner.update(cx, |this, cx| {
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
        let audio_engine = self.audio_engine.clone();
        let timeline = self.timeline.clone();
        Arc::new(move |id: &String, _w, cx: &mut App| {
            let id = id.clone();
            let mut value = false;
            timeline.update(cx, |t, cx| {
                match kind {
                    TrackToggle::Mute => t.state.toggle_track_mute(&id),
                    TrackToggle::Solo => t.state.toggle_track_solo(&id),
                    TrackToggle::Arm => t.state.toggle_track_arm(&id),
                    TrackToggle::Input => t.state.cycle_track_input_monitor(&id),
                }
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
                cx.notify();
            });
            inspector_debug(&format!(
                "edit track {} track={id} new={value}",
                kind.label()
            ));
            let _ = owner.update(cx, |this, cx| {
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
