
import type { AudioEngineAdapter, AudioEngineStatus, MeterCallback, TransportCallback } from "./AudioEngineAdapter";
import type { DawProject, DawTrack, DawClip, InsertDevice, TrackId } from "../types/daw";

// Vite resolves new URL(..., import.meta.url) as a static asset URL for both
// dev and production builds. The .wasm file is served with the correct MIME type.
// eslint-disable-next-line @typescript-eslint/ban-ts-comment
// @ts-ignore
const wasmUrl = new URL("./wasm-pkg/futureboard_core_bg.wasm", import.meta.url).href;

// Load the AudioWorklet processor as a plain module URL (not a ?worker bundle).
// Using ?worker&url bundles the file as an IIFE chunk which inlines the
// wasm-bindgen glue and triggers new TextDecoder() at parse time — failing in
// the worker bundle environment. The raw .js file loaded via new URL() avoids
// all bundling and runs cleanly as a true AudioWorklet ES module.
// eslint-disable-next-line @typescript-eslint/ban-ts-comment
// @ts-ignore
const processorUrl = new URL("./WasmAudioProcessor.worklet.js", import.meta.url).href;

export class WasmAudioEngineAdapter implements AudioEngineAdapter {
  private _ctx: AudioContext | null = null;
  private _worklet: AudioWorkletNode | null = null;
  private _initialized = false;
  private _status: AudioEngineStatus = "uninitialized";
  private _transportCallbacks = new Set<TransportCallback>();
  private _meterCallbacks = new Set<MeterCallback>();
  private _bpm = 120;
  private _playing = false;
  private _positionSeconds = 0;

  async init(): Promise<void> {
    if (this._initialized) return;

    try {
      this._ctx = new AudioContext();
      await this._ctx.audioWorklet.addModule(processorUrl);

      const response = await fetch(wasmUrl);
      const wasmBytes = await response.arrayBuffer();

      this._worklet = new AudioWorkletNode(this._ctx, "wasm-audio-processor", {
        outputChannelCount: [2],
      });

      this._worklet.port.onmessage = this._handleMessage.bind(this);

      const config = {
        sample_rate: this._ctx.sampleRate,
        max_block_size: 128,
        channel_count: 2,
        bpm: this._bpm,
      };

      await new Promise<void>((resolve, reject) => {
        const onInit = (e: MessageEvent) => {
          if (e.data.type === "initialized") {
            this._worklet!.port.removeEventListener("message", onInit);
            resolve();
          } else if (e.data.type === "error") {
            this._worklet!.port.removeEventListener("message", onInit);
            reject(new Error(e.data.error));
          }
        };
        this._worklet!.port.addEventListener("message", onInit);
        this._worklet!.port.start();
        this._worklet!.port.postMessage({
          type: "init",
          payload: { wasmBytes, config },
        });
      });

      this._worklet.connect(this._ctx.destination);
      this._initialized = true;
      this._status = "running";
    } catch (e) {
      this._status = "error";
      console.error("[WasmAudioEngineAdapter] failed to init:", e);
      throw e;
    }
  }

  dispose(): void {
    if (this._worklet) {
      this._worklet.disconnect();
      this._worklet = null;
    }
    if (this._ctx) {
      this._ctx.close();
      this._ctx = null;
    }
    this._initialized = false;
    this._status = "closed";
  }

  getStatus(): AudioEngineStatus {
    return this._status;
  }

  private _handleMessage(e: MessageEvent) {
    const { type, payload } = e.data;
    if (type === "events") {
      for (const event of payload) {
        this._handleEvent(event);
      }
    }
  }

  private _handleEvent(event: any) {
    switch (event.type) {
      case "TransportPosition":
        // New struct API sends beat + time_seconds; old JSON API sent only time_seconds.
        if (event.time_seconds != null) this._positionSeconds = event.time_seconds;
        else if (event.beat != null && this._bpm > 0)
          this._positionSeconds = (event.beat * 60) / this._bpm;
        this._notifyTransport();
        break;
      case "PlaybackStarted":
        this._playing = true;
        this._notifyTransport();
        break;
      case "PlaybackPaused":
      case "PlaybackStopped":
        this._playing = false;
        this._positionSeconds = 0;
        this._notifyTransport();
        break;
      case "MeterUpdate":
        for (const m of event.meters) {
          for (const cb of this._meterCallbacks) {
            cb(m.track_id === "master" ? "master" : m.track_id, { l: m.left, r: m.right });
          }
        }
        break;
    }
  }

