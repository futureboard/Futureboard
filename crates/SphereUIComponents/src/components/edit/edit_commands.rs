//! Undo/redo edit commands — all timeline mutations go through here.

use crate::components::timeline::timeline_state::{
    ClipState, MidiControllerKind, MidiControllerPoint, MidiNoteState, TimelineState, TrackState,
};

/// Snapshot of a clip plus its owning track for undo/redo.
#[derive(Debug, Clone)]
pub struct ClipSnapshot {
    pub track_id: String,
    pub clip: ClipState,
}

impl ClipSnapshot {
    pub fn capture(state: &TimelineState, clip_id: &str) -> Option<Self> {
        for track in &state.tracks {
            if let Some(clip) = track.clips.iter().find(|c| c.id == clip_id) {
                return Some(Self {
                    track_id: track.id.clone(),
                    clip: clip.clone(),
                });
            }
        }
        None
    }
}

/// Snapshot of a track plus its original index for undo/redo.
#[derive(Debug, Clone)]
pub struct TrackSnapshot {
    pub index: usize,
    pub track: TrackState,
}

impl TrackSnapshot {
    pub fn capture(state: &TimelineState, track_id: &str) -> Option<Self> {
        state
            .tracks
            .iter()
            .position(|track| track.id == track_id)
            .map(|index| Self {
                index,
                track: state.tracks[index].clone(),
            })
    }
}

/// Editable command with perfect undo.
#[derive(Debug, Clone)]
pub enum EditCommand {
    CreateClip {
        track_id: String,
        clip: ClipState,
    },
    BatchCreateClips {
        clips: Vec<(String, ClipState)>,
    },
    DeleteClip {
        snapshot: ClipSnapshot,
    },
    BatchDeleteClips {
        snapshots: Vec<ClipSnapshot>,
    },
    ReplaceClipWithClips {
        snapshot: ClipSnapshot,
        clips: Vec<(String, ClipState)>,
    },
    DeleteTrack {
        snapshot: TrackSnapshot,
    },
    CreateMidiNote {
        clip_id: String,
        note: MidiNoteState,
    },
    /// Batch note insert (paste / duplicate) — one undo entry for the group.
    CreateMidiNotes {
        clip_id: String,
        notes: Vec<MidiNoteState>,
    },
    DeleteMidiNotes {
        clip_id: String,
        notes: Vec<MidiNoteState>,
    },
    /// Set the muted flag on a set of notes. `prev` snapshots each note's
    /// original muted state so undo restores it exactly.
    SetMidiNotesMuted {
        clip_id: String,
        prev: Vec<(u64, bool)>,
        muted: bool,
    },
    /// In-place transform of a fixed set of notes (move / resize / velocity /
    /// quantize / transpose / nudge). The note set is unchanged — only field
    /// values differ — so `prev`/`next` carry full per-id snapshots and
    /// execute/undo simply overwrite matching notes by id.
    EditMidiNotes {
        clip_id: String,
        prev: Vec<MidiNoteState>,
        next: Vec<MidiNoteState>,
    },
    /// Replace a controller lane's points (draw / erase gesture). One entry per
    /// gesture; `prev`/`next` are full point snapshots of the lane.
    SetControllerPoints {
        clip_id: String,
        kind: MidiControllerKind,
        prev: Vec<MidiControllerPoint>,
        next: Vec<MidiControllerPoint>,
    },
    /// Split one note into `parts` (two or more contiguous notes). Atomic so a
    /// single undo restores the original note and removes every part.
    SplitMidiNote {
        clip_id: String,
        original: MidiNoteState,
        parts: Vec<MidiNoteState>,
    },
}

