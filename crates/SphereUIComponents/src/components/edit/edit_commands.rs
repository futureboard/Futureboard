//! Undo/redo edit commands — all timeline mutations go through here.

use crate::components::timeline::timeline_state::{
    ClipState, ClipType, MidiNoteState, TimelineState,
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

/// Editable command with perfect undo.
#[derive(Debug, Clone)]
pub enum EditCommand {
    CreateClip {
        track_id: String,
        clip: ClipState,
    },
    DeleteClip {
        snapshot: ClipSnapshot,
    },
    BatchDeleteClips {
        snapshots: Vec<ClipSnapshot>,
    },
    CreateMidiNote {
        clip_id: String,
        note: MidiNoteState,
    },
    DeleteMidiNotes {
        clip_id: String,
        notes: Vec<MidiNoteState>,
    },
}

impl EditCommand {
    pub fn label(&self) -> &'static str {
        match self {
            EditCommand::CreateClip { .. } => "Create Clip",
            EditCommand::DeleteClip { .. } => "Delete Clip",
            EditCommand::BatchDeleteClips { .. } => "Delete Clips",
            EditCommand::CreateMidiNote { .. } => "Create MIDI Note",
            EditCommand::DeleteMidiNotes { .. } => "Delete MIDI Notes",
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
            EditCommand::DeleteClip { snapshot } => {
                state.delete_clip(&snapshot.clip.id);
            }
            EditCommand::BatchDeleteClips { snapshots } => {
                for snap in snapshots {
                    state.delete_clip(&snap.clip.id);
                }
            }
            EditCommand::CreateMidiNote { clip_id, note } => {
                if let Some(notes) = state.midi_clip_notes_mut(clip_id) {
                    if !notes.iter().any(|n| n.id == note.id) {
                        notes.push(note.clone());
                    }
                }
            }
            EditCommand::DeleteMidiNotes { clip_id, notes } => {
                let ids: Vec<u64> = notes.iter().map(|n| n.id).collect();
                state.delete_midi_notes(clip_id, &ids);
            }
        }
    }

    pub fn undo(&self, state: &mut TimelineState) {
        match self {
            EditCommand::CreateClip { clip, .. } => {
                state.delete_clip(&clip.id);
            }
            EditCommand::DeleteClip { snapshot } => {
                restore_clip_snapshot(state, snapshot);
            }
            EditCommand::BatchDeleteClips { snapshots } => {
                for snap in snapshots {
                    restore_clip_snapshot(state, snap);
                }
            }
            EditCommand::CreateMidiNote { clip_id, note } => {
                state.delete_midi_notes(clip_id, &[note.id]);
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
