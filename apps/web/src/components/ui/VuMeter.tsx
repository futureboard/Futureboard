const SEGMENTS = 16;

type Props = { level?: number; height?: number; width?: number };

export function VuMeter({ level = 0, height = 64, width = 5 }: Props) {
  const active = Math.round(level * SEGMENTS);
  return (
    <div className="flex flex-col-reverse gap-px" style={{ width, height }}>
      {Array.from({ length: SEGMENTS }, (_, i) => {
        const on = i < active;
        const color = i >= SEGMENTS - 2 ? "#f06a61" : i >= SEGMENTS - 5 ? "#e0b24d" : "#63c174";
        return <div key={i} className="flex-1 rounded-[1px]" style={{ background: on ? color : "#20262d" }} />;
      })}
    </div>
  );
}
