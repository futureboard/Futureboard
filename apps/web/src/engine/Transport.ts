import type { DawFile, DawTrack, WaveformPeaks } from "../types/daw";
import { audioEngine } from "./AudioEngine";
import { metronomeScheduler } from "./MetronomeScheduler";
import { clipScheduler } from "./ClipScheduler";

export type PlayState = "stopped" | "playing" | "paused";

class Transport {
  private _state: PlayState = "stopped";
  private transportStartAudioTime = 0;
  private transportStartProjectTime = 0;
  private _playheadTime = 0;
  private trackGetter: (() => DawTrack[]) | null = null;
  private fileGetter: (() => DawFile[]) | null = null;
  private peaksCallback: ((fileId: string, peaks: WaveformPeaks) => void) | null = null;

  setTrackGetter(fn: () => DawTrack[]): void {
    this.trackGetter = fn;
  }

  setFileGetter(fn: () => DawFile[]): void {
    this.fileGetter = fn;
  }

  setPeaksCallback(fn: (fileId: string, peaks: WaveformPeaks) => void): void {
    this.peaksCallback = fn;
  }

  private getTracks(): DawTrack[] {
    return this.trackGetter?.() ?? [];
  }

  private getFiles(): DawFile[] {
    return this.fileGetter?.() ?? [];
  }

  get state(): PlayState {
    return this._state;
  }

  get isPlaying(): boolean {
    return this._state === "playing";
  }

  get projectTime(): number {
    if (this._state !== "playing") return this._playheadTime;
    return (
      this.transportStartProjectTime +
      (audioEngine.currentTime - this.transportStartAudioTime)
    );
  }

  /**
   * Start playback.
   *
   * @param options.skipProviders  Storage providers whose files should NOT be
   *   loaded into the WebAudio buffer cache.  Pass `["project-folder"]` when
   *   the native Rust engine is active so it handles those files and we avoid
   *   double audio.
   */
  async play(options?: { skipProviders?: Array<DawFile["storageProvider"]> }) {
    if (this._state === "playing") return;
    await audioEngine.resume();
    await audioEngine.ensureSoundTouchWorklet().catch((error) => {
      console.warn("[Transport] SoundTouch worklet unavailable:", error);
    });
    this.transportStartAudioTime = audioEngine.currentTime;
    this.transportStartProjectTime = this._playheadTime;
    await this.ensurePlayableBuffers(options?.skipProviders);
    this._state = "playing";
    metronomeScheduler.start();
    clipScheduler.schedule(this.getTracks());
  }

  private async ensurePlayableBuffers(
    skipProviders?: Array<DawFile["storageProvider"]>,
  ): Promise<void> {
    const files = new Map(this.getFiles().map((file) => [file.id, file]));
    const neededFileIds = new Set<string>();
    const playheadTime = this._playheadTime;
    for (const track of this.getTracks()) {
      for (const clip of track.clips) {
        if (clip.startTime + clip.duration > playheadTime && clip.fileId) {
          neededFileIds.add(clip.fileId);
        }
      }
    }

    for (const fileId of neededFileIds) {
      if (audioEngine.getBuffer(fileId)) continue;
      const file = files.get(fileId);
      if (!file) continue;
      // Skip files owned by the native engine to prevent double audio.
      if (skipProviders && skipProviders.includes(file.storageProvider)) continue;
      await audioEngine.restoreBuffer(file, (fid, peaks) => {
        this.peaksCallback?.(fid, peaks);
      });
    }
  }

  pause() {
    if (this._state !== "playing") return;
    this._playheadTime = this.projectTime;
    this._state = "paused";
    metronomeScheduler.stop();
    clipScheduler.cancelAll();
  }

  stop() {
    this._playheadTime = 0;
    this._state = "stopped";
    metronomeScheduler.stop();
    clipScheduler.cancelAll();
  }

  seek(time: number) {
    const wasPlaying = this._state === "playing";
    if (wasPlaying) {
      this._state = "paused";
      clipScheduler.cancelAll();
    }
    this._playheadTime = Math.max(0, time);
    if (wasPlaying) {
      this.transportStartAudioTime = audioEngine.currentTime;
      this.transportStartProjectTime = this._playheadTime;
      this._state = "playing";
      clipScheduler.schedule(this.getTracks());
      metronomeScheduler.seek();
    }
  }

  /**
   * Reschedule all clips from the current playhead position without stopping
   * playback or resetting the playhead. No-op when transport is not playing.
   *
   * Call this after processed audio cache is invalidated (e.g. speed/pitch change)
   * or when clip params (gain, mute, position) change while playing.
   */
  rescheduleIfPlaying(): void {
    if (this._state !== "playing") return;
    // Snapshot current position so schedule() reads the right playhead time
    const currentPos = this.projectTime;
    this.transportStartAudioTime    = audioEngine.currentTime;
    this.transportStartProjectTime  = currentPos;
    this._playheadTime              = currentPos;
    clipScheduler.schedule(this.getTracks());
  }
}

export const transport = new Transport();
