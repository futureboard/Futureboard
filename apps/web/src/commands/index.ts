/**
 * All concrete DAW commands.
 *
 * Each command captures the BEFORE and AFTER state it needs for perfect
 * undo / redo. Commands call the raw projectStore setters directly —
 * they do not go through historyStore again (no recursion).
 */

import type { DawClip, DawTrack, MidiNote, TrackPreviewMode, TrackSend } from "../types/daw";
import { useProjectStore } from "../store/projectStore";
import { activeAudioEngine } from "../engine/activeAudioEngine";
import type { DawCommand } from "./types";

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

function store() {
  return useProjectStore.getState();
}

// ─────────────────────────────────────────────────────────────────────────────
// Track commands
// ─────────────────────────────────────────────────────────────────────────────

export class AddTrackCommand implements DawCommand {
  readonly label: string;
  private track: DawTrack;

  constructor(track: DawTrack) {
    this.track = track;
    this.label = `Add Track "${track.name}"`;
  }
  execute() {
    store().addTrack(this.track);
    activeAudioEngine.createTrack(this.track);
  }
  undo() {
    store().removeTrack(this.track.id);
    activeAudioEngine.removeTrack(this.track.id);
  }
}

export class BatchImportCommand implements DawCommand {
  readonly label: string;
  private tracks: DawTrack[];
  private clips: Array<{ trackId: string; clip: DawClip }>;

  constructor(tracks: DawTrack[], clips: Array<{ trackId: string; clip: DawClip }>) {
    this.tracks = tracks;
    this.clips = clips;
    const n = tracks.length || clips.length;
    this.label = `Import ${n} Track${n === 1 ? "" : "s"}`;
  }
  execute() { store().batchImportTracks(this.tracks, this.clips); }
  undo() {
    for (const { clip } of this.clips) store().removeClip(clip.id);
    for (const track of this.tracks) store().removeTrack(track.id);
  }
}

export class DeleteTrackCommand implements DawCommand {
  readonly label: string;
  private trackId: string;
  private snapshot: DawTrack | undefined;

  constructor(trackId: string) {
    this.trackId = trackId;
    const t = store().project.tracks.find((t) => t.id === trackId);
    this.snapshot = t ? { ...t, clips: [...t.clips] } : undefined;
    this.label = `Delete Track "${this.snapshot?.name ?? trackId}"`;
  }
  execute() {
    if (!this.snapshot) {
      this.snapshot = store().project.tracks.find((t) => t.id === this.trackId);
    }
    store().removeTrack(this.trackId);
  }
  undo() {
    if (this.snapshot) {
      store().addTrack(this.snapshot);
      activeAudioEngine.createTrack(this.snapshot);
    }
  }
}

export class RenameTrackCommand implements DawCommand {
  readonly label: string;
  private trackId: string;
  private newName: string;
  private oldName: string;

  constructor(trackId: string, newName: string, oldName: string) {
    this.trackId = trackId;
    this.newName = newName;
    this.oldName = oldName;
    this.label = `Rename Track to "${newName}"`;
  }
  execute() { store().setTrackName(this.trackId, this.newName); }
  undo()    { store().setTrackName(this.trackId, this.oldName); }
}

export class SetTrackColorCommand implements DawCommand {
  readonly label = "Change Track Color";
  private trackId: string;
  private newColor: string;
  private oldColor: string;

  constructor(trackId: string, newColor: string, oldColor: string) {
    this.trackId = trackId;
    this.newColor = newColor;
    this.oldColor = oldColor;
  }
  execute() { store().setTrackColor(this.trackId, this.newColor); }
  undo()    { store().setTrackColor(this.trackId, this.oldColor); }
}

export class DuplicateTrackCommand implements DawCommand {
  readonly label = "Duplicate Track";
  private originalTrackId: string;
  private newTrackId: string;
  private newTrackSnapshot: DawTrack | null = null;
  
  constructor(trackId: string) {
    this.originalTrackId = trackId;
    this.newTrackId = crypto.randomUUID();
  }