impl EditCommand {
    pub fn label(&self) -> &'static str {
        match self {
            EditCommand::CreateClip { .. } => "Create Clip",
            EditCommand::BatchCreateClips { .. } => "Create Clips",
            EditCommand::DeleteClip { .. } => "Delete Clip",
            EditCommand::BatchDeleteClips { .. } => "Delete Clips",
            EditCommand::ReplaceClipWithClips { .. } => "Split Clip",
            EditCommand::DeleteTrack { .. } => "Delete Track",
            EditCommand::CreateMidiNote { .. } => "Create MIDI Note",
            EditCommand::CreateMidiNotes { .. } => "Add MIDI Notes",
            EditCommand::DeleteMidiNotes { .. } => "Delete MIDI Notes",
            EditCommand::SetMidiNotesMuted { muted, .. } => {
                if *muted {
                    "Mute Notes"
                } else {
                    "Unmute Notes"
                }
            }
            EditCommand::EditMidiNotes { .. } => "Edit MIDI Notes",
            EditCommand::SetControllerPoints { .. } => "Edit CC Lane",
            EditCommand::SplitMidiNote { .. } => "Split MIDI Note",
        }
    }

    pub fn execute(&self, state: &mut TimelineState) {
        match self {
            EditCommand::CreateClip { track_id, clip } => {
                if let Some(track) = state.tracks.iter_mut().find(|t| t.id == *track_id) {
                    track.clips.push(clip.clone());
                    state.selection.selected_track_id = Some(track_id.clone());
                    state.selection.selected_clip_ids = vec![clip.id.clone()];
                }
            }
            EditCommand::BatchCreateClips { clips } => {
                let mut selected = Vec::new();
                let mut selected_track = None;
                for (track_id, clip) in clips {
                    if let Some(track) = state.tracks.iter_mut().find(|t| t.id == *track_id) {
                        track.clips.push(clip.clone());
                        selected_track = Some(track_id.clone());
                        selected.push(clip.id.clone());
                    }
                }
                if !selected.is_empty() {
                    state.selection.selected_track_id = selected_track;
                    state.selection.selected_clip_ids = selected;
                }
            }
            EditCommand::DeleteClip { snapshot } => {
                state.delete_clip(&snapshot.clip.id);
            }
            EditCommand::BatchDeleteClips { snapshots } => {
                for snap in snapshots {
                    state.delete_clip(&snap.clip.id);
                }
            }
            EditCommand::ReplaceClipWithClips { snapshot, clips } => {
                state.delete_clip(&snapshot.clip.id);
                for (track_id, clip) in clips {
                    if let Some(track) = state.tracks.iter_mut().find(|t| t.id == *track_id) {
                        if !track.clips.iter().any(|c| c.id == clip.id) {
                            track.clips.push(clip.clone());
                        }
                    }
                }
                state.selection.selected_track_id = Some(snapshot.track_id.clone());
                state.selection.selected_clip_ids =
                    clips.iter().map(|(_, clip)| clip.id.clone()).collect();
            }
            EditCommand::DeleteTrack { snapshot } => {
                state.delete_track(&snapshot.track.id);
            }
            EditCommand::CreateMidiNote { clip_id, note } => {
                if let Some(notes) = state.midi_clip_notes_mut(clip_id) {
                    if !notes.iter().any(|n| n.id == note.id) {
                        notes.push(note.clone());
                    }
                }
                // A note drawn past the clip end auto-expands the clip so it is
                // always contained. Applies to redo too.
                state.expand_clip_to_contain_notes(clip_id);
            }
            EditCommand::CreateMidiNotes { clip_id, notes } => {
                if let Some(existing) = state.midi_clip_notes_mut(clip_id) {
                    for note in notes {
                        if !existing.iter().any(|n| n.id == note.id) {
                            existing.push(note.clone());
                        }
                    }
                }
                state.expand_clip_to_contain_notes(clip_id);
            }
            EditCommand::DeleteMidiNotes { clip_id, notes } => {
                let ids: Vec<u64> = notes.iter().map(|n| n.id).collect();
                state.delete_midi_notes(clip_id, &ids);
            }
            EditCommand::SetMidiNotesMuted {
                clip_id,
                prev,
                muted,
            } => {
                let ids: Vec<u64> = prev.iter().map(|(id, _)| *id).collect();
                state.set_midi_notes_muted(clip_id, &ids, *muted);
            }
            EditCommand::EditMidiNotes { clip_id, next, .. } => {
                state.overwrite_midi_notes(clip_id, next);
            }
            EditCommand::SetControllerPoints {
                clip_id,
                kind,
                next,
                ..
            } => {
                state.set_controller_lane_points(clip_id, *kind, next.clone());
            }
            EditCommand::SplitMidiNote {
                clip_id,
                original,
                parts,
            } => {
                state.delete_midi_notes(clip_id, &[original.id]);
                if let Some(existing) = state.midi_clip_notes_mut(clip_id) {
                    for note in parts {
                        if !existing.iter().any(|n| n.id == note.id) {
                            existing.push(note.clone());
                        }
                    }
                }
                state.expand_clip_to_contain_notes(clip_id);
            }
        }
    }

    pub fn undo(&self, state: &mut TimelineState) {
        match self {
            EditCommand::CreateClip { clip, .. } => {
                state.delete_clip(&clip.id);
            }
            EditCommand::BatchCreateClips { clips } => {
                for (_, clip) in clips {
                    state.delete_clip(&clip.id);
                }
            }
            EditCommand::DeleteClip { snapshot } => {
                restore_clip_snapshot(state, snapshot);
            }
            EditCommand::BatchDeleteClips { snapshots } => {
                for snap in snapshots {
                    restore_clip_snapshot(state, snap);
                }
            }
            EditCommand::ReplaceClipWithClips { snapshot, clips } => {
                for (_, clip) in clips {
                    state.delete_clip(&clip.id);
                }
                restore_clip_snapshot(state, snapshot);
                state.selection.selected_track_id = Some(snapshot.track_id.clone());
                state.selection.selected_clip_ids = vec![snapshot.clip.id.clone()];
            }
            EditCommand::DeleteTrack { snapshot } => {
                restore_track_snapshot(state, snapshot);
            }
            EditCommand::CreateMidiNote { clip_id, note } => {
                state.delete_midi_notes(clip_id, &[note.id]);
            }
            EditCommand::CreateMidiNotes { clip_id, notes } => {
                let ids: Vec<u64> = notes.iter().map(|n| n.id).collect();
                state.delete_midi_notes(clip_id, &ids);
            }
            EditCommand::DeleteMidiNotes { clip_id, notes } => {
                if let Some(existing) = state.midi_clip_notes_mut(clip_id) {
                    for note in notes {
                        if !existing.iter().any(|n| n.id == note.id) {
                            existing.push(note.clone());
                        }
                    }
                }
            }
            EditCommand::SetMidiNotesMuted { clip_id, prev, .. } => {
                if let Some(existing) = state.midi_clip_notes_mut(clip_id) {
                    for (id, was) in prev {
                        if let Some(note) = existing.iter_mut().find(|n| n.id == *id) {
                            note.muted = *was;
                        }
                    }
                }
            }
            EditCommand::EditMidiNotes { clip_id, prev, .. } => {
                state.overwrite_midi_notes(clip_id, prev);
            }
            EditCommand::SetControllerPoints {
                clip_id,
                kind,
                prev,
                ..
            } => {
                state.set_controller_lane_points(clip_id, *kind, prev.clone());
            }
            EditCommand::SplitMidiNote {
                clip_id,
                original,
                parts,
            } => {
                let ids: Vec<u64> = parts.iter().map(|n| n.id).collect();
                state.delete_midi_notes(clip_id, &ids);
                if let Some(existing) = state.midi_clip_notes_mut(clip_id) {
                    if !existing.iter().any(|n| n.id == original.id) {
                        existing.push(original.clone());
                    }
                }
                state.expand_clip_to_contain_notes(clip_id);
            }
        }
    }
}

