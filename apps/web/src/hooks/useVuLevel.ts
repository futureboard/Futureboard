import { useState, useEffect, useRef } from "react";
import { mixer, type StereoLevel } from "../engine/Mixer";

// Attack is fast (85% toward target per frame), release is slow (10% per frame).
const ATTACK = 0.85;
const RELEASE = 0.10;

function smooth(current: number, target: number): number {
  const coeff = target > current ? ATTACK : RELEASE;
  return current + coeff * (target - current);
}

function rmsToMeter(rms: number): number {
  if (rms < 0.000001) return 0;
  const db = 20 * Math.log10(rms);
  return Math.max(0, Math.min(1, (db + 60) / 60));
}

export function useVuStereoLevels(trackId: string | "master"): StereoLevel {
  const [levels, setLevels] = useState<StereoLevel>({ l: 0, r: 0 });
  const smoothedRef = useRef<StereoLevel>({ l: 0, r: 0 });

  useEffect(() => {
    let rafId: number;

    function tick() {
      const raw =
        trackId === "master" ? mixer.getMasterLevel() : mixer.getLevel(trackId);
      const targetL = rmsToMeter(raw.l);
      const targetR = rmsToMeter(raw.r);

      const cur = smoothedRef.current;
      const newL = smooth(cur.l, targetL);
      const newR = smooth(cur.r, targetR);
      smoothedRef.current = { l: newL, r: newR };

      // Skip React re-render if the visual change is sub-pixel (< 0.002 on 0–1 scale).
      setLevels((prev) => {
        if (Math.abs(prev.l - newL) < 0.002 && Math.abs(prev.r - newR) < 0.002)
          return prev;
        return { l: newL, r: newR };
      });

      rafId = requestAnimationFrame(tick);
    }

    rafId = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(rafId);
  }, [trackId]);

  return levels;
}