  execute() {
    if (this.newTrackSnapshot) {
      store().addTrack(this.newTrackSnapshot);
      activeAudioEngine.createTrack(this.newTrackSnapshot);
      return;
    }

    const t = store().project.tracks.find(x => x.id === this.originalTrackId);
    if (!t) return;
    
    const newTrack: DawTrack = {
      ...t,
      id: this.newTrackId,
      name: t.name + " (Copy)",
      clips: t.clips.map(c => ({
        ...c,
        id: crypto.randomUUID(),
        trackId: this.newTrackId
      }))
    };
    
    this.newTrackSnapshot = newTrack;
    store().addTrack(newTrack);
    activeAudioEngine.createTrack(newTrack);
  }
  
  undo() {
    store().removeTrack(this.newTrackId);
  }
}

export class SetTrackVolumeCommand implements DawCommand {
  readonly label = "Set Track Volume";
  private trackId: string;
  private newVolume: number;
  private oldVolume: number;

  constructor(trackId: string, newVolume: number, oldVolume: number) {
    this.trackId = trackId;
    this.newVolume = newVolume;
    this.oldVolume = oldVolume;
  }
  execute() {
    store().setTrackVolume(this.trackId, this.newVolume);
    activeAudioEngine.setTrackVolume(this.trackId, this.newVolume);
  }
  undo() {
    store().setTrackVolume(this.trackId, this.oldVolume);
    activeAudioEngine.setTrackVolume(this.trackId, this.oldVolume);
  }
}

export class SetTrackPanCommand implements DawCommand {
  readonly label = "Set Track Pan";
  private trackId: string;
  private newPan: number;
  private oldPan: number;

  constructor(trackId: string, newPan: number, oldPan: number) {
    this.trackId = trackId;
    this.newPan = newPan;
    this.oldPan = oldPan;
  }
  execute() {
    store().setTrackPan(this.trackId, this.newPan);
    activeAudioEngine.setTrackPan(this.trackId, this.newPan);
  }
  undo() {
    store().setTrackPan(this.trackId, this.oldPan);
    activeAudioEngine.setTrackPan(this.trackId, this.oldPan);
  }
}

export class SetTrackMuteCommand implements DawCommand {
  readonly label: string;
  private trackId: string;
  private newMuted: boolean;

  constructor(trackId: string, newMuted: boolean) {
    this.trackId = trackId;
    this.newMuted = newMuted;
    this.label = newMuted ? "Mute Track" : "Unmute Track";
  }
  execute() {
    store().setTrackMute(this.trackId, this.newMuted);
    activeAudioEngine.setTrackMute(this.trackId, this.newMuted);
  }
  undo() {
    store().setTrackMute(this.trackId, !this.newMuted);
    activeAudioEngine.setTrackMute(this.trackId, !this.newMuted);
  }
}

export class SetTrackSoloCommand implements DawCommand {
  readonly label: string;
  private trackId: string;
  private newSolo: boolean;

  constructor(trackId: string, newSolo: boolean) {
    this.trackId = trackId;
    this.newSolo = newSolo;
    this.label = newSolo ? "Solo Track" : "Unsolo Track";
  }
  execute() {
    store().setTrackSolo(this.trackId, this.newSolo);
    activeAudioEngine.setTrackSolo(this.trackId, this.newSolo);
  }
  undo() {
    store().setTrackSolo(this.trackId, !this.newSolo);
    activeAudioEngine.setTrackSolo(this.trackId, !this.newSolo);
  }
}

export class SetTrackPreviewModeCommand implements DawCommand {
  readonly label: string;
  private trackId: string;
  private newMode: TrackPreviewMode;
  private oldMode: TrackPreviewMode;

  constructor(trackId: string, newMode: TrackPreviewMode, oldMode: TrackPreviewMode = "stereo") {
    this.trackId = trackId;
    this.newMode = newMode;
    this.oldMode = oldMode;
    this.label = `Set Preview Mode: ${newMode}`;
  }
  execute() {
    store().setTrackPreviewMode(this.trackId, this.newMode);
    activeAudioEngine.setTrackPreviewMode(this.trackId, this.newMode);
  }
  undo() {
    store().setTrackPreviewMode(this.trackId, this.oldMode);
    activeAudioEngine.setTrackPreviewMode(this.trackId, this.oldMode);
  }
}

export class SetTrackOutputCommand implements DawCommand {
  readonly label = "Set Track Output";
  private trackId: string;
  private newOutput: string;
  private oldOutput: string;

