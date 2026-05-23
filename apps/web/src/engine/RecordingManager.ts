/**
 * RecordingManager — post-recording flow for the native (Electron/DAUx) path.
 *
 * After stopRecording() resolves with per-track WAV results, this module:
 *  1. Creates a DawFile + DawProjectAsset for each recorded WAV.
 *  2. Creates a DawClip placed at recordStartBeat on the armed track.
 *  3. Triggers waveform peak generation for immediate waveform display.
 */
import { nanoid } from "nanoid";
import { platform } from "../platform";
import { useProjectStore } from "../store/projectStore";
import type { DawFile, DawProjectAsset, DawClip } from "../types/daw";

export type RecordingTrackResult = {
  trackId: string;
  filePath: string;
  relativePath: string;
  startBeat: number;
  durationSeconds: number;
  sampleRate: number;
  channels: number;
  success: boolean;
  error?: string | null;
};

export async function commitRecordingResults(results: RecordingTrackResult[]): Promise<void> {
  const { project, addFile, addAsset, addClip } = useProjectStore.getState();

  for (const result of results) {
    if (!result.success || !result.filePath) continue;

    const track = project.tracks.find((t) => t.id === result.trackId);
    if (!track) continue;

    const id = nanoid();
    const fileName = result.relativePath.split("/").pop() ?? "Recording.wav";

    const file: DawFile = {
      id,
      name: fileName,
      mimeType: "audio/wav",
      duration: result.durationSeconds,
      sampleRate: result.sampleRate,
      channels: result.channels,
      storageProvider: "project-folder",
      relativePath: result.relativePath,
    };
    addFile(file);

    const asset: DawProjectAsset = {
      id,
      type: "audio",
      name: fileName,
      relativePath: result.relativePath,
      durationSeconds: result.durationSeconds,
      sampleRate: result.sampleRate,
      channels: result.channels,
    };
    addAsset(asset);

    // Place clip at the beat position the recording started.
    const beatsPerSecond = project.bpm / 60;
    const startTime = result.startBeat / beatsPerSecond;

    const clip: DawClip = {
      id: nanoid(),
      name: fileName.replace(/\.wav$/i, ""),
      type: "audio",
      fileId: id,
      assetId: id,
      trackId: result.trackId,
      startTime,
      offset: 0,
      duration: result.durationSeconds,
      gain: 1,
    };
    addClip(result.trackId, clip);

    // Kick off peak generation in the background (non-blocking).
    const absPath = result.filePath;
    if (platform.kind === "electron" && (window as { dawElectron?: { fs?: { generateWavPeaks?: unknown } } }).dawElectron?.fs?.generateWavPeaks) {
      const bridge = (window as { dawElectron: { fs: { generateWavPeaks: (path: string, id: string, spp: number) => Promise<unknown> } } }).dawElectron;
      bridge.fs
        .generateWavPeaks(absPath, id, 512)
        .catch((e: unknown) => console.warn("[RecordingManager] peak generation failed:", e));
    }
  }
}
