import { useEffect, useRef } from "react";
import {
  categories,
  chainOrder,
  icons,
  type CategoryId,
} from "../data";

type VuLevels = {
  inL: number;
  inR: number;
  outL: number;
  outR: number;
};

type SignalChainProps = {
  activeCat: CategoryId;
  bypassed: Partial<Record<CategoryId, boolean>>;
  vu: VuLevels;
  onSelectCategory: (cat: CategoryId) => void;
};

function IoMeter({
  title,
  left,
  right,
}: {
  title: string;
  left: number;
  right: number;
}) {
  return (
    <div className="io-meter" title={title}>
      <div className="io-bar">
        <div className="io-fill" style={{ height: `${left * 100}%` }} />
      </div>
      <div className="io-bar">
        <div className="io-fill" style={{ height: `${right * 100}%` }} />
      </div>
    </div>
  );
}

export function SignalChain({
  activeCat,
  bypassed,
  vu,
  onSelectCategory,
}: SignalChainProps) {
  const svgRef = useRef<SVGSVGElement>(null);
  const rowRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const draw = () => {
      const svg = svgRef.current;
      const row = rowRef.current;
      if (!svg || !row) return;
      while (svg.firstChild) svg.removeChild(svg.firstChild);

      const nodes = row.querySelectorAll<HTMLElement>(".module");
      if (nodes.length < 2) return;
      const box = svg.getBoundingClientRect();

      for (let i = 0; i < nodes.length - 1; i++) {
        const a = nodes[i]!.getBoundingClientRect();
        const b = nodes[i + 1]!.getBoundingClientRect();
        const x1 = a.right - box.left;
        const y1 = a.top + a.height / 2 - box.top;
        const x2 = b.left - box.left;
        const y2 = b.top + b.height / 2 - box.top;
        const dx = (x2 - x1) * 0.5;
        const path = document.createElementNS(
          "http://www.w3.org/2000/svg",
          "path",
        );
        path.setAttribute(
          "d",
          `M ${x1} ${y1} C ${x1 + dx} ${y1}, ${x2 - dx} ${y2}, ${x2} ${y2}`,
        );
        path.setAttribute("fill", "none");
        const dim =
          nodes[i]!.classList.contains("bypassed") ||
          nodes[i + 1]!.classList.contains("bypassed");
        path.setAttribute(
          "stroke",
          dim ? "rgba(255,255,255,0.04)" : "rgba(255,255,255,0.11)",
        );
        path.setAttribute("stroke-width", "1.5");
        svg.appendChild(path);
      }
    };

    draw();
    const t = window.setTimeout(draw, 60);
    window.addEventListener("resize", draw);
    return () => {
      window.clearTimeout(t);
      window.removeEventListener("resize", draw);
    };
  }, [activeCat, bypassed]);

  return (
    <section className="chain">
      <span className="chain-title">Signal Path</span>
      <svg className="chain-svg" ref={svgRef} />
      <div className="chain-row" ref={rowRef} id="chain-row">
        <IoMeter title="Input" left={vu.inL} right={vu.inR} />
        {chainOrder.map((cat) => {
          const c = categories[cat];
          const selected = cat === activeCat;
          const isBypassed = !!bypassed[cat];
          return (
            <div
              key={cat}
              className={`module${selected ? " selected" : ""}${isBypassed ? " bypassed" : ""}`}
              style={{ ["--mc" as string]: c.color }}
              onClick={() => onSelectCategory(cat)}
            >
              <span className="accent" />
              <div className="ic">
                <svg
                  width="17"
                  height="17"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  dangerouslySetInnerHTML={{ __html: icons[c.node] ?? "" }}
                />
              </div>
              <span className="mtitle">{c.short}</span>
            </div>
          );
        })}
        <IoMeter title="Output" left={vu.outL} right={vu.outR} />
      </div>
    </section>
  );
}
