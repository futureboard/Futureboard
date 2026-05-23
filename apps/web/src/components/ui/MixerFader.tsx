/**
 * Professional-style vertical mixer fader.
 *
 * Coordinate convention
 *   t ∈ [0, 1]  —  0 = bottom (-60 dB / -∞),  1 = top (0 dB)
 *   thumb top   = (1 - t) * (100% - THUMB_H px)
 *   scale mark  = same formula +  translateY(-50%)  →  centered on thumb center
 *
 * Interaction
 *   • Drag thumb — normal speed
 *   • Shift + drag — fine (0.1×)
 *   • Double-click thumb — reset to unity (1.0 / 0 dB)
 *   • Wheel — ±1 dB; Shift+wheel — ±0.2 dB
 */

import { useEffect, useRef } from "react";
import { VuMeter } from "./VuMeter";

// ── dB / linear helpers ───────────────────────────────────────────────────────

export const FADER_MIN_DB = -60;
export const FADER_MAX_DB = 0;   // unity — no gain boost for now

export function linearToDb(v: number): number {
  if (v <= 0.001) return -Infinity;
  return 20 * Math.log10(v);
}

export function dbToLinear(db: number): number {
  if (!isFinite(db) || db <= FADER_MIN_DB) return 0;
  return Math.pow(10, db / 20);
}

function dbToT(db: number): number {
  if (!isFinite(db) || db <= FADER_MIN_DB) return 0;
  return Math.max(0, Math.min(1, (db - FADER_MIN_DB) / (FADER_MAX_DB - FADER_MIN_DB)));
}
function tToDb(t: number): number {
  return FADER_MIN_DB + t * (FADER_MAX_DB - FADER_MIN_DB);
}
function volumeToT(v: number): number { return dbToT(linearToDb(v)); }
function tToVolume(t: number): number {
  return Math.max(0, Math.min(1, dbToLinear(tToDb(Math.max(0, Math.min(1, t))))));
}

export function formatDbDisplay(v: number): string {
  if (v <= 0.001) return "-∞";
  const db = linearToDb(v);
  if (Math.abs(db) < 0.05) return "0.00";
  if (db > 0) return `+${db.toFixed(1)}`;
  return db.toFixed(1);
}

// ── Scale marks ───────────────────────────────────────────────────────────────

const SCALE_MARKS: { db: number; label: string }[] = [
  { db:  0,  label: "0"   },
  { db: -6,  label: "6"   },
  { db: -12, label: "12"  },
  { db: -18, label: "18"  },
  { db: -24, label: "24"  },
  { db: -36, label: "36"  },
  { db: -54, label: "∞"   },
];

// ── Geometry ──────────────────────────────────────────────────────────────────

const THUMB_H  = 10;   // px — rectangular fader cap height
const RAIL_W   = 3;    // px

/**
 * Position an element so that its CENTRE aligns with the thumb centre at `t`.
 *   top = (1 - t) * (100% - THUMB_H px) + THUMB_H/2 px
 * Combined with translateY(-50%) this centres the element on that y.
 */
function thumbCenterStyle(t: number): React.CSSProperties {
  return {
    top: `calc(${(1 - t).toFixed(5)} * (100% - ${THUMB_H}px) + ${THUMB_H / 2}px)`,
    transform: "translateY(-50%)",
  };
}

/**
 * Top edge of the fader thumb (no translate correction needed — raw position).
 *   top = (1 - t) * (100% - THUMB_H px)
 */
function thumbTopStyle(t: number): React.CSSProperties {
  return {
    top: `calc(${(1 - t).toFixed(5)} * (100% - ${THUMB_H}px))`,
  };
}

// ── Props ──────────────────────────────────────────────────────────────────────

export type MixerFaderProps = {
  value: number;               // linear 0..1
  levelL?: number;             // VU meter left
  levelR?: number;             // VU meter right
  meterTrackId?: string | "master";
  meterMode?: "mono" | "stereo";
  muted?: boolean;
  solo?: boolean;
  isMaster?: boolean;
  color?: string;
  showTrackButtons?: boolean;
  onChange:  (v: number) => void;
  onCommit?: (v: number) => void;
  onMute?:   () => void;
  onSolo?:   () => void;
};

