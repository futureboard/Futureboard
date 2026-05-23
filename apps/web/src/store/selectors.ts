/**
 * Pure selector functions — derive computed values from project state.
 * Use these in UI components instead of duplicating lookup logic.
 */
import type { DawProject, DawTrack, DawClip, InsertDevice, TrackId, ClipId } from "../types/daw";

// ── Track lookups ─────────────────────────────────────────────────────────────

export function getTrack(project: DawProject, trackId: TrackId): DawTrack | undefined {
  return project.tracks.find((t) => t.id === trackId);
}

export function getMasterTrack(project: DawProject): DawTrack | undefined {
  if (project.masterTrackId) return getTrack(project, project.masterTrackId);
  return project.tracks.find((t) => t.type === "master");
}

export function getArrangementTracks(project: DawProject): DawTrack[] {
  return project.tracks.filter((t) => t.type !== "master" && t.type !== "return");
}

export function getMixerTracks(project: DawProject): DawTrack[] {
  return project.tracks;
}

// ── Clip lookups ──────────────────────────────────────────────────────────────

export function getClip(project: DawProject, clipId: ClipId): DawClip | undefined {
  for (const t of project.tracks) {
    const c = t.clips.find((c) => c.id === clipId);
    if (c) return c;
  }
  return undefined;
}

export function getClipTrack(project: DawProject, clipId: ClipId): DawTrack | undefined {
  return project.tracks.find((t) => t.clips.some((c) => c.id === clipId));
}

export function getTrackClips(track: DawTrack): DawClip[] {
  return track.clips;
}

// ── Insert lookups ────────────────────────────────────────────────────────────

export function getTrackInserts(track: DawTrack): InsertDevice[] {
  return (track.inserts ?? []).slice().sort((a, b) => a.order - b.order);
}

export function getInsertDevice(
  track: DawTrack,
  deviceId: string,
): InsertDevice | undefined {
  return track.inserts?.find((ins) => ins.id === deviceId);
}

// ── Selection helpers ─────────────────────────────────────────────────────────

export function getSelectedTrack(
  project: DawProject,
  selectedTrackId: TrackId | null,
): DawTrack | undefined {
  return selectedTrackId ? getTrack(project, selectedTrackId) : undefined;
}

export function getSelectedClip(
  project: DawProject,
  selectedClipIds: ClipId[],
): DawClip | undefined {
  if (selectedClipIds.length !== 1) return undefined;
  return getClip(project, selectedClipIds[0]);
}

export function getSelectedMidiClip(
  project: DawProject,
  selectedClipIds: ClipId[],
): DawClip | undefined {
  const clip = getSelectedClip(project, selectedClipIds);
  if (!clip) return undefined;
  return clip.type === "midi" || (!clip.fileId && clip.notes) ? clip : undefined;
}

// ── Mixer / routing helpers ───────────────────────────────────────────────────

/** Effective mute state, taking solo logic into account. */
export function getEffectiveTrackMute(project: DawProject, trackId: TrackId): boolean {
  const track = getTrack(project, trackId);
  if (!track) return false;
  if (track.muted) return true;
  const anySolo = project.tracks.some((t) => t.solo);
  if (anySolo && !track.solo) return true;
  return false;
}

export function getSoloState(project: DawProject): boolean {
  return project.tracks.some((t) => t.solo);
}

// ── Project length ────────────────────────────────────────────────────────────

/** Latest clip end time in seconds across all tracks. */
export function getProjectLengthSeconds(project: DawProject): number {
  let max = 0;
  for (const t of project.tracks) {
    for (const c of t.clips) {
      const end = c.startTime + c.duration;
      if (end > max) max = end;
    }
  }
  return max;
}
