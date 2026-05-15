import type { AutomationLane, AutomationTarget, DawProject, TrackId } from "../types/daw";
import { evaluateAutomationAtBeat } from "./automationEval";

export function getAutomationLanesForTrack(
  project: DawProject,
  trackId: TrackId,
): AutomationLane[] {
  const track = project.tracks.find((t) => t.id === trackId);
  return track?.automationLanes ?? [];
}

export function getVisibleAutomationLanesForTrack(
  project: DawProject,
  trackId: TrackId,
): AutomationLane[] {
  return getAutomationLanesForTrack(project, trackId).filter((l) => l.visible);
}

export function getAutomationLane(
  project: DawProject,
  trackId: TrackId,
  laneId: string,
): AutomationLane | undefined {
  return getAutomationLanesForTrack(project, trackId).find((l) => l.id === laneId);
}

export function hasAutomationForTarget(project: DawProject, targetId: string): boolean {
  for (const track of project.tracks) {
    if ((track.automationLanes ?? []).some((l) => l.target.id === targetId)) return true;
  }
  return false;
}

export function getAutomationValueAtBeat(
  project: DawProject,
  targetId: string,
  beat: number,
): number | undefined {
  for (const track of project.tracks) {
    const lane = (track.automationLanes ?? []).find((l) => l.target.id === targetId);
    if (lane) {
      return evaluateAutomationAtBeat(lane.points, beat, lane.target.defaultValue);
    }
  }
  return undefined;
}

export function getAutomatedTargetIdsForTrack(
  project: DawProject,
  trackId: TrackId,
): string[] {
  return getAutomationLanesForTrack(project, trackId).map((l) => l.target.id);
}

/** Returns all AutomationTargets currently automated across all tracks. */
export function getAllAutomatedTargets(project: DawProject): AutomationTarget[] {
  return project.tracks.flatMap((t) =>
    (t.automationLanes ?? []).map((l) => l.target)
  );
}
