use crate::components::plugin_picker::STUB_PLUGIN_ID;
use crate::components::timeline::timeline_state::{
    self, ClipType, TimelineState, TrackState, TrackType,
};

use DAUx::types::{
    EngineClipAudioProcess, EngineClipSnapshot, EngineInsertSnapshot, EngineMidiClipSnapshot,
    EngineMidiNoteSnapshot, EngineProjectSnapshot, EngineRoutingSnapshot, EngineSendSnapshot,
    EngineTrackSnapshot,
};

/// Build the DAUx insert descriptors for one track's mixer insert chain
/// (Phase 2b). Only real, instantiable VST3 plugins are emitted as
/// `native-plugin` descriptors — DAUx then instantiates a
/// `Vst3RuntimeProcessor` on its worker and routes audio through it. The
/// documented stub (`STUB_PLUGIN_ID`) and any slot without a usable path are
/// skipped so the realtime runtime keeps no-op'ing on placeholders rather than
/// logging passthrough noise.
///
/// `enabled` mirrors the UI bypass flag (`!bypassed`), so toggling bypass in
/// the mixer changes the audio path on the next engine sync. This runs on the
/// UI thread inside snapshot construction — never the audio callback.
fn build_engine_inserts(track: &TrackState) -> Vec<EngineInsertSnapshot> {
    use crate::components::timeline::timeline_state::InsertPluginFormat;

    track
        .inserts
        .iter()
        .filter_map(|slot| {
            let plugin_id = slot.plugin_id.as_deref()?;
            // Skip the placeholder stub — it has no real processor.
            if plugin_id == STUB_PLUGIN_ID {
                return None;
            }
            // Only VST3 with a real module path is instantiable today.
            if slot.plugin_format != Some(InsertPluginFormat::Vst3) {
                return None;
            }
            let path = slot
                .plugin_path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .filter(|p| !p.trim().is_empty())?;

            let mut params: std::collections::HashMap<String, serde_json::Value> =
                std::collections::HashMap::new();
            params.insert("format".to_string(), serde_json::json!("VST3"));
            params.insert("modulePath".to_string(), serde_json::json!(path));
            params.insert("path".to_string(), serde_json::json!(path));
            params.insert("classId".to_string(), serde_json::json!(plugin_id));
            params.insert("class_id".to_string(), serde_json::json!(plugin_id));
            params.insert("pluginInstanceId".to_string(), serde_json::json!(slot.id));
            params.insert(
                "displayName".to_string(),
                serde_json::json!(slot.display_name),
            );

            Some(EngineInsertSnapshot {
                id: slot.id.clone(),
                kind: "native-plugin".to_string(),
                enabled: slot.enabled && !slot.bypassed,
                params,
            })
        })
        .collect()
}

/// Build the DAUx send descriptors for one track (Phase 3). Each send carries
/// a linear level (from `gain_db`) and its target Bus/Return track id; DAUx
/// accumulates the scaled signal into the target's receive buffer. Sends with
/// no target are skipped. Pre-fader is persisted but the runtime currently taps
/// post-fader only. Runs on the UI thread during snapshot construction.
fn build_engine_sends(track: &TrackState) -> Vec<EngineSendSnapshot> {
    track
        .sends
        .iter()
        .filter(|s| !s.target_track_id.trim().is_empty())
        .map(|s| EngineSendSnapshot {
            id: s.id.clone(),
            return_track_id: s.target_track_id.clone(),
            level: s.gain_linear(),
            enabled: s.enabled,
            pre_fader: s.pre_fader,
        })
        .collect()
}

