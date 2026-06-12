use super::*;

#[derive(Debug, Clone, PartialEq)]
pub enum ClipType {
    Audio {
        file_id: String,
        /// Absolute path to the decoded source file, if this clip was created
        /// by importing a real audio file. Used as the waveform cache key.
        source_path: Option<String>,
    },
    Midi {
        notes: Vec<MidiNoteState>,
        /// MIDI controller (CC / pitch-bend / pressure) lanes for this clip.
        controller_lanes: Vec<MidiControllerLane>,
    },
}

/// Background import/decode state for a real audio file (waveform + engine).
#[derive(Debug, Clone, PartialEq)]
pub enum AudioImportState {
    Pending,
    Probing,
    Decoding { progress: f32 },
    GeneratingPeaks { progress: f32 },
    Ready,
    Failed { message: String },
}

impl Default for AudioImportState {
    fn default() -> Self {
        Self::Pending
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClipState {
    pub id: String,
    pub name: String,
    pub start_beat: f32,
    pub duration_beats: f32,
    pub source_duration_seconds: Option<f64>,
    pub offset_beats: f32,
    pub gain: f32,
    pub clip_type: ClipType,
    pub muted: bool,
    /// Populated for imported audio clips; drives clip chrome + waveform UI.
    pub audio_import: AudioImportState,
}

impl ClipState {
    /// Stable key for an imported audio clip's waveform peaks and import state.
    ///
    /// Keyed on the asset id (`file_id`), **not** the on-disk path, so the
    /// waveform binding survives a later change of `source_path` (e.g. copying
    /// the source into the project folder). Returns `None` for clips with no
    /// real source (placeholder / live-recording preview).
    pub fn audio_asset_key(&self) -> Option<&str> {
        match &self.clip_type {
            ClipType::Audio {
                file_id,
                source_path: Some(_),
            } if !file_id.is_empty() => Some(file_id.as_str()),
            _ => None,
        }
    }
}

/// Which edge of a clip an edge-resize gesture is dragging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipEdge {
    Left,
    Right,
}

/// Whether a clip's anchor is stored in beats or wall-clock time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClipTimebase {
    #[default]
    Musical,
    Absolute,
}

impl TimelineState {
    pub fn next_clip_id(&self) -> String {
        let mut n = 0u32;
        for t in &self.tracks {
            for c in &t.clips {
                if let Some(rest) = c.id.strip_prefix("clip-") {
                    if let Ok(v) = rest.parse::<u32>() {
                        if v > n {
                            n = v;
                        }
                    }
                }
            }
        }
        format!("clip-{}", n + 1)
    }

    /// Length of a clip in beats, if it exists.
    pub fn clip_duration_beats(&self, clip_id: &str) -> Option<f32> {
        for track in &self.tracks {
            if let Some(clip) = track.clips.iter().find(|c| c.id == clip_id) {
                return Some(clip.duration_beats);
            }
        }
        None
    }

    /// Clips intersecting a beat range on any track.
    pub fn clips_intersecting_beats(&self, start: f32, end: f32) -> Vec<String> {
        let (lo, hi) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        let mut ids = Vec::new();
        for track in &self.tracks {
            for clip in &track.clips {
                let clip_end = clip.start_beat + clip.duration_beats;
                if clip.start_beat < hi && clip_end > lo {
                    ids.push(clip.id.clone());
                }
            }
        }
        ids
    }

