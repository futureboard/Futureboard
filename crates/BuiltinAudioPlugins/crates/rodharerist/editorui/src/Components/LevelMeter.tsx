import { useEffect, useRef } from "react";
import { formatDbfs, linearToDbfs, meterPosition } from "../globals";
import {
  getHold,
  subscribeSmoothedMeters,
  type SmoothedLevels,
} from "../state/meters";

type LevelMeterProps = {
  /** Which end of the chain to display. */
  side: "in" | "out";
  label: string;
};

/**
 * A compact horizontal level meter with an RMS bar, a peak overlay and a
 * peak-hold tick.
 *
 * Painted from the rAF-timed ballistic meter view ({@link subscribeSmoothedMeters})
 * so the bars glide at display rate instead of stepping at whatever cadence
 * telemetry frames actually arrive. Writes straight to its own DOM nodes — it
 * never calls `setState`, so meter motion re-renders nothing else. It renders
 * exactly once per mount.
 *
 * The DSP reports one peak/RMS pair across both channels rather than per-channel
 * levels, so this is a single bar. It is not drawn as a stereo pair, because
 * that would imply channel information the engine does not currently provide.
 */
export function LevelMeter({ side, label }: LevelMeterProps) {
  const rmsRef = useRef<HTMLDivElement>(null);
  const peakRef = useRef<HTMLDivElement>(null);
  const holdRef = useRef<HTMLSpanElement>(null);
  const readoutRef = useRef<HTMLSpanElement>(null);

  useEffect(() => {
    const paint = (levels: SmoothedLevels) => {
      const peakDb = side === "in" ? levels.inPeakDb : levels.outPeakDb;
      const rmsDb = side === "in" ? levels.inRmsDb : levels.outRmsDb;
      const hold = side === "in" ? getHold().in : getHold().out;

      if (rmsRef.current) {
        rmsRef.current.style.width = `${meterPosition(rmsDb) * 100}%`;
      }
      if (peakRef.current) {
        peakRef.current.style.width = `${meterPosition(peakDb) * 100}%`;
      }
      if (holdRef.current) {
        const p = meterPosition(linearToDbfs(hold));
        holdRef.current.style.left = `${p * 100}%`;
        holdRef.current.style.opacity = p > 0 ? "1" : "0";
      }
      if (readoutRef.current) {
        readoutRef.current.textContent = formatDbfs(peakDb);
      }
    };

    return subscribeSmoothedMeters(paint);
  }, [side]);

  return (
    <div className="lvl">
      <span className="lvl-label">{label}</span>
      <div
        className="lvl-track"
        role="meter"
        aria-label={`${label} level`}
        title={`${label} level — peak over RMS, dBFS`}
      >
        <div ref={rmsRef} className="lvl-rms" style={{ width: 0 }} />
        <div ref={peakRef} className="lvl-peak" style={{ width: 0 }} />
        <span ref={holdRef} className="lvl-hold" style={{ left: 0, opacity: 0 }} />
      </div>
      <span className="lvl-readout">
        <span ref={readoutRef}>-∞</span>
        <span className="lvl-unit">dBFS</span>
      </span>
    </div>
  );
}