  constructor(trackId: string, newOutput: string, oldOutput: string) {
    this.trackId = trackId;
    this.newOutput = newOutput;
    this.oldOutput = oldOutput;
  }
  execute() {
    store().setTrackOutput(this.trackId, this.newOutput);
    activeAudioEngine.syncProject(store().project);
  }
  undo() {
    store().setTrackOutput(this.trackId, this.oldOutput);
    activeAudioEngine.syncProject(store().project);
  }
}

export class AddTrackSendCommand implements DawCommand {
  readonly label: string;
  private trackId: string;
  private send: TrackSend;

  constructor(trackId: string, send: TrackSend) {
    this.trackId = trackId;
    this.send = send;
    this.label = `Add Send to "${send.name}"`;
  }
  execute() {
    store().addTrackSend(this.trackId, this.send);
    activeAudioEngine.syncProject(store().project);
  }
  undo() {
    store().removeTrackSend(this.trackId, this.send.id);
    activeAudioEngine.syncProject(store().project);
  }
}

export class RemoveTrackSendCommand implements DawCommand {
  readonly label: string;
  private trackId: string;
  private send: TrackSend;

  constructor(trackId: string, send: TrackSend) {
    this.trackId = trackId;
    this.send = send;
    this.label = `Remove Send to "${send.name}"`;
  }
  execute() {
    store().removeTrackSend(this.trackId, this.send.id);
    activeAudioEngine.syncProject(store().project);
  }
  undo() {
    store().addTrackSend(this.trackId, this.send);
    activeAudioEngine.syncProject(store().project);
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// Clip commands
// ─────────────────────────────────────────────────────────────────────────────

export class AddClipCommand implements DawCommand {
  readonly label: string;
  private trackId: string;
  private clip: DawClip;

  constructor(trackId: string, clip: DawClip) {
    this.trackId = trackId;
    this.clip = clip;
    this.label = `Add Clip "${clip.name}"`;
  }
  execute() { store().addClip(this.trackId, this.clip); }
  undo()    { store().removeClip(this.clip.id); }
}

export class MoveClipCommand implements DawCommand {
  readonly label = "Move Clip";
  private clipId: string;
  private trackId: string;
  private newStartTime: number;
  private oldStartTime: number;
  private newTrackId: string | undefined;
  private oldTrackId: string | undefined;

  constructor(
    clipId: string,
    trackId: string,
    newStartTime: number,
    oldStartTime: number,
    /** Set when the clip moves to a different track */
    newTrackId?: string,
    oldTrackId?: string,
  ) {
    this.clipId = clipId;
    this.trackId = trackId;
    this.newStartTime = newStartTime;
    this.oldStartTime = oldStartTime;
    this.newTrackId = newTrackId;
    this.oldTrackId = oldTrackId;
  }

  execute() {
    if (this.newTrackId && this.newTrackId !== this.oldTrackId) {
      store().moveClipToTrack(this.clipId, this.newTrackId, this.newStartTime);
    } else {
      store().moveClip(this.clipId, this.trackId, this.newStartTime);
    }
  }
  undo() {
    if (this.oldTrackId && this.oldTrackId !== this.newTrackId) {
      store().moveClipToTrack(this.clipId, this.oldTrackId, this.oldStartTime);
    } else {
      store().moveClip(this.clipId, this.trackId, this.oldStartTime);
    }
  }
}

export class MoveClipsCommand implements DawCommand {
  readonly label = "Move Clips";
  private moves: Array<{ clipId: string; trackId: string; newTime: number; oldTime: number }>;

  constructor(moves: Array<{ clipId: string; trackId: string; newTime: number; oldTime: number }>) {
    this.moves = moves;
  }

  execute() {
    for (const m of this.moves) store().moveClip(m.clipId, m.trackId, m.newTime);
  }
  undo() {
    for (const m of this.moves) store().moveClip(m.clipId, m.trackId, m.oldTime);
  }
}

export class ResizeClipCommand implements DawCommand {
  readonly label = "Resize Clip";
  private clipId: string;
  private trackId: string;
  private newStartTime: number;
  private newOffset: number;
  private newDuration: number;
  private oldStartTime: number;
  private oldOffset: number;
  private oldDuration: number;

  constructor(
    clipId: string,
    trackId: string,
    newStartTime: number,
    newOffset: number,
    newDuration: number,
    oldStartTime: number,
    oldOffset: number,
    oldDuration: number,
  ) {
    this.clipId = clipId;
    this.trackId = trackId;
    this.newStartTime = newStartTime;
    this.newOffset = newOffset;
    this.newDuration = newDuration;
    this.oldStartTime = oldStartTime;
    this.oldOffset = oldOffset;
    this.oldDuration = oldDuration;
  }
  execute() {
    store().resizeClip(this.clipId, this.trackId, this.newStartTime, this.newOffset, this.newDuration);
  }
  undo() {
    store().resizeClip(this.clipId, this.trackId, this.oldStartTime, this.oldOffset, this.oldDuration);
  }
}

export class SplitClipCommand implements DawCommand {
  readonly label = "Split Clip";
  private clipId: string;
  private time: number;
  /** The second clip created by the split — generated at execute() time */
  private splitClipId: string | null = null;
  private originalClip: DawClip | undefined;

  constructor(clipId: string, time: number) {
    this.clipId = clipId;
    this.time = time;
    this.originalClip = store().project.tracks
      .flatMap((t) => t.clips)
      .find((c) => c.id === clipId);
  }

  execute() {
    // Snapshot the current clip so undo can restore it
    this.originalClip = store().project.tracks
      .flatMap((t) => t.clips)
      .find((c) => c.id === this.clipId);

    store().splitClip(this.clipId, this.time);

    // Find the new clip that was created (the one with id !== this.clipId starting at this.time)
    this.splitClipId = store().project.tracks
      .flatMap((t) => t.clips)
      .find((c) => c.id !== this.clipId && c.startTime === this.time && c.fileId === this.originalClip?.fileId)
      ?.id ?? null;
  }

  undo() {
    // Remove the second half
    if (this.splitClipId) store().removeClip(this.splitClipId);
    // Restore original clip dimensions
    if (this.originalClip) {
      store().resizeClip(
        this.clipId,
        this.originalClip.trackId,
        this.originalClip.startTime,
        this.originalClip.offset,
        this.originalClip.duration,
      );
    }
  }
}

export class SplitClipsCommand implements DawCommand {
  readonly label: string;
  private commands: SplitClipCommand[] = [];
  /** IDs of the newly created right-side clips */
  newClipIds: string[] = [];

  constructor(clipIds: string[], time: number) {
    this.label = clipIds.length === 1 ? "Split Clip" : `Split ${clipIds.length} Clips`;
    this.commands = clipIds.map(id => new SplitClipCommand(id, time));
  }

  execute() {
    const before = new Set(
      store().project.tracks.flatMap((t) => t.clips.map((c) => c.id)),
    );
    
    this.commands.forEach(cmd => cmd.execute());
    
    this.newClipIds = store()
      .project.tracks.flatMap((t) => t.clips.map((c) => c.id))
      .filter((id) => !before.has(id));
  }

  undo() {
    // Undo in reverse order
    [...this.commands].reverse().forEach(cmd => cmd.undo());
  }
}

export class DeleteClipsCommand implements DawCommand {
  readonly label: string;
  private clipIds: string[];
  /** Snapshots of all deleted clips (with their original trackIds) */
  private snapshots: Array<{ trackId: string; clip: DawClip }> = [];

  constructor(clipIds: string[]) {
    this.clipIds = clipIds;
    this.label = clipIds.length === 1 ? "Delete Clip" : `Delete ${clipIds.length} Clips`;
    // Capture the clips now, before deletion
    this._captureSnapshots();
  }

  private _captureSnapshots() {
    const ids = new Set(this.clipIds);
    for (const track of store().project.tracks) {
      for (const clip of track.clips) {
        if (ids.has(clip.id)) {
          this.snapshots.push({ trackId: track.id, clip: { ...clip } });
        }
      }
    }
  }

  execute() {
    // Re-capture in case execute() is called after redo
    this.snapshots = [];
    this._captureSnapshots();
    store().deleteClips(this.clipIds);
  }
  undo() {
    for (const { trackId, clip } of this.snapshots) {
      store().addClip(trackId, clip);
    }
  }
}

export class DuplicateClipsCommand implements DawCommand {
  readonly label: string;
  private clipIds: string[];
  /** IDs of the newly created duplicates — filled at execute() time */
  newClipIds: string[] = [];

  constructor(clipIds: string[]) {
    this.clipIds = clipIds;
    this.label = clipIds.length === 1 ? "Duplicate Clip" : `Duplicate ${clipIds.length} Clips`;
  }

  execute() {
    const before = new Set(
      store().project.tracks.flatMap((t) => t.clips.map((c) => c.id)),
    );
    store().duplicateClips(this.clipIds);
    this.newClipIds = store()
      .project.tracks.flatMap((t) => t.clips.map((c) => c.id))
      .filter((id) => !before.has(id));
  }
  undo() {
    if (this.newClipIds.length) store().deleteClips(this.newClipIds);
  }
}

export class GlueClipsCommand implements DawCommand {
  readonly label = "Glue Clips";
  private sortedClips: DawClip[];
  private trackId: string;
  private mergedDuration: number;

  constructor(sortedClips: DawClip[], trackId: string) {
    this.sortedClips = sortedClips;
    this.trackId = trackId;
    const last = sortedClips[sortedClips.length - 1];
    this.mergedDuration = last.startTime + last.duration - sortedClips[0].startTime;
  }

  execute() {
    store().updateClip(this.sortedClips[0].id, { duration: this.mergedDuration });
    store().deleteClips(this.sortedClips.slice(1).map((c) => c.id));
  }

  undo() {
    store().updateClip(this.sortedClips[0].id, { duration: this.sortedClips[0].duration });
    for (const clip of this.sortedClips.slice(1)) {
      store().addClip(this.trackId, clip);
    }
  }
}

export class UpdateClipCommand implements DawCommand {
  readonly label: string;
  private clipId: string;
  private updates: Partial<DawClip>;
  private oldValues: Partial<DawClip>;

  constructor(
    clipId: string,
    updates: Partial<DawClip>,
    label?: string,
  ) {
    this.clipId = clipId;
    this.updates = updates;
    this.label = label ?? "Edit Clip";
    // Capture the current values for the keys we are about to change
    const clip = store().project.tracks
      .flatMap((t) => t.clips)
      .find((c) => c.id === clipId);
    const old: Partial<DawClip> = {};
    for (const key of Object.keys(updates) as Array<keyof DawClip>) {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (old as any)[key] = clip?.[key];
    }
    this.oldValues = old;
  }
  execute() { store().updateClip(this.clipId, this.updates); }
  undo()    { store().updateClip(this.clipId, this.oldValues); }
}

// ─────────────────────────────────────────────────────────────────────────────
// MIDI note commands
// ─────────────────────────────────────────────────────────────────────────────

export class AddMidiNotesCommand implements DawCommand {
  readonly label = "Add MIDI Note";
  private clipId: string;
  private notes: MidiNote[];

  constructor(clipId: string, notes: MidiNote[]) {
    this.clipId = clipId;
    this.notes = notes;
  }
  execute() { store().addMidiNotes(this.clipId, this.notes); }
  undo()    { store().removeMidiNotes(this.clipId, this.notes.map((n) => n.id)); }
}

export class RemoveMidiNotesCommand implements DawCommand {
  readonly label = "Delete MIDI Notes";
  private clipId: string;
  private notes: MidiNote[];

  constructor(clipId: string, notes: MidiNote[]) {
    this.clipId = clipId;
    this.notes = notes;
  }
  execute() { store().removeMidiNotes(this.clipId, this.notes.map((n) => n.id)); }
  undo()    { store().addMidiNotes(this.clipId, this.notes); }
}

export class UpdateMidiNotesCommand implements DawCommand {
  readonly label: string;
  private _clipId: string;
  private prevNotes: MidiNote[];
  private nextNotes: MidiNote[];

  constructor(clipId: string, prevNotes: MidiNote[], nextNotes: MidiNote[], label = "Edit MIDI Notes") {
    this.label = label;
    this._clipId = clipId;
    this.prevNotes = prevNotes;
    this.nextNotes = nextNotes;
  }

  execute() {
    store().updateMidiNotes(this._clipId, this.nextNotes);
  }
  undo() {
    store().updateMidiNotes(this._clipId, this.prevNotes);
  }
}