  private _notifyTransport() {
    const state = {
      playing: this._playing,
      positionSeconds: this._positionSeconds,
    };
    for (const cb of this._transportCallbacks) cb(state);
  }

  private _sendCommand(command: any) {
    if (this._worklet) {
      this._worklet.port.postMessage({ type: "command", payload: command });
    }
  }

  // ── Project sync ───────────────────────────────────────────────────────────
  async loadProject(_project: DawProject): Promise<void> {
    // Skeleton: just set BPM for now
    this.setBpm(_project.bpm);
  }

  syncProject(_project: DawProject): void {
    this.setBpm(_project.bpm);
  }

  // ── Transport ──────────────────────────────────────────────────────────────
  async play(positionSeconds?: number): Promise<void> {
    if (this._ctx?.state === "suspended") await this._ctx.resume();
    
    let position_beat: number | undefined;
    if (positionSeconds !== undefined) {
      position_beat = (positionSeconds * this._bpm) / 60;
    }
    this._sendCommand({ type: "Play", position_beat });
  }

  pause(): void {
    this._sendCommand({ type: "Pause" });
  }

  stop(): void {
    this._sendCommand({ type: "Stop" });
  }

  seekSeconds(seconds: number): void {
    const beat = (seconds * this._bpm) / 60;
    this._sendCommand({ type: "SeekBeat", beat });
  }

  setBpm(bpm: number): void {
    this._bpm = bpm;
    this._sendCommand({ type: "SetBpm", bpm });
  }

  setLoop(enabled: boolean, startSeconds: number, endSeconds: number): void {
    const start_beat = (startSeconds * this._bpm) / 60;
    const end_beat = (endSeconds * this._bpm) / 60;
    this._sendCommand({ type: "SetLoop", enabled, start_beat, end_beat });
  }

  // ── Track management ───────────────────────────────────────────────────────
  createTrack(track: DawTrack): void {
    this._sendCommand({
      type: "CreateTrack",
      track_id: track.id,
      volume: track.volume,
      pan: track.pan,
      muted: track.muted,
      solo: track.solo,
    });
  }

  removeTrack(trackId: TrackId): void {
    this._sendCommand({ type: "RemoveTrack", track_id: trackId });
  }

  // ── Clip management ────────────────────────────────────────────────────────
  scheduleClip(_trackId: TrackId, _clip: DawClip): void {}
  unscheduleClip(_clipId: string): void {}

  // ── Audio files ────────────────────────────────────────────────────────────
  loadAudioFile(_fileId: string, _buffer: AudioBuffer): void {}
  unloadAudioFile(_fileId: string): void {}

  // ── Mixer ──────────────────────────────────────────────────────────────────
  setTrackVolume(trackId: TrackId, volume: number): void {
    this._sendCommand({ type: "SetTrackVolume", track_id: trackId, volume });
  }

  setTrackPan(trackId: TrackId, pan: number): void {
    this._sendCommand({ type: "SetTrackPan", track_id: trackId, pan });
  }

  setTrackMute(trackId: TrackId, muted: boolean): void {
    this._sendCommand({ type: "SetTrackMute", track_id: trackId, muted });
  }

  setTrackSolo(trackId: TrackId, solo: boolean): void {
    this._sendCommand({ type: "SetTrackSolo", track_id: trackId, solo });
  }

  setTrackPhaseInvert(_trackId: TrackId, _inverted: boolean): void {}

  setTrackOutput(_trackId: TrackId, _output: string): void {}

  setMasterVolume(volume: number): void {
    this._sendCommand({ type: "SetMasterVolume", volume });
  }

  // ── Insert devices ─────────────────────────────────────────────────────────
  addInsertDevice(_trackId: TrackId, _device: InsertDevice): void {}
  removeInsertDevice(_trackId: TrackId, _deviceId: string): void {}
  setInsertEnabled(_trackId: TrackId, _deviceId: string, _enabled: boolean): void {}
  setInsertParam(_trackId: TrackId, _deviceId: string, _param: string, _value: any): void {}

  // ── Metering ───────────────────────────────────────────────────────────────
  subscribeMeters(callback: MeterCallback): () => void {
    this._meterCallbacks.add(callback);
    return () => this._meterCallbacks.delete(callback);
  }

  subscribeTransport(callback: TransportCallback): () => void {
    this._transportCallbacks.add(callback);
    return () => this._transportCallbacks.delete(callback);
  }
}