pub(super) fn build_engine_project_snapshot(
    state: &TimelineState,
    sample_rate: u32,
) -> EngineProjectSnapshot {
    let mut tracks: Vec<EngineTrackSnapshot> = state
        .tracks
        .iter()
        .map(|track| EngineTrackSnapshot {
            id: track.id.clone(),
            track_type: track_type_name(track.track_type).to_string(),
            volume: volume_norm_to_linear(track.volume),
            pan: track.pan.clamp(-1.0, 1.0),
            muted: track.muted,
            solo: track.solo,
            armed: track.armed,
            preview_mode: "stereo".to_string(),
            output_track_id: None,
            inserts: build_engine_inserts(track),
            sends: build_engine_sends(track),
        })
        .collect();

    tracks.push(EngineTrackSnapshot {
        id: "master".to_string(),
        track_type: "master".to_string(),
        volume: volume_norm_to_linear(state.master.volume),
        pan: 0.0,
        muted: false,
        solo: false,
        armed: false,
        preview_mode: "stereo".to_string(),
        output_track_id: None,
        inserts: Vec::new(),
        sends: Vec::new(),
    });

    let clips = state
        .tracks
        .iter()
        .flat_map(|track| {
            track.clips.iter().filter_map(move |clip| {
                if clip.muted {
                    return None;
                }
                let ClipType::Audio {
                    file_id,
                    source_path: Some(source_path),
                } = &clip.clip_type
                else {
                    return None;
                };
                if source_path.trim().is_empty() {
                    return None;
                }

                Some(EngineClipSnapshot {
                    id: clip.id.clone(),
                    track_id: track.id.clone(),
                    asset_id: file_id.clone(),
                    media_path: Some(source_path.clone()),
                    start_beat: clip.start_beat.max(0.0) as f64,
                    duration_beats: clip.duration_beats.max(0.0) as f64,
                    offset_seconds: state.beats_to_seconds(clip.offset_beats.max(0.0)) as f64,
                    gain: clip.gain.clamp(0.0, 4.0),
                    fades: None,
                    audio_process: Some(EngineClipAudioProcess {
                        speed_ratio: 1.0,
                        pitch_semitones: 0.0,
                        preserve_pitch: false,
                        mode: "none".to_string(),
                        quality: "balanced".to_string(),
                    }),
                })
            })
        })
        .collect();

    // MIDI clips (Phase 2): notes stay clip-relative; the engine resolves them
    // to absolute beats/samples. Muted clips are skipped, matching audio clips.
    let midi_clips = state
        .tracks
        .iter()
        .flat_map(|track| {
            let track_id = track.id.clone();
            track.clips.iter().filter_map(move |clip| {
                if clip.muted {
                    return None;
                }
                let ClipType::Midi { notes } = &clip.clip_type else {
                    return None;
                };
                Some(EngineMidiClipSnapshot {
                    id: clip.id.clone(),
                    track_id: track_id.clone(),
                    start_beat: clip.start_beat.max(0.0) as f64,
                    length_beats: clip.duration_beats.max(0.0) as f64,
                    notes: notes
                        .iter()
                        .map(|n| EngineMidiNoteSnapshot {
                            id: n.id,
                            pitch: n.pitch.min(127),
                            start_beat: n.start.max(0.0) as f64,
                            length_beats: n.duration.max(0.0) as f64,
                            velocity: n.velocity.clamp(1, 127),
                            channel: 0,
                        })
                        .collect(),
                })
            })
        })
        .collect();

    EngineProjectSnapshot {
        project_id: "futureboard-native".to_string(),
        project_root: None,
        bpm: state.bpm.max(1.0) as f64,
        time_signature: [state.time_signature_num, state.time_signature_den],
        sample_rate: sample_rate.max(1),
        tracks,
        clips,
        midi_clips,
        routing: EngineRoutingSnapshot {
            master_output_device: None,
            sample_rate: sample_rate.max(1),
            buffer_size: 256,
        },
    }
}

pub(super) fn log_engine_sync_snapshot(
    snapshot: &EngineProjectSnapshot,
    dirty: bool,
    reason: &'static str,
) {
    let clips_with_path = snapshot
        .clips
        .iter()
        .filter(|clip| {
            clip.media_path
                .as_deref()
                .map(|path| !path.trim().is_empty())
                .unwrap_or(false)
        })
        .count();
    let insert_count: usize = snapshot.tracks.iter().map(|t| t.inserts.len()).sum();
    let midi_note_count: usize = snapshot.midi_clips.iter().map(|c| c.notes.len()).sum();
    eprintln!(
        "[engine-sync] reason={} tracks={} clips={} clips_with_path={} inserts={} midi_clips={} midi_notes={} dirty={}",
        reason,
        snapshot.tracks.len(),
        snapshot.clips.len(),
        clips_with_path,
        insert_count,
        snapshot.midi_clips.len(),
        midi_note_count,
        dirty
    );
    for track in &snapshot.tracks {
        for insert in &track.inserts {
            eprintln!(
                "[engine-sync] insert track={} id={} kind={} enabled={} path={}",
                track.id,
                insert.id,
                insert.kind,
                insert.enabled,
                insert
                    .params
                    .get("modulePath")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<none>")
            );
        }
    }
    for clip in &snapshot.clips {
        eprintln!(
            "[engine-sync] clip id={} track={} path={} start={:.3} duration={:.3}",
            clip.id,
            clip.track_id,
            clip.media_path.as_deref().unwrap_or("<none>"),
            clip.start_beat,
            clip.duration_beats
        );
    }
}

fn track_type_name(track_type: TrackType) -> &'static str {
    match track_type {
        TrackType::Audio => "audio",
        TrackType::Midi => "midi",
        TrackType::Instrument => "instrument",
        TrackType::Bus => "bus",
        TrackType::Return => "return",
        TrackType::Master => "master",
    }
}

pub(super) fn volume_norm_to_linear(norm: f32) -> f32 {
    let norm = norm.clamp(0.0, 1.0);
    if norm <= 0.001 {
        return 0.0;
    }
    let db = timeline_state::volume::norm_to_db(norm);
    if db <= timeline_state::volume::MIN_DB + 0.05 {
        0.0
    } else {
        10.0_f32.powf(db / 20.0).clamp(0.0, 2.0)
    }
}