fn restore_clip_snapshot(state: &mut TimelineState, snapshot: &ClipSnapshot) {
    if let Some(track) = state.tracks.iter_mut().find(|t| t.id == snapshot.track_id) {
        if !track.clips.iter().any(|c| c.id == snapshot.clip.id) {
            track.clips.push(snapshot.clip.clone());
        }
    }
}

fn restore_track_snapshot(state: &mut TimelineState, snapshot: &TrackSnapshot) {
    if state
        .tracks
        .iter()
        .any(|track| track.id == snapshot.track.id)
    {
        return;
    }
    let index = snapshot.index.min(state.tracks.len());
    state.tracks.insert(index, snapshot.track.clone());
    state.selection.selected_track_id = Some(snapshot.track.id.clone());
    state.selection.selected_clip_ids.clear();
}

/// Bounded undo/redo stack.
#[derive(Debug, Clone, Default)]
pub struct EditHistory {
    undo_stack: Vec<EditCommand>,
    redo_stack: Vec<EditCommand>,
    max_steps: usize,
}

impl EditHistory {
    pub fn new(max_steps: usize) -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            max_steps: max_steps.max(1),
        }
    }

    pub fn push(&mut self, cmd: EditCommand) {
        self.undo_stack.push(cmd);
        if self.undo_stack.len() > self.max_steps {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    pub fn undo(&mut self, state: &mut TimelineState) -> bool {
        let Some(cmd) = self.undo_stack.pop() else {
            return false;
        };
        cmd.undo(state);
        self.redo_stack.push(cmd);
        true
    }

    pub fn redo(&mut self, state: &mut TimelineState) -> bool {
        let Some(cmd) = self.redo_stack.pop() else {
            return false;
        };
        cmd.execute(state);
        self.undo_stack.push(cmd);
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
}
