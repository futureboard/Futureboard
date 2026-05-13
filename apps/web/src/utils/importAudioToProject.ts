import { audioEngine } from "../engine/AudioEngine";
import { mixer } from "../engine/Mixer";
import { useProjectStore } from "../store/projectStore";
import { getTrackColor } from "../theme";
import type { DawClip, DawFile, DawTrack } from "../types/daw";

export function isImportableAudioFile(file: File): boolean {
  if (file.type.startsWith("audio/")) return true;
  const m = /\.([^.]+)$/.exec(file.name);
  const ext = m?.[1]?.toLowerCase() ?? "";
  return ext === "wav" || ext === "mp3";
}

/**
 * Decode each file, add a new audio track, and place one clip at t=0 (same behaviour as file import).
 */
export async function importAudioFilesAsNewTracks(files: File[]): Promise<void> {
  const audioFiles = files.filter(isImportableAudioFile);
  if (audioFiles.length === 0) return;

  for (const f of audioFiles) {
    const { addTrack, addFile, addClip, setPeaks, project } = useProjectStore.getState();
    const arrayBuffer = await f.arrayBuffer();
    const fileId = crypto.randomUUID();
    const trackId = crypto.randomUUID();
    const clipId = crypto.randomUUID();
    const trackColor = getTrackColor(project.tracks.length);

    try {
      const audioBuffer = await audioEngine.loadBuffer(
        { id: fileId, name: f.name, mimeType: f.type, duration: 0, sampleRate: 48000, channels: 2 },
        arrayBuffer,
        (fid, peaks) => setPeaks(fid, peaks)
      );

      const dawFile: DawFile = {
        id: fileId,
        name: f.name,
        mimeType: f.type,
        duration: audioBuffer.duration,
        sampleRate: audioBuffer.sampleRate,
        channels: audioBuffer.numberOfChannels,
        localObjectUrl: URL.createObjectURL(f),
      };

      const track: DawTrack = {
        id: trackId,
        name: f.name.replace(/\.[^.]+$/, ""),
        type: "audio",
        color: trackColor,
        volume: 0.8,
        pan: 0,
        muted: false,
        solo: false,
        armed: false,
        clips: [],
      };

      const clip: DawClip = {
        id: clipId,
        name: f.name.replace(/\.[^.]+$/, ""),
        fileId,
        trackId,
        startTime: 0,
        offset: 0,
        duration: audioBuffer.duration,
        gain: 1,
      };

      mixer.getOrCreateTrack(trackId, track.volume, track.pan);
      addFile(dawFile);
      addTrack(track);
      addClip(trackId, clip);
    } catch (err) {
      console.error("Failed to import", f.name, err);
      alert(`Could not import "${f.name}". The format may not be supported.`);
    }
  }
}
