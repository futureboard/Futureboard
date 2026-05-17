import type {
  AudioEngineAdapter,
  AudioSelfTestResult,
  MeterCallback,
  TransportCallback,
} from "./AudioEngineAdapter";
import type { DawProject, DawTrack, InsertDevice, TrackId } from "../types/daw";
import { createAudioEngineAdapter, type AdapterSelection } from "./native/createAudioEngineAdapter";
import type { PreferredEngine } from "../store/settingsStore";
import { useSettingsStore } from "../store/settingsStore";
import { useProjectStore } from "../store/projectStore";
import { useTransportStore } from "../store/transportStore";
import { useAudioBackendStore, type AudioBackendKind, type AudioBackendRequest } from "../store/audioBackendStore";
import { platform } from "../platform";
import { audioEngine } from "./AudioEngine";
import { metronomeScheduler } from "./MetronomeScheduler";

type ActiveBackend = AdapterSelection["backend"] | "uninitialized";

class ActiveAudioEngine {
  private _adapter: AudioEngineAdapter | null = null;
  private _backend: ActiveBackend = "uninitialized";
  private _initPromise: Promise<void> | null = null;
  private _transportUnsub: (() => void) | null = null;
  private _meterUnsub: (() => void) | null = null;
  private _meterCallbacks = new Set<MeterCallback>();
  private _pendingProject: DawProject | null = null;
  private _transport = { playing: false, positionSeconds: 0 };

  async init(preferredEngine = useSettingsStore.getState().preferredEngine): Promise<void> {
    if (this._adapter) return;
    if (this._initPromise) return this._initPromise;

    this._initPromise = this._init(preferredEngine);
    return this._initPromise;
  }

  async reconfigure(preferredEngine = useSettingsStore.getState().preferredEngine): Promise<void> {
    this.dispose();
    await this.init(preferredEngine);
  }

  dispose(): void {
    this._transportUnsub?.();
    this._meterUnsub?.();
    this._transportUnsub = null;
    this._meterUnsub = null;
    this._adapter?.dispose();
    this._adapter = null;
    this._backend = "uninitialized";
    this._initPromise = null;
    useAudioBackendStore.getState().setActive(null, {
      initialized: false,
      healthy: false,
      contextState: "uninitialized",
    });
  }

  get backend(): ActiveBackend {
    return this._backend;
  }

  get isNative(): boolean {
    return this._backend === "sphere-native";
  }

  get isPlaying(): boolean {
    return this._transport.playing;
  }

  get projectTime(): number {
    return this._transport.positionSeconds;
  }

  async play(positionSeconds?: number): Promise<void> {
    const adapter = await this._ensureAdapter();
    if (positionSeconds !== undefined) {
      this._transport.positionSeconds = Math.max(0, positionSeconds);
    }
    console.log(
      `[ActiveEngine] play() → backend: ${this._backend}, position: ${positionSeconds ?? this._transport.positionSeconds}s`,
    );
    await adapter.play(positionSeconds);
    this._setTransport({ playing: true, positionSeconds: this._transport.positionSeconds });
    this._refreshBackendStatus();

    // Native mode: metronome uses WebAudio oscillators for scheduling even though
    // clip audio comes from the Rust engine.  Resume the AudioContext so oscillators
    // can fire, then start the scheduler.
    if (this.isNative) {
      await audioEngine.resume();
      metronomeScheduler.start();
      console.log("[ActiveEngine] metronome started (native mode)");
    }
  }

  pause(): void {
    console.log(`[ActiveEngine] pause() → backend: ${this._backend}`);
    this._adapter?.pause();
    this._setTransport({ ...this._transport, playing: false });
    this._refreshBackendStatus();
    if (this.isNative) {
      metronomeScheduler.stop();
    }
  }

  stop(): void {
    console.log(`[ActiveEngine] stop() → backend: ${this._backend}`);
    this._adapter?.stop();
    this._setTransport({ playing: false, positionSeconds: 0 });
    this._refreshBackendStatus();
    if (this.isNative) {
      metronomeScheduler.stop();
    }
  }

  seekSeconds(seconds: number): void {
    const positionSeconds = Math.max(0, seconds);
    console.log(`[ActiveEngine] seekSeconds(${positionSeconds}) → backend: ${this._backend}`);
    this._adapter?.seekSeconds(positionSeconds);
    this._setTransport({ ...this._transport, positionSeconds });
    // Resync metronome beat grid to the new playhead position in native mode.
    if (this.isNative && this._transport.playing) {
      metronomeScheduler.seek();
    }
  }

