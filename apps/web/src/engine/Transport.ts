import { audioEngine } from "./AudioEngine";

export type PlayState = "stopped" | "playing" | "paused";

class Transport {
  private _state: PlayState = "stopped";
  private transportStartAudioTime = 0;
  private transportStartProjectTime = 0;
  private _playheadTime = 0;

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

  async play(onPlay?: () => void) {
    if (this._state === "playing") return;
    await audioEngine.resume();
    this.transportStartAudioTime = audioEngine.currentTime;
    this.transportStartProjectTime = this._playheadTime;
    this._state = "playing";
    onPlay?.();
  }

  pause() {
    if (this._state !== "playing") return;
    this._playheadTime = this.projectTime;
    this._state = "paused";
  }

  stop(onStop?: () => void) {
    this._playheadTime = 0;
    this._state = "stopped";
    onStop?.();
  }

  seek(time: number) {
    const wasPlaying = this._state === "playing";
    if (wasPlaying) {
      this._state = "paused";
    }
    this._playheadTime = Math.max(0, time);
    if (wasPlaying) {
      this.transportStartAudioTime = audioEngine.currentTime;
      this.transportStartProjectTime = this._playheadTime;
      this._state = "playing";
    }
  }
}

export const transport = new Transport();
