import {
  categories,
  chainOrder,
  models,
  type CategoryId,
  type Param,
} from "../data";
import { Knob } from "./Knob";
import { ScopeBackdrop } from "./ScopeBackdrop";

type ModuleEditorProps = {
  activeCat: CategoryId;
  activeModelId: string;
  bypassed: boolean;
  params: Param[];
  onSelectCategory: (cat: CategoryId) => void;
  onSelectModel: (id: string) => void;
  onToggleBypass: () => void;
  onParamChange: (id: string, value: number) => void;
};

export function ModuleEditor({
  activeCat,
  activeModelId,
  bypassed,
  params,
  onSelectCategory,
  onSelectModel,
  onToggleBypass,
  onParamChange,
}: ModuleEditorProps) {
  const list = models[activeCat] ?? [];
  const model = list.find((m) => m.id === activeModelId) ?? list[0];

  return (
    <section className="editor">
      <div className="cats">
        <div className="cats-head">Modules</div>
        {chainOrder.map((cat) => {
          const c = categories[cat];
          return (
            <div
              key={cat}
              className={`cat${cat === activeCat ? " active" : ""}`}
              style={{ ["--cc" as string]: c.color }}
              onClick={() => onSelectCategory(cat)}
            >
              <span className="cdot" />
              {c.name}
            </div>
          );
        })}
      </div>

      <div className="models">
        <div className="models-head">Model</div>
        {list.map((m) => (
          <div
            key={m.id}
            className={`model${m.id === activeModelId ? " active" : ""}`}
            onClick={() => onSelectModel(m.id)}
          >
            <div className="mt">{m.name}</div>
            <div className="ms">{m.sub}</div>
          </div>
        ))}
      </div>

      <div className="faceplate">
        <ScopeBackdrop />
        <div className="fp-head">
          <div>
            <div className="fp-name">{model?.name ?? "—"}</div>
            <div className="fp-sub">{model?.sub ?? ""}</div>
          </div>
          <button
            className={`bypass${bypassed ? " off" : ""}`}
            onClick={onToggleBypass}
            type="button"
          >
            <span className="led" />
            <span>{bypassed ? "Bypassed" : "Active"}</span>
          </button>
        </div>
        <div className="knob-bank">
          {params.map((p) => (
            <Knob
              key={p.id}
              id={p.id}
              name={p.name}
              min={p.min}
              max={p.max}
              value={p.val}
              unit={p.unit}
              onChange={onParamChange}
            />
          ))}
        </div>
      </div>
    </section>
  );
}