// ── Component ─────────────────────────────────────────────────────────────────

export function MixerFader({
  value,
  levelL = 0,
  levelR = 0,
  meterTrackId,
  meterMode = "mono",
  muted,
  solo,
  isMaster = false,
  showTrackButtons = true,
  onChange,
  onCommit,
  onMute,
  onSolo,
}: MixerFaderProps) {
  const trackRef = useRef<HTMLDivElement>(null);

  // Keep latest callbacks in refs to avoid stale closure in non-React handlers.
  const latestV         = useRef(value);
  latestV.current       = value;
  const onChangeRef     = useRef(onChange);
  onChangeRef.current   = onChange;
  const onCommitRef     = useRef(onCommit);
  onCommitRef.current   = onCommit;

  // ── drag ──────────────────────────────────────────────────────────────────

  const drag = useRef<{ lastY: number; lastT: number } | null>(null);

  const getUsable = (): number => {
    const h = trackRef.current?.offsetHeight ?? 200;
    return Math.max(1, h - THUMB_H);
  };

  const onPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    drag.current = { lastY: e.clientY, lastT: volumeToT(latestV.current) };
  };

  const onPointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!drag.current) return;
    const usable = getUsable();
    const dy     = e.clientY - drag.current.lastY;
    const speed  = e.shiftKey ? 0.1 : 1;
    const t      = Math.max(0, Math.min(1, drag.current.lastT - (dy / usable) * speed));
    drag.current  = { lastY: e.clientY, lastT: t };
    onChangeRef.current(Math.round(tToVolume(t) * 10000) / 10000);
  };

  const onPointerUp = () => {
    if (drag.current) {
      onCommitRef.current?.(latestV.current);
      drag.current = null;
    }
  };

  const onDoubleClick = () => {
    onChangeRef.current(1.0);
    onCommitRef.current?.(1.0);
  };

  // ── wheel (non-passive so we can preventDefault) ──────────────────────────

  useEffect(() => {
    const el = trackRef.current;
    if (!el) return;
    const handler = (e: WheelEvent) => {
      e.preventDefault();
      const cur  = latestV.current;
      const db   = linearToDb(cur);
      const step = e.shiftKey ? 0.2 : 1.0;
      const next_db = Math.max(
        FADER_MIN_DB,
        Math.min(FADER_MAX_DB, (isFinite(db) ? db : FADER_MIN_DB) - Math.sign(e.deltaY) * step),
      );
      onChangeRef.current(Math.round(Math.max(0, Math.min(1, dbToLinear(next_db))) * 10000) / 10000);
    };
    el.addEventListener("wheel", handler, { passive: false });
    return () => el.removeEventListener("wheel", handler);
  }, []);

  // ── render ────────────────────────────────────────────────────────────────

  const t           = volumeToT(value);
  const meterWidth  = meterMode === "stereo" ? 13 : 6;

  return (
    <div className="flex min-h-0 flex-1 select-none flex-col">
      {/* ── dB readout ── */}
      <div className="flex shrink-0 items-baseline justify-center gap-[3px] pb-[3px]">
        <span
          className="tabular-nums text-[10px] font-medium leading-none"
          style={{ color: "rgba(238,242,245,0.72)" }}
        >
          {formatDbDisplay(value)}
        </span>
        <span className="text-[7.5px] leading-none" style={{ color: "rgba(255,255,255,0.22)" }}>
          dB
        </span>
      </div>

      {/* ── scale + fader + meter ── */}
      <div className="relative flex min-h-0 flex-1 gap-1.5 overflow-hidden">

        {/* dB scale column — label centres aligned with thumb centres */}
        <div className="relative shrink-0 w-[15px] pointer-events-none">
          {SCALE_MARKS.map(({ db, label }) => (
            <span
              key={db}
              className="absolute right-0 tabular-nums leading-none"
              style={{
                ...thumbCenterStyle(dbToT(db)),
                fontSize: "7.5px",
                color: db === 0 ? "rgba(255,255,255,0.35)" : "rgba(255,255,255,0.18)",
              }}
            >
              {label}
            </span>
          ))}
        </div>

        {/* fader track — drag target */}
        <div
          ref={trackRef}
          className="relative flex-1"
          style={{ touchAction: "none", cursor: "ns-resize" }}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onPointerCancel={onPointerUp}
          onDoubleClick={onDoubleClick}
        >
          {/* Rail — spans from thumb-centre-top to thumb-centre-bottom */}
          <div
            className="absolute left-1/2 -translate-x-1/2 rounded-full"
            style={{
              top:        THUMB_H / 2,
              bottom:     THUMB_H / 2,
              width:      RAIL_W,
              background: "rgba(255,255,255,0.06)",
            }}
          />

          {/* Tick marks at labelled dB positions */}
          {SCALE_MARKS.map(({ db }) => (
            <div
              key={db}
              className="absolute left-1/2 pointer-events-none"
              style={{
                ...thumbCenterStyle(dbToT(db)),
                width:      db === 0 ? 13 : 9,
                height:     1,
                marginLeft: db === 0 ? -6.5 : -4.5,
                background: db === 0
                  ? "rgba(255,255,255,0.30)"
                  : "rgba(255,255,255,0.12)",
              }}
            />
          ))}

          {/* Fader thumb */}
          <div
            className="absolute left-1/2 z-10"
            style={{
              ...thumbTopStyle(t),
              width:        24,
              height:       THUMB_H,
              marginLeft:   -12,
              background:   "linear-gradient(180deg, rgba(255,255,255,0.22) 0%, rgba(255,255,255,0.08) 100%)",
              border:       "1px solid rgba(255,255,255,0.22)",
              borderRadius: 2,
              boxShadow:    "0 2px 5px rgba(0,0,0,0.45), 0 1px 0 rgba(255,255,255,0.06) inset",
              cursor:       "ns-resize",
            }}
          >
            {/* two grip lines */}
            <div
              className="absolute inset-x-[5px]"
              style={{ top: 3, height: 1, background: "rgba(255,255,255,0.32)" }}
            />
            <div
              className="absolute inset-x-[5px]"
              style={{ top: 5, height: 1, background: "rgba(255,255,255,0.16)" }}
            />
          </div>
        </div>

        {/* Meter */}
        <div className="flex shrink-0 items-stretch" style={{ width: meterWidth }}>
          <VuMeter
            mode={meterMode}
            levelL={levelL}
            levelR={levelR}
            meterTrackId={meterTrackId}
            columnWidth={meterMode === "stereo" ? 5 : 6}
          />
        </div>
      </div>

      {/* ── M / S  or  "master" label ── */}
      {isMaster ? (
        <div className="shrink-0 py-1.5 text-center">
          <span
            className="text-[8px] font-semibold uppercase tracking-[0.18em]"
            style={{ color: "rgba(255,255,255,0.22)" }}
          >
            master
          </span>
        </div>
      ) : showTrackButtons ? (
        <div className="flex shrink-0 gap-1 px-1 py-1.5">
          <button
            type="button"
            title="Mute"
            onClick={onMute}
            className="h-[18px] flex-1 rounded-[3px] text-[9px] font-bold tracking-wide transition-colors"
            style={{
              background: muted ? "#f0c35b" : "rgba(255,255,255,0.03)",
              border:     `1px solid ${muted ? "#f0c35b" : "rgba(255,255,255,0.09)"}`,
              color:      muted ? "#0d1015" : "rgba(220,232,240,0.45)",
            }}
          >
            M
          </button>
          <button
            type="button"
            title="Solo"
            onClick={onSolo}
            className="h-[18px] flex-1 rounded-[3px] text-[9px] font-bold tracking-wide transition-colors"
            style={{
              background: solo ? "#7ccf86" : "rgba(255,255,255,0.03)",
              border:     `1px solid ${solo ? "#7ccf86" : "rgba(255,255,255,0.09)"}`,
              color:      solo ? "#0d1015" : "rgba(220,232,240,0.45)",
            }}
          >
            S
          </button>
        </div>
      ) : null}
    </div>
  );
}
