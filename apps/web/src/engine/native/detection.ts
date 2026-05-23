/**
 * detectAudioEngineBackends — probes what audio backends are available in the
 * current runtime (Web Browser vs Electron).
 *
 * Rules:
 *   Web:      native-sphere-direct is always unavailable.
 *   Electron: checks window.dawElectron.sphereAudio.getStatus().
 *             A probe failure → unavailable, never throws.
 *
 * Results are cached after the first successful probe.
 * Call invalidateBackendDetection() to force a fresh probe.
 */
import type { AudioEngineBackendStatus } from "./types";

let _cached: AudioEngineBackendStatus[] | null = null;

export async function detectAudioEngineBackends(): Promise<AudioEngineBackendStatus[]> {
  if (_cached) return _cached;

  const results: AudioEngineBackendStatus[] = [];

  // ── WebAudio ────────────────────────────────────────────────────────────────
  const hasWebAudio =
    typeof AudioContext !== "undefined" ||
    typeof (window as unknown as Record<string, unknown>)["webkitAudioContext"] !== "undefined";

  results.push({
    backend:  "web-audio",
    available: hasWebAudio,
    running:  false,
    reason:   hasWebAudio ? undefined : "AudioContext not supported in this browser",
  });

  // ── SphereDirectAudioEngine (Electron only) ──────────────────────────────────
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const sphereBridge = (window as any).dawElectron?.sphereAudio;

  if (!sphereBridge) {
    results.push({
      backend:  "native-sphere-direct",
      available: false,
      running:  false,
      reason:   "SphereDirectAudioEngine not available (requires Electron client)",
    });
  } else {
    try {
      const [status, version] = await Promise.all([
        sphereBridge.getStatus() as Promise<{
          available:    boolean;
          running:      boolean;
          sampleRate:   number;
          bufferSize:   number;
          inputDevice:  string | null;
          outputDevice: string | null;
          lastError:    string | null;
        }>,
        sphereBridge.getVersion() as Promise<string>,
      ]);

      results.push({
        backend:      "native-sphere-direct",
        // status.available is false when the native addon failed to load
        // (the IPC handler is always registered but returns a "not available"
        // placeholder when the .node file couldn't be found or loaded).
        available:    status.available,
        running:      status.running,
        version,
        sampleRate:   status.sampleRate,
        bufferSize:   status.bufferSize,
        inputDevice:  status.inputDevice  ?? undefined,
        outputDevice: status.outputDevice ?? undefined,
        reason:       status.available ? undefined
          : (status.lastError ?? "Native addon failed to load"),
      });
    } catch (e) {
      // Log once — not a spam source.
      console.warn("[AudioBackend] SphereDirectAudioEngine probe failed:", e);
      results.push({
        backend:  "native-sphere-direct",
        available: false,
        running:  false,
        reason:   `Native engine unreachable: ${e instanceof Error ? e.message : String(e)}`,
      });
    }
  }

  _cached = results;
  return results;
}

/** Force re-detection on the next detectAudioEngineBackends() call. */
export function invalidateBackendDetection(): void {
  _cached = null;
}

/** Synchronous cache read — returns null if detection hasn't run yet. */
export function getCachedBackendStatus(
  backend: AudioEngineBackendStatus["backend"],
): AudioEngineBackendStatus | null {
  return _cached?.find((s) => s.backend === backend) ?? null;
}
