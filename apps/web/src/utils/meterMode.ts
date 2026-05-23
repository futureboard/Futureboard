import type { DawFile, DawTrack } from "../types/daw";

/** Infer mono vs stereo from audio files on the track (max channel count). Empty tracks default to stereo. */
export function effectiveTrackMeterMode(track: DawTrack, files: DawFile[]): "mono" | "stereo" {
  if (track.clips.length === 0) return (track.channelCount ?? 2) >= 2 ? "stereo" : "mono";
  let maxCh = 1;
  for (const c of track.clips) {
    const f = files.find((x) => x.id === c.fileId);
    if (f) maxCh = Math.max(maxCh, f.channels);
  }
  return maxCh >= 2 ? "stereo" : "mono";
}