  setBpm(bpm: number): void {
    this._adapter?.setBpm(bpm);
  }

  setLoop(enabled: boolean, startSeconds: number, endSeconds: number): void {
    this._adapter?.setLoop(enabled, startSeconds, endSeconds);
  }

  loadProject(project: DawProject): Promise<void> {
    this._pendingProject = project;
    if (!this._adapter) return Promise.resolve();
    return this._adapter.loadProject(project);
  }

  syncProject(project: DawProject): void {
    this._pendingProject = project;
    this._adapter?.syncProject(project);
  }

  createTrack(track: DawTrack): void {
    this._adapter?.createTrack(track);
  }

  removeTrack(trackId: TrackId): void {
    this._adapter?.removeTrack(trackId);
  }

  addInsertDevice(trackId: TrackId, device: InsertDevice): void {
    this._adapter?.addInsertDevice(trackId, device);
  }

  removeInsertDevice(trackId: TrackId, deviceId: string): void {
    this._adapter?.removeInsertDevice(trackId, deviceId);
  }

  setInsertEnabled(trackId: TrackId, deviceId: string, enabled: boolean): void {
    this._adapter?.setInsertEnabled(trackId, deviceId, enabled);
  }

  setInsertParam(trackId: TrackId, deviceId: string, param: string, value: number | string | boolean): void {
    this._adapter?.setInsertParam(trackId, deviceId, param, value);
  }

  setTrackVolume(trackId: TrackId, volume: number): void {
    this._adapter?.setTrackVolume(trackId, volume);
  }

  setTrackPan(trackId: TrackId, pan: number): void {
    this._adapter?.setTrackPan(trackId, pan);
  }

  setTrackMute(trackId: TrackId, muted: boolean): void {
    this._adapter?.setTrackMute(trackId, muted);
  }

  setTrackSolo(trackId: TrackId, solo: boolean): void {
    this._adapter?.setTrackSolo(trackId, solo);
  }

  setTrackPhaseInvert(trackId: TrackId, inverted: boolean): void {
    this._adapter?.setTrackPhaseInvert(trackId, inverted);
  }

  setMasterVolume(volume: number): void {
    this._adapter?.setMasterVolume(volume);
  }

  async syncTrackInserts(): Promise<void> {
    if (this.isNative) {
      this.syncProject(useProjectStore.getState().project);
      return;
    }
    const { mixer } = await import("./Mixer");
    const project = useProjectStore.getState().project;
    const bpm = project.bpm ?? 120;
    for (const track of project.tracks) {
      if (track.inserts && track.inserts.length > 0) {
        mixer.syncTrackInserts(track.id, track.inserts, bpm);
      }
    }
  }

  async updateClipGain(clipId: string, gain: number): Promise<void> {
    if (this.isNative) {
      this.syncProject(useProjectStore.getState().project);
      return;
    }
    const { clipScheduler } = await import("./ClipScheduler");
    clipScheduler.updateClipGain(clipId, gain);
  }

  async updateClipMute(clipId: string, muted: boolean): Promise<void> {
    if (this.isNative) {
      this.syncProject(useProjectStore.getState().project);
      return;
    }
    const { clipScheduler } = await import("./ClipScheduler");
    clipScheduler.updateClipMute(clipId, muted);
  }

  async rescheduleIfPlaying(): Promise<void> {
    if (this.isNative) {
      this.syncProject(useProjectStore.getState().project);
      return;
    }
    const { transport } = await import("./Transport");
    transport.rescheduleIfPlaying();
  }

  async runSelfTest(): Promise<AudioSelfTestResult> {
    const adapter = await this._ensureAdapter();
    if (adapter.runSelfTest) {
      const result = await adapter.runSelfTest();
      useAudioBackendStore.getState().setHealth(result.ok, result.error);
      return result;
    }
    return { ok: false, backend: String(this._backend), error: "Audio self-test is not implemented for this backend" };
  }

  subscribeMeters(callback: MeterCallback): () => void {
    this._meterCallbacks.add(callback);
    return () => this._meterCallbacks.delete(callback);
  }

  subscribeTransport(callback: TransportCallback): () => void {
    if (!this._adapter) return () => {};
    return this._adapter.subscribeTransport(callback);
  }

