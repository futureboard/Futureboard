/**
 * AudioDeviceService — enumerates browser audio I/O devices.
 *
 * Web path:  navigator.mediaDevices.enumerateDevices()
 *            Permission required for labeled results (labels are blank until granted).
 * Electron:  same Web API for now; native device list via IPC will replace later.
 *
 * Usage:
 *   await audioDeviceService.requestAudioPermission();
 *   await audioDeviceService.refreshAudioDevices();
 *   audioDeviceService.getAudioInputs(); // from deviceStore snapshot
 */
import { useDeviceStore } from "../store/deviceStore";
import type { AudioDeviceInfo, AudioPermissionState } from "../store/deviceStore";

function store() {
  return useDeviceStore.getState();
}

class AudioDeviceService {
  private _deviceChangeUnlisten: (() => void) | null = null;

  /** Parse a raw MediaDeviceInfo into our typed model. */
  private parseDevice(d: MediaDeviceInfo, defaultId: string): AudioDeviceInfo {
    const name = d.label || (d.kind === "audioinput" ? "Microphone" : "Speaker");
    return {
      id: d.deviceId,
      name,
      kind: d.kind as "audioinput" | "audiooutput",
      isDefault: d.deviceId === "default" || d.deviceId === defaultId,
    };
  }

  /**
   * Enumerate devices and update deviceStore.
   * Labels will be empty strings until audio permission has been granted.
   */
  async refreshAudioDevices(): Promise<void> {
    if (!navigator.mediaDevices?.enumerateDevices) {
      store().setAudioDevices([], []);
      return;
    }
    try {
      const raw = await navigator.mediaDevices.enumerateDevices();

      const defaultInput = raw.find((d) => d.kind === "audioinput" && d.deviceId === "default");
      const defaultOutput = raw.find((d) => d.kind === "audiooutput" && d.deviceId === "default");

      const inputs: AudioDeviceInfo[] = raw
        .filter((d) => d.kind === "audioinput" && d.deviceId !== "default")
        .map((d) => this.parseDevice(d, defaultInput?.groupId ?? ""));

      const outputs: AudioDeviceInfo[] = raw
        .filter((d) => d.kind === "audiooutput" && d.deviceId !== "default")
        .map((d) => this.parseDevice(d, defaultOutput?.groupId ?? ""));

      store().setAudioDevices(inputs, outputs);
    } catch {
      store().setAudioDevices([], []);
    }
  }

  /**
   * Request microphone permission. This causes the browser to show the
   * permission prompt and — if granted — makes device labels visible.
   */
  async requestAudioPermission(): Promise<AudioPermissionState> {
    if (!navigator.mediaDevices?.getUserMedia) {
      store().setAudioPermission("denied");
      return "denied";
    }

    store().setAudioPermission("prompting");
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true, video: false });
      // Stop all tracks immediately — we only needed the permission grant.
      stream.getTracks().forEach((t) => t.stop());
      store().setAudioPermission("granted");
      await this.refreshAudioDevices();
      return "granted";
    } catch {
      store().setAudioPermission("denied");
      return "denied";
    }
  }

  /** Start listening for device plug/unplug events. Call once on app init. */
  listenForDeviceChanges(): void {
    if (!navigator.mediaDevices?.addEventListener) return;
    if (this._deviceChangeUnlisten) return;

    const handler = () => { this.refreshAudioDevices(); };
    navigator.mediaDevices.addEventListener("devicechange", handler);
    this._deviceChangeUnlisten = () =>
      navigator.mediaDevices.removeEventListener("devicechange", handler);
  }

  stopListening(): void {
    this._deviceChangeUnlisten?.();
    this._deviceChangeUnlisten = null;
  }

  // ── Accessors (snapshots from the store) ──────────────────────────────────

  getAudioInputs(): AudioDeviceInfo[] {
    return store().audioInputs;
  }

  getAudioOutputs(): AudioDeviceInfo[] {
    return store().audioOutputs;
  }

  getDefaultInput(): AudioDeviceInfo | null {
    return store().audioInputs.find((d) => d.isDefault) ?? store().audioInputs[0] ?? null;
  }

  getDefaultOutput(): AudioDeviceInfo | null {
    return store().audioOutputs.find((d) => d.isDefault) ?? store().audioOutputs[0] ?? null;
  }
}

export const audioDeviceService = new AudioDeviceService();
