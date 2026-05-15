import type { DawTrack } from "../types/daw";
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

  setTrackGetter(fn: () => DawTrack[]): void {
    this.trackGetter = fn;
  }

  private getTracks(): DawTrack[] {
    return this.trackGetter?.() ?? [];
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

  async play() {
    if (this._state === "playing") return;
    await audioEngine.resume();
    this.transportStartAudioTime = audioEngine.currentTime;
    this.transportStartProjectTime = this._playheadTime;
    this._state = "playing";
    metronomeScheduler.start();
    clipScheduler.schedule(this.getTracks());
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
