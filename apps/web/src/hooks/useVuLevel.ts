import { useState, useEffect } from "react";
import { mixer, type StereoLevel } from "../engine/Mixer";

/**
 * Reads per-channel RMS for a mixer track or master at ~60 fps.
 * Scale: -60 dBFS → 0.0 (silence), 0 dBFS → 1.0 (full scale).
 */
export function useVuStereoLevels(trackId: string | "master"): StereoLevel {
  const [levels, setLevels] = useState<StereoLevel>({ l: 0, r: 0 });

  useEffect(() => {
    let rafId: number;

    function tick() {
      const raw =
        trackId === "master" ? mixer.getMasterLevel() : mixer.getLevel(trackId);
      setLevels({
        l: rmsToMeter(raw.l),
        r: rmsToMeter(raw.r),
      });
      rafId = requestAnimationFrame(tick);
    }

    rafId = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(rafId);
  }, [trackId]);

  return levels;
}

/** Map RMS amplitude → 0-1 meter level using a 60 dB range. */
function rmsToMeter(rms: number): number {
  if (rms < 0.000001) return 0;
  const db = 20 * Math.log10(rms);
  return Math.max(0, Math.min(1, (db + 60) / 60));
}