  private async _init(preferredEngine: PreferredEngine): Promise<void> {
    const requested = resolveRequestedBackend(preferredEngine);
    const backendStore = useAudioBackendStore.getState();
    backendStore.setRuntime(platform.kind === "electron" ? "electron" : "web");
    backendStore.setRequested(requested);

    let selection: AdapterSelection;
    try {
      selection = await createAudioEngineAdapter(preferRuntimeEngine(preferredEngine), {
        requested,
        disableFallback: readDisableFallback(),
      });
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      backendStore.setHealth(false, message);
      throw error;
    }
    this._adapter = selection.adapter;
    this._backend = selection.backend;
    backendStore.setActive(toBackendKind(selection.backend), {
      fallbackReason: selection.fallback ? selection.fallbackReason ?? "SphereAudio unavailable, using WebAudio fallback" : undefined,
      healthy: true,
    });
    backendStore.setAvailability({
      webAudio: selection.available.webAudio,
      sphereNative: selection.available.sphereNative,
    });
    console.log(`[AudioEngine] Playback backend: ${toBackendKind(selection.backend)}`);
    console.log(`[AudioEngine] DSP backend: ${selection.backend === "sphere-native" ? "sphere-native" : "rust-wasm / web-audio fallback"}`);
    console.log(`[AudioEngine] Waveform backend: ${selection.backend === "sphere-native" ? "native / worker" : "worker / wasm"}`);
    this._transportUnsub = selection.adapter.subscribeTransport((state) => {
      this._setTransport(state);
    });
    this._meterUnsub = selection.adapter.subscribeMeters((trackId, level) => {
      for (const cb of this._meterCallbacks) {
        cb(trackId, level);
      }
    });

    const project = this._pendingProject ?? useProjectStore.getState().project;
    await selection.adapter.loadProject(project);
    this._pendingProject = project;
    this._refreshBackendStatus();
  }

  private async _ensureAdapter(): Promise<AudioEngineAdapter> {
    await this.init();
    if (!this._adapter) throw new Error("Audio engine failed to initialize");
    return this._adapter;
  }

  private _setTransport(next: { playing: boolean; positionSeconds: number }): void {
    this._transport = {
      playing: next.playing,
      positionSeconds: Math.max(0, next.positionSeconds),
    };
    const store = useTransportStore.getState();
    store.setIsPlaying(this._transport.playing);
    store.setPlayheadTime(this._transport.positionSeconds);
  }

  private _refreshBackendStatus(): void {
    if (!this._adapter) return;
    useAudioBackendStore.getState().setActive(toBackendKind(this._backend), {
      contextState: this._adapter.getStatus(),
      healthy: this._adapter.getStatus() !== "error",
    });
  }
}

function preferRuntimeEngine(preferredEngine: PreferredEngine): PreferredEngine {
  if (platform.kind === "electron") return "native-sphere-direct";
  return preferredEngine === "native-sphere-direct" ? "auto" : preferredEngine;
}

export const activeAudioEngine = new ActiveAudioEngine();

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function _getSphere(): any | null {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  return (window as any).dawElectron?.sphereAudio ?? null;
}

type AudioDebugGlobal = Window & {
  __futureboardAudioDebug?: {
    runSelfTest:     () => Promise<AudioSelfTestResult>;
    getState:        () => ReturnType<typeof useAudioBackendStore.getState>;
    nativeTestTone:  (freq?: number, durationMs?: number) => Promise<void>;
    nativeTestFile:  (path: string, durationS?: number) => Promise<void>;
    nativeDumpGraph: () => Promise<unknown>;
  };
};

