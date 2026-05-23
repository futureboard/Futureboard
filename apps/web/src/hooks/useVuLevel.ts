import { useState, useEffect, useRef } from "react";
import { meterStore } from "../store/meterStore";

export type StereoLevel = { l: number; r: number };

// Attack should feel instant; release decays quickly enough for DAW meters.
const ATTACK = 1.0;
const RELEASE = 0.26;

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
  const targetRef = useRef<StereoLevel>({ l: 0, r: 0 });

  useEffect(() => {
    let rafId: number;
    const unsubscribe = meterStore.subscribe(trackId, (raw) => {
      targetRef.current = {
        l: rmsToMeter(raw.peakL),
        r: rmsToMeter(raw.peakR),
      };
    });

    function tick() {
      const target = targetRef.current;

      const cur = smoothedRef.current;
      const newL = smooth(cur.l, target.l);
      const newR = smooth(cur.r, target.r);
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
    return () => {
      unsubscribe();
      cancelAnimationFrame(rafId);
    };
  }, [trackId]);

  return levels;
}
