import {
  categories,
  defaultValueFor,
  models,
  type CategoryId,
  type Param,
} from "../data";
import { Knob } from "./Knob";

type ModuleEditorProps = {
  activeCat: CategoryId;
  activeModelId: string;
  bypassed: boolean;
  params: Param[];
  onSelectModel: (id: string) => void;
  onToggleBypass: () => void;
  onParamChange: (id: string, value: number) => void;
};

export function ModuleEditor({
  activeCat,
  activeModelId,
  bypassed,
  params,
  onSelectModel,
  onToggleBypass,
  onParamChange,
}: ModuleEditorProps) {
  const list = models[activeCat] ?? [];
  const model = list.find((m) => m.id === activeModelId) ?? list[0];
  const cat = categories[activeCat];

  return (
    <section className="editor">
      <div
        className="faceplate"
        style={{ ["--cat-color" as string]: cat.color }}
      >
        <div className="fp-head">
          <div className="fp-identity">
            <span className="fp-stage" style={{ color: cat.color }}>
              {cat.name}
            </span>
            <div className="fp-name">{model?.name ?? "—"}</div>
            <div className="fp-sub">{model?.sub ?? ""}</div>
          </div>
          <button
            className={`bypass${bypassed ? " off" : ""}`}
            onClick={onToggleBypass}
            type="button"
            aria-pressed={!bypassed}
          >
            <span className="led" />
            <span>{bypassed ? "Bypassed" : "Active"}</span>
          </button>
        </div>

        <div className="model-strip" role="listbox" aria-label="Model">
          {list.map((m) => (
            <button
              key={m.id}
              type="button"
              role="option"
              aria-selected={m.id === activeModelId}
              className={`model-chip${m.id === activeModelId ? " active" : ""}`}
              onClick={() => onSelectModel(m.id)}
            >
              <span className="mt">{m.name}</span>
              <span className="ms">{m.sub}</span>
            </button>
          ))}
        </div>

        <div className="param-bank">
          {params.map((p) => (
            <Knob
              key={p.id}
              id={p.id}
              name={p.name}
              min={p.min}
              max={p.max}
              value={p.val}
              unit={p.unit}
              defaultValue={defaultValueFor(activeModelId, p.id)}
              onChange={onParamChange}
            />
          ))}
        </div>
      </div>
    </section>
  );
}
