import type { WaveformCacheAdapter } from "./types";
import { WebWaveformCache } from "./WebWaveformCache";
import { ElectronWaveformCache } from "./ElectronWaveformCache";
import { MemoryWaveformCache } from "./MemoryWaveformCache";

export * from "./types";

function createAdapter(): WaveformCacheAdapter {
  const isElectron =
    typeof window !== "undefined" &&
    typeof (window as unknown as { dawElectron?: unknown }).dawElectron !== "undefined";

  if (isElectron) return new ElectronWaveformCache();

  if (typeof indexedDB !== "undefined") return new WebWaveformCache();

  return new MemoryWaveformCache();
}

export const waveformCache: WaveformCacheAdapter = createAdapter();

export { MemoryWaveformCache, WebWaveformCache, ElectronWaveformCache };
