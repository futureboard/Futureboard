import type { DawProject, DawTrack, TrackType } from "../types/daw";

export type RoutingTarget = {
  id: string;           // "master" or a track ID
  name: string;
  type: "master" | TrackType;
};

export function getTrackById(project: DawProject, trackId: string): DawTrack | undefined {
  return project.tracks.find((t) => t.id === trackId);
}

/** Returns the display name for an output target ID. */
export function outputTargetName(project: DawProject, output: string | undefined): string {
  if (!output || output === "master") return "Master";
  const t = project.tracks.find((t) => t.id === output);
  return t ? t.name : "Master";
}

/** Returns whether a target track is a valid routing destination for a source. */
export function isRoutableTarget(source: DawTrack, target: DawTrack): boolean {
  if (source.id === target.id) return false;
  // Return tracks must not route to other return tracks (trivial feedback)
  if (source.type === "return" && target.type === "return") return false;
  return true;
}

/**
 * Valid output targets for a track (where its final audio goes).
 * Audio/MIDI/instrument → master, bus, group.
 * Bus/group → master, other bus/group (not self, no trivial loop).
 * Return → master only (keep it simple).
 */
export function getOutputTargets(project: DawProject, sourceTrackId: string): RoutingTarget[] {
  const source = project.tracks.find((t) => t.id === sourceTrackId);
  const targets: RoutingTarget[] = [{ id: "master", name: "Master", type: "master" }];

  if (source?.type === "return") return targets; // return tracks always go to master

  for (const t of project.tracks) {
    if (t.id === sourceTrackId) continue;
    if (t.type !== "bus" && t.type !== "group") continue;
    if (!isRoutableTarget(source ?? { id: sourceTrackId } as DawTrack, t)) continue;
    targets.push({ id: t.id, name: t.name, type: t.type });
  }

  return targets;
}

/**
 * Valid send targets for a track (parallel routing, typically to return/bus FX).
 * Any track except self can receive a send, as long as it's a return or bus.
 */
export function getSendTargets(project: DawProject, sourceTrackId: string): RoutingTarget[] {
  const targets: RoutingTarget[] = [];

  for (const t of project.tracks) {
    if (t.id === sourceTrackId) continue;
    if (t.type === "return" || t.type === "bus") {
      targets.push({ id: t.id, name: t.name, type: t.type });
    }
  }

  return targets;
}