    pub fn rename_clip(&mut self, clip_id: &str, name: &str) -> bool {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return false;
        }
        for track in &mut self.tracks {
            if let Some(clip) = track.clips.iter_mut().find(|clip| clip.id == clip_id) {
                if clip.name != trimmed {
                    clip.name = trimmed.to_string();
                    return true;
                }
                return false;
            }
        }
        false
    }

    pub fn set_clip_start(&mut self, clip_id: &str, start_beat: f32) -> bool {
        let start_beat = start_beat.max(0.0);
        for track in &mut self.tracks {
            if let Some(clip) = track.clips.iter_mut().find(|clip| clip.id == clip_id) {
                if (clip.start_beat - start_beat).abs() > 0.0001 {
                    clip.start_beat = start_beat;
                    return true;
                }
                return false;
            }
        }
        false
    }

    pub fn set_clip_length(&mut self, clip_id: &str, duration_beats: f32) -> bool {
        for track in &mut self.tracks {
            if let Some(clip) = track.clips.iter_mut().find(|clip| clip.id == clip_id) {
                let min_len = match &clip.clip_type {
                    ClipType::Midi { notes, .. } => {
                        let last_note_end = notes
                            .iter()
                            .map(|note| note.start.max(0.0) + note.duration.max(MIN_NOTE_BEATS))
                            .fold(0.0_f32, f32::max);
                        MIN_MIDI_CLIP_BEATS.max(last_note_end)
                    }
                    ClipType::Audio { .. } => 0.25,
                };
                let duration_beats = duration_beats.max(min_len);
                if (clip.duration_beats - duration_beats).abs() > 0.0001 {
                    clip.duration_beats = duration_beats;
                    return true;
                }
                return false;
            }
        }
        false
    }

    pub fn set_clip_muted(&mut self, clip_id: &str, muted: bool) -> bool {
        for track in &mut self.tracks {
            if let Some(clip) = track.clips.iter_mut().find(|clip| clip.id == clip_id) {
                if clip.muted != muted {
                    clip.muted = muted;
                    return true;
                }
                return false;
            }
        }
        false
    }

    pub fn set_clip_gain(&mut self, clip_id: &str, gain: f32) -> bool {
        let gain = gain.clamp(0.0, 4.0);
        for track in &mut self.tracks {
            if let Some(clip) = track.clips.iter_mut().find(|clip| clip.id == clip_id) {
                if (clip.gain - gain).abs() > 0.0001 {
                    clip.gain = gain;
                    return true;
                }
                return false;
            }
        }
        false
    }

    pub fn find_clip(&self, clip_id: &str) -> Option<(&TrackState, &ClipState)> {
        for t in &self.tracks {
            if let Some(c) = t.clips.iter().find(|c| c.id == clip_id) {
                return Some((t, c));
            }
        }
        None
    }

    pub fn delete_clip(&mut self, clip_id: &str) {
        for track in &mut self.tracks {
            if let Some(index) = track.clips.iter().position(|clip| clip.id == clip_id) {
                track.clips.remove(index);
                self.selection.selected_clip_ids.retain(|id| id != clip_id);
                self.selection.selected_track_id = Some(track.id.clone());
                return;
            }
        }
    }

    pub fn duplicate_clip(&mut self, clip_id: &str) {
        let Some((track_id, duplicate)) = self.build_clip_duplicate_after(clip_id) else {
            return;
        };
        let duplicate_id = duplicate.id.clone();
        if let Some(track) = self.tracks.iter_mut().find(|track| track.id == track_id) {
            if let Some(index) = track.clips.iter().position(|clip| clip.id == clip_id) {
                track.clips.insert(index + 1, duplicate);
            } else {
                track.clips.push(duplicate);
            }
            self.selection.selected_track_id = Some(track.id.clone());
            self.selection.selected_clip_ids = vec![duplicate_id];
        }
    }

    pub fn build_clip_duplicate_after(&self, clip_id: &str) -> Option<(String, ClipState)> {
        let snap_step = if self.snap_to_grid && self.grid_division != SnapDivision::Off {
            Some((self.grid_division.step_beats(self.beats_per_bar())).max(0.0))
        } else {
            None
        };
        for track in &self.tracks {
            if let Some(clip) = track.clips.iter().find(|clip| clip.id == clip_id) {
                let raw_start = clip.start_beat + clip.duration_beats;
                let start_beat = snap_step
                    .filter(|step| *step > 0.0)
                    .map(|step| (raw_start / step).round() * step)
                    .unwrap_or(raw_start)
                    .max(0.0);
                let duplicate = self.clone_clip_for_insert(
                    clip,
                    self.next_clip_id(),
                    format!("{} Copy", clip.name),
                    start_beat,
                );
                return Some((track.id.clone(), duplicate));
            }
        }
        None
    }

    pub fn clone_clip_for_insert(
        &self,
        clip: &ClipState,
        id: String,
        name: String,
        start_beat: f32,
    ) -> ClipState {
        let mut cloned = clip.clone();
        cloned.id = id;
        cloned.name = name;
        cloned.start_beat = start_beat.max(0.0);
        cloned.clip_type = match &clip.clip_type {
            ClipType::Audio {
                file_id,
                source_path,
            } => ClipType::Audio {
                file_id: file_id.clone(),
                source_path: source_path.clone(),
            },
            ClipType::Midi {
                notes,
                controller_lanes,
            } => ClipType::Midi {
                notes: notes
                    .iter()
                    .map(|note| {
                        let mut cloned = MidiNoteState::new(
                            note.pitch,
                            note.start,
                            note.duration,
                            note.velocity,
                        );
                        cloned.muted = note.muted;
                        cloned
                    })
                    .collect(),
                controller_lanes: controller_lanes
                    .iter()
                    .map(|lane| MidiControllerLane {
                        kind: lane.kind,
                        points: lane
                            .points
                            .iter()
                            .map(|point| MidiControllerPoint::new(point.beat, point.value))
                            .collect(),
                        visible: lane.visible,
                        height: lane.height,
                        collapsed: lane.collapsed,
                    })
                    .collect(),
            },
        };
        cloned
    }

    pub fn move_clip_to_track(&mut self, clip_id: &str, target_track_id: &str, start_beat: f32) {
        let start_beat = self.snap_beats(start_beat).max(0.0);
        let mut moved_clip = None;
        let mut source_track_id = None;

        for track in &mut self.tracks {
            if let Some(index) = track.clips.iter().position(|clip| clip.id == clip_id) {
                let mut clip = track.clips.remove(index);
                clip.start_beat = start_beat;
                moved_clip = Some(clip);
                source_track_id = Some(track.id.clone());
                break;
            }
        }

        let Some(clip) = moved_clip else {
            return;
        };

        let target_id = if self.tracks.iter().any(|track| track.id == target_track_id) {
            target_track_id.to_string()
        } else {
            source_track_id.unwrap_or_else(|| target_track_id.to_string())
        };

        if let Some(track) = self.tracks.iter_mut().find(|track| track.id == target_id) {
            track.clips.push(clip);
            self.selection.selected_track_id = Some(track.id.clone());
            self.selection.selected_clip_ids = vec![clip_id.to_string()];
        }
    }

    /// Resize a clip by dragging one edge to `new_edge_beat` (absolute beats;
    /// snapped here). The opposite edge stays fixed. Enforces a minimum length
    /// and, for MIDI clips, never shrinks below the last note end. Left-edge
    /// resizes re-offset clip-local notes so they keep their absolute position,
    /// clamping so the earliest note never crosses clip-local beat 0.
    ///
    /// UI-mutating only — the caller marks the project dirty once on commit.
    /// Returns `true` when a matching clip was found.
    pub fn resize_clip(&mut self, clip_id: &str, edge: ClipEdge, new_edge_beat: f32) -> bool {
        let snapped = self.snap_beats(new_edge_beat).max(0.0);
        let Some(track) = self
            .tracks
            .iter_mut()
            .find(|t| t.clips.iter().any(|c| c.id == clip_id))
        else {
            return false;
        };
        let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) else {
            return false;
        };

        let is_midi = matches!(clip.clip_type, ClipType::Midi { .. });
        let min_len = if is_midi { MIN_MIDI_CLIP_BEATS } else { 0.25 };
        // Clip-local end of the furthest note — the floor for any MIDI shrink.
        let last_note_end = if let ClipType::Midi { notes, .. } = &clip.clip_type {
            notes
                .iter()
                .map(|n| n.start.max(0.0) + n.duration.max(MIN_NOTE_BEATS))
                .fold(0.0_f32, f32::max)
        } else {
            0.0
        };

        match edge {
            ClipEdge::Right => {
                // Right edge moves; start fixed. Cannot shrink below the last
                // note end or the minimum length.
                let dur = (snapped - clip.start_beat).max(min_len).max(last_note_end);
                clip.duration_beats = dur;
            }
            ClipEdge::Left => {
                let old_start = clip.start_beat;
                let old_right = old_start + clip.duration_beats;
                // Keep the right edge fixed; clamp the new start to [0, right-min].
                let mut new_start = snapped.min(old_right - min_len).max(0.0);
                // Trimming from the left must not push the earliest note < 0.
                if let ClipType::Midi { notes, .. } = &clip.clip_type {
                    if let Some(min_local) = notes.iter().map(|n| n.start).reduce(f32::min) {
                        let max_start = (old_start + min_local).max(0.0);
                        new_start = new_start.min(max_start);
                    }
                }
                let delta = old_start - new_start;
                if let ClipType::Midi { notes, .. } = &mut clip.clip_type {
                    for note in notes.iter_mut() {
                        note.start = (note.start + delta).max(0.0);
                    }
                }
                clip.start_beat = new_start;
                clip.duration_beats = (old_right - new_start).max(min_len);
            }
        }

        if midi_debug_enabled() {
            eprintln!(
                "[midi] resize_clip clip={} edge={:?} start={:.3} len={:.3}",
                clip_id, edge, clip.start_beat, clip.duration_beats
            );
        }
        true
    }
}