(window as AudioDebugGlobal).__futureboardAudioDebug = {
  runSelfTest: () => activeAudioEngine.runSelfTest(),
  getState:    () => useAudioBackendStore.getState(),

  /** Plays a sine test tone directly from the native Rust engine. */
  nativeTestTone: async (freq = 440, durationMs = 500) => {
    const sphere = _getSphere();
    if (!sphere) {
      console.warn("[NativeDebug] nativeTestTone: sphere bridge not available (not Electron?)");
      return;
    }
    console.log(`[NativeDebug] nativeTestTone: ${freq} Hz for ${durationMs} ms`);
    await sphere.setTestTone(true, freq);
    await new Promise<void>((r) => setTimeout(r, durationMs));
    await sphere.setTestTone(false, freq);
    console.log("[NativeDebug] nativeTestTone: done");
  },

  /**
   * Loads a minimal one-clip project into the native engine and plays it.
   * @param path  Absolute filesystem path to an audio file (WAV / MP3 / FLAC etc.)
   * @param durationS  How long to let it play before stopping (default 5 s).
   */
  nativeTestFile: async (path: string, durationS = 5) => {
    const sphere = _getSphere();
    if (!sphere) {
      console.warn("[NativeDebug] nativeTestFile: sphere bridge not available (not Electron?)");
      return;
    }
    console.log(`[NativeDebug] nativeTestFile: "${path}" for ${durationS}s`);
    const snapshot = {
      projectId:     "native-debug-test",
      projectRoot:   null,
      bpm:           120,
      timeSignature: [4, 4],
      sampleRate:    44100,
      tracks: [{
        id: "dbg-t1", type: "audio", volume: 1, pan: 0,
        muted: false, solo: false, armed: false,
        outputTrackId: null, inserts: [], sends: [],
      }],
      clips: [{
        id: "dbg-c1", trackId: "dbg-t1", assetId: "dbg-a1",
        mediaPath:     path,
        startBeat:     0,
        durationBeats: durationS * 2,  // at 120 bpm, 1 beat = 0.5 s
        offsetSeconds: 0,
        gain:          1,
        fades:         null,
        audioProcess:  null,
      }],
      routing: { masterOutputDevice: null, sampleRate: 44100, bufferSize: 256 },
    };
    await sphere.loadProject(snapshot);
    console.log("[NativeDebug] nativeTestFile: snapshot loaded — starting playback");
    await sphere.setTransportState({ playing: true, positionSeconds: 0 });
    await new Promise<void>((r) => setTimeout(r, durationS * 1000));
    await sphere.setTransportState({ playing: false, positionSeconds: 0 });
    console.log("[NativeDebug] nativeTestFile: stopped");
  },

  /**
   * Dumps the native engine's full runtime state to the console.
   *
   * Returns: backend, status, transport position, loaded clips/tracks, per-clip summaries.
   * Use this to verify the project was loaded and clips have decoded audio buffers.
   */
  nativeDumpGraph: async () => {
    const sphere = _getSphere();
    if (!sphere) {
      console.warn("[NativeDebug] nativeDumpGraph: sphere bridge not available (not Electron?)");
      return null;
    }
    const [status, transportState, debugInfo] = await Promise.all([
      sphere.getStatus(),
      sphere.getTransportState(),
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      typeof (sphere as any).getDebugInfo === "function" ? sphere.getDebugInfo() : Promise.resolve(null),
    ]);
    const data = {
      backend:    activeAudioEngine.backend,
      isNative:   activeAudioEngine.isNative,
      status,
      transport:  transportState,
      graph:      debugInfo,
      storeState: useAudioBackendStore.getState(),
    };
    console.group("[NativeDebug] nativeDumpGraph");
    console.log("Backend:", data.backend);
    console.log("Playing:", status.transportPlaying, "@ ", status.positionSeconds?.toFixed(3), "s");
    if (debugInfo) {
      console.log(`Graph: ${debugInfo.loadedTracks} tracks, ${debugInfo.loadedClips} clips (${debugInfo.readyClips} ready)`);
      if (debugInfo.loadedClips === 0) {
        console.warn("⚠ No clips loaded — check that audio files have resolvable filesystem paths");
      }
      if (debugInfo.clipSummaries?.length) {
        console.log("Clips:");
        debugInfo.clipSummaries.forEach((s: string) => console.log(" ", s));
      }
    }
    console.log("Full data:", data);
    console.groupEnd();
    return data;
  },
};

function toBackendKind(backend: ActiveBackend): AudioBackendKind | null {
  if (backend === "sphere-native") return "sphere-native";
  if (backend === "web-audio") return "web-audio";
  return null;
}

function resolveRequestedBackend(preferredEngine: PreferredEngine): AudioBackendRequest {
  const forced = readBackendEnv();
  if (forced === "sphere-native") return "force-native";
  if (forced === "web-audio" || forced === "rust-wasm") return "force-web";
  if (platform.kind === "electron") return "force-native";
  if (preferredEngine === "native-sphere-direct") return "force-native";
  if (preferredEngine === "webAudio" || preferredEngine === "wasm") return "force-web";
  return "auto";
}

function readBackendEnv(): "sphere-native" | "web-audio" | "rust-wasm" | "auto" {
  const env = (import.meta as unknown as { env?: Record<string, string | undefined> }).env ?? {};
  const value = env.FUTUREBOARD_AUDIO_BACKEND ?? env.VITE_FUTUREBOARD_AUDIO_BACKEND ?? "auto";
  return value === "sphere-native" || value === "web-audio" || value === "rust-wasm" ? value : "auto";
}

function readDisableFallback(): boolean {
  const env = (import.meta as unknown as { env?: Record<string, string | undefined> }).env ?? {};
  return (env.FUTUREBOARD_DISABLE_AUDIO_FALLBACK ?? env.VITE_FUTUREBOARD_DISABLE_AUDIO_FALLBACK) === "1";
}
