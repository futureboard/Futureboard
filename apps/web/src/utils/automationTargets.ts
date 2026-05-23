import type { AutomationTarget, DawProject, DawTrack } from "../types/daw";

// ── Factory helpers ────────────────────────────────────────────────────────────

export function createTrackVolumeTarget(trackId: string): AutomationTarget {
  return {
    id: `track:${trackId}:volume`,
    kind: "track-volume",
    trackId,
    label: "Volume",
    unit: "dB",
    min: 0,
    max: 1,
    defaultValue: 1,
    displayScale: "db",
  };
}

export function createTrackPanTarget(trackId: string): AutomationTarget {
  return {
    id: `track:${trackId}:pan`,
    kind: "track-pan",
    trackId,
    label: "Pan",
    unit: "",
    min: -1,
    max: 1,
    defaultValue: 0,
    displayScale: "pan",
  };
}

export function createTrackMuteTarget(trackId: string): AutomationTarget {
  return {
    id: `track:${trackId}:mute`,
    kind: "track-mute",
    trackId,
    label: "Mute",
    unit: "",
    min: 0,
    max: 1,
    defaultValue: 0,
    displayScale: "linear",
  };
}

export function createDeviceParamTarget(
  trackId: string,
  deviceId: string,
  paramId: string,
  label: string,
  min = 0,
  max = 1,
  defaultValue = 0,
): AutomationTarget {
  return {
    id: `device:${trackId}:${deviceId}:${paramId}`,
    kind: "device-param",
    trackId,
    deviceId,
    paramId,
    label,
    min,
    max,
    defaultValue,
    displayScale: "linear",
  };
}

// ── Query helpers ──────────────────────────────────────────────────────────────

export function getTrackAutomationTargets(track: DawTrack): AutomationTarget[] {
  const targets: AutomationTarget[] = [
    createTrackVolumeTarget(track.id),
    createTrackPanTarget(track.id),
    createTrackMuteTarget(track.id),
  ];

  for (const device of track.inserts ?? []) {
    for (const [paramId, val] of Object.entries(device.params)) {
      if (typeof val === "number") {
        targets.push(
          createDeviceParamTarget(
            track.id,
            device.id,
            paramId,
            `${device.name}: ${paramId}`,
          )
        );
      }
    }
  }

  return targets;
}

export function getAutomationTargetsForProject(project: DawProject): AutomationTarget[] {
  return project.tracks.flatMap((t) => getTrackAutomationTargets(t));
}

export function getAutomationTargetById(
  project: DawProject,
  targetId: string,
): AutomationTarget | undefined {
  for (const track of project.tracks) {
    const targets = getTrackAutomationTargets(track);
    const found = targets.find((t) => t.id === targetId);
    if (found) return found;
  }
  return undefined;
}

/** Resolve a human-readable label for a display scale value. */
export function formatAutomationValue(value: number, target: AutomationTarget): string {
  switch (target.displayScale) {
    case "db": {
      if (value <= 0) return "-∞ dB";
      const db = 20 * Math.log10(value);
      return `${db >= 0 ? "+" : ""}${db.toFixed(1)} dB`;
    }
    case "pan": {
      if (Math.abs(value) < 0.01) return "C";
      const pct = Math.round(Math.abs(value) * 100);
      return value < 0 ? `L${pct}` : `R${pct}`;
    }
    case "percent":
      return `${Math.round(value * 100)}%`;
    default:
      return value.toFixed(2);
  }
}
