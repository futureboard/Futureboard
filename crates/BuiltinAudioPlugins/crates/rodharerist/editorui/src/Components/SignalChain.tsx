import { useEffect, useRef, useState } from "react";
import {
  categories,
  icons,
  models,
  rackFromPath,
  type CategoryId,
} from "../data";

type VuLevels = {
  inL: number;
  inR: number;
  outL: number;
  outR: number;
};

type SignalChainProps = {
  pathOrder: CategoryId[];
  activeCat: CategoryId;
  stageModels: Record<CategoryId, string>;
  bypassed: Partial<Record<CategoryId, boolean>>;
  vu: VuLevels;
  onSelectCategory: (cat: CategoryId) => void;
  onToggleModule: (cat: CategoryId) => void;
  onReorderPath: (next: CategoryId[]) => void;
};

function modelLabel(cat: CategoryId, modelId: string): string {
  const list = models[cat] ?? [];
  const found = list.find((m) => m.id === modelId) ?? list[0];
  return found?.short ?? found?.name ?? "—";
}

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
    <div className="io-col">
      <div className="io-meter" title={title}>
        <div className="io-bar">
          <div className="io-fill" style={{ height: `${left * 100}%` }} />
        </div>
        <div className="io-bar">
          <div className="io-fill" style={{ height: `${right * 100}%` }} />
        </div>
      </div>
      <span className="io-cap">{title}</span>
    </div>
  );
}

export function SignalChain({
  pathOrder,
  activeCat,
  stageModels,
  bypassed,
  vu,
  onSelectCategory,
  onToggleModule,
  onReorderPath,
}: SignalChainProps) {
  const svgRef = useRef<SVGSVGElement>(null);
  const rowRef = useRef<HTMLDivElement>(null);
  const [dragCat, setDragCat] = useState<CategoryId | null>(null);
  const [dragFromRack, setDragFromRack] = useState(false);
  const rack = rackFromPath(pathOrder);

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
        const x1 = a.right - box.left + 2;
        const y1 = a.top + a.height / 2 - box.top;
        const x2 = b.left - box.left - 2;
        const y2 = b.top + b.height / 2 - box.top;
        const path = document.createElementNS(
          "http://www.w3.org/2000/svg",
          "path",
        );
        path.setAttribute("d", `M ${x1} ${y1} L ${x2} ${y2}`);
        path.setAttribute("fill", "none");
        const dim =
          nodes[i]!.classList.contains("bypassed") ||
          nodes[i + 1]!.classList.contains("bypassed");
        path.setAttribute(
          "stroke",
          dim ? "rgba(255,255,255,0.06)" : "rgba(255,255,255,0.18)",
        );
        path.setAttribute("stroke-width", "2");
        path.setAttribute("stroke-linecap", "square");
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
  }, [activeCat, bypassed, pathOrder, stageModels]);

  const moveInPath = (cat: CategoryId, dir: -1 | 1) => {
    const i = pathOrder.indexOf(cat);
    if (i < 0) return;
    const j = i + dir;
    if (j < 0 || j >= pathOrder.length) return;
    const next = [...pathOrder];
    const tmp = next[i]!;
    next[i] = next[j]!;
    next[j] = tmp;
    onReorderPath(next);
  };

  const removeFromPath = (cat: CategoryId) => {
    onReorderPath(pathOrder.filter((c) => c !== cat));
  };

  const addToPath = (cat: CategoryId, at?: number) => {
    if (pathOrder.includes(cat)) return;
    const next = [...pathOrder];
    if (at === undefined || at < 0 || at > next.length) next.push(cat);
    else next.splice(at, 0, cat);
    onReorderPath(next);
  };

  const onDropAt = (index: number) => {
    if (!dragCat) return;
    if (dragFromRack) {
      addToPath(dragCat, index);
    } else {
      const without = pathOrder.filter((c) => c !== dragCat);
      const clamped = Math.max(0, Math.min(index, without.length));
      without.splice(clamped, 0, dragCat);
      onReorderPath(without);
    }
    setDragCat(null);
    setDragFromRack(false);
  };

  return (
    <section className="chain">
      <span className="chain-title">Path</span>
      <span className="chain-hint">Drag to reorder · × remove · rack adds back</span>
      <svg className="chain-svg" ref={svgRef} />
      <div className="chain-row" ref={rowRef} id="chain-row">
        <IoMeter title="In" left={vu.inL} right={vu.inR} />

        <div
          className="drop-zone"
          onDragOver={(e) => e.preventDefault()}
          onDrop={() => onDropAt(0)}
        />

        {pathOrder.length === 0 && (
          <div className="path-empty">Empty path — add blocks from the rack</div>
        )}

        {pathOrder.map((cat, index) => {
          const c = categories[cat];
          const selected = cat === activeCat;
          const isBypassed = !!bypassed[cat];
          const mid = stageModels[cat] ?? models[cat][0]?.id ?? "";
          const label = modelLabel(cat, mid);
          return (
            <div key={cat} className="module-wrap">
              <div
                className={`module${selected ? " selected" : ""}${isBypassed ? " bypassed" : ""}`}
                style={{ ["--mc" as string]: c.color }}
                draggable
                onDragStart={() => {
                  setDragCat(cat);
                  setDragFromRack(false);
                }}
                onDragEnd={() => {
                  setDragCat(null);
                  setDragFromRack(false);
                }}
                onClick={() => onSelectCategory(cat)}
                title={`${c.name}: ${models[cat].find((m) => m.id === mid)?.name ?? label}`}
              >
                <button
                  className="blk-power"
                  title={isBypassed ? "Enable" : "Bypass"}
                  type="button"
                  onClick={(e) => {
                    e.stopPropagation();
                    onToggleModule(cat);
                  }}
                />
                <button
                  className="blk-remove"
                  type="button"
                  title="Remove from path"
                  onClick={(e) => {
                    e.stopPropagation();
                    removeFromPath(cat);
                  }}
                >
                  ×
                </button>
                <div className="blk-move">
                  <button
                    type="button"
                    title="Move left"
                    onClick={(e) => {
                      e.stopPropagation();
                      moveInPath(cat, -1);
                    }}
                  >
                    ‹
                  </button>
                  <button
                    type="button"
                    title="Move right"
                    onClick={(e) => {
                      e.stopPropagation();
                      moveInPath(cat, 1);
                    }}
                  >
                    ›
                  </button>
                </div>
                <div className="ic">
                  <svg
                    width="22"
                    height="22"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    dangerouslySetInnerHTML={{ __html: icons[c.node] ?? "" }}
                  />
                </div>
                <div className="mtext">
                  <span className="mtitle">{c.short}</span>
                  <span className="mmodel">{label}</span>
                </div>
              </div>
              <div
                className="drop-zone"
                onDragOver={(e) => e.preventDefault()}
                onDrop={() => onDropAt(index + 1)}
              />
            </div>
          );
        })}

        <IoMeter title="Out" left={vu.outL} right={vu.outR} />
      </div>

      {rack.length > 0 && (
        <div className="rack">
          <span className="rack-label">Rack</span>
          {rack.map((cat) => {
            const c = categories[cat];
            return (
              <button
                key={cat}
                type="button"
                className="rack-item"
                style={{ ["--mc" as string]: c.color }}
                draggable
                onDragStart={() => {
                  setDragCat(cat);
                  setDragFromRack(true);
                }}
                onDragEnd={() => {
                  setDragCat(null);
                  setDragFromRack(false);
                }}
                onClick={() => addToPath(cat)}
                title={`Add ${c.name} to path`}
              >
                {c.short}
              </button>
            );
          })}
        </div>
      )}
    </section>
  );
}
