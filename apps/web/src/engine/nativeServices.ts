// Tracks whether native services (Sphere audio, peak service) have settled.
// openProjectFromPath awaits whenReadyOrSettled() before reading peak data,
// preventing re-entrant store mutations on the same synchronous tick as boot.
// Auto-settles on the next microtask — IPC handlers are registered at
// main-process module-load time so no actual wait is needed in practice.

let _settled = false;
const _waiters: Array<() => void> = [];

function settle(): void {
  if (_settled) return;
  _settled = true;
  _waiters.splice(0).forEach((r) => r());
}

// Defer settle to next microtask so callers always get an async boundary.
Promise.resolve().then(settle);

export const nativeServices = {
  /** Call when the native audio service confirms it is running. */
  markReady: settle,
  /** Call when native service startup failed or is unavailable. */
  markSettled: settle,
  /** Resolves as soon as native services are ready or after the auto-settle. */
  whenReadyOrSettled(): Promise<void> {
    if (_settled) return Promise.resolve();
    return new Promise<void>((resolve) => _waiters.push(resolve));
  },
};
