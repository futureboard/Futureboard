const SEGMENTS = 20;

type Props = {
  mode?: "mono" | "stereo";
  /** Mono: single level. Stereo: left channel (ignored if mode mono uses max of l/r). */
  levelL: number;
  /** Right channel; mono mode collapses to max(L,R). */
  levelR: number;
  height?: number;
  /** Width of one meter column (stereo uses two columns + gap). */
  columnWidth?: number;
};

function segmentColor(i: number, on: boolean) {
  const color =
    i >= SEGMENTS - 2  ? "#f07a72" :
    i >= SEGMENTS - 5  ? "#f0c35b" :
    i >= SEGMENTS - 10 ? "#5ed8da" :
                         "#3ab5b8";
  const dim =
    i >= SEGMENTS - 2  ? "#3d1e1c" :
    i >= SEGMENTS - 5  ? "#3a2e10" :
                         "#133233";
  return on ? color : dim;
}

function MeterColumn({ level, width }: { level: number; width: number }) {
  const active = Math.round(level * SEGMENTS);
  return (
    <div
      className={`flex flex-col-reverse gap-[1.5px] h-full`}
      style={{ width }}
    >
      {Array.from({ length: SEGMENTS }, (_, i) => {
        const on = i < active;
        return (
          <div
            key={i}
            className="flex-1 rounded-[1px]"
            style={{ background: segmentColor(i, on) }}
          />
        );
      })}
    </div>
  );
}

export function VuMeter({
  mode = "mono",
  levelL,
  levelR,
  height,
  columnWidth = 5,
}: Props) {
  const monoLevel = Math.max(levelL, levelR);

  return (
    <div
      className={`flex gap-[2px] ${height === undefined ? "h-full" : ""}`}
      style={height !== undefined ? { height } : undefined}
    >
      {mode === "mono" ? (
        <MeterColumn level={monoLevel} width={columnWidth} />
      ) : (
        <>
          <MeterColumn level={levelL} width={columnWidth} />
          <MeterColumn level={levelR} width={columnWidth} />
        </>
      )}
    </div>
  );
}
