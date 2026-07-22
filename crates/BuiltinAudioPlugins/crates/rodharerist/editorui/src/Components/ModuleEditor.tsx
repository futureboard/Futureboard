import { useState } from "react";
import {
  categories,
  defaultValueFor,
  models,
  type CategoryId,
  type Param,
} from "../data";
import type { NamCaptureLoadOptions } from "../bridge";
import { distanceCm, positionLabel } from "../globals";
import { Knob } from "./Knob";

type ModuleEditorProps = {
  activeCat: CategoryId;
  activeModelId: string;
  bypassed: boolean;
  params: Param[];
  onSelectModel: (id: string) => void;
  onToggleBypass: () => void;
  onParamChange: (id: string, value: number) => void;
  onLoadNamCapture: (json: string, opts: NamCaptureLoadOptions) => void;
  onBypassCab: () => void;
};

export function ModuleEditor({
  activeCat,
  activeModelId,
  bypassed,
  params,
  onSelectModel,
  onToggleBypass,
  onParamChange,
  onLoadNamCapture,
  onBypassCab,
}: ModuleEditorProps) {
  const list = models[activeCat] ?? [];
  const model = list.find((m) => m.id === activeModelId) ?? list[0];
  const cat = categories[activeCat];
  const isNamCapture = activeCat === "amp" && activeModelId === "nam_capture";
  const isCabinet = activeCat === "cab";
  const [namStereo, setNamStereo] = useState(true);
  const [namFullRig, setNamFullRig] = useState(false);

  const paramValue = (id: string, fallback: number) =>
    params.find((p) => p.id === id)?.val ?? fallback;

  const handleNamFile = (file: File | undefined) => {
    if (!file) return;
    const reader = new FileReader();
    reader.onload = () => {
      const json = reader.result;
      if (typeof json === "string") {
        onLoadNamCapture(json, {
          name: file.name.replace(/\.nam$/i, ""),
          stereo: namStereo,
          fullRig: namFullRig,
        });
      }
    };
    reader.readAsText(file);
  };

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

        {isNamCapture && (
          <div className="nam-capture-controls">
            <label className="nam-file-btn">
              Load .nam Capture…
              <input
                type="file"
                accept=".nam"
                onChange={(e) => handleNamFile(e.target.files?.[0])}
              />
            </label>
            <label className="nam-check">
              <input
                type="checkbox"
                checked={namStereo}
                onChange={(e) => setNamStereo(e.target.checked)}
              />
              Stereo (two independent models)
            </label>
            <label className="nam-check">
              <input
                type="checkbox"
                checked={namFullRig}
                onChange={(e) => setNamFullRig(e.target.checked)}
              />
              Full Rig capture (amp + cab + mic)
            </label>
            {namFullRig && (
              <button type="button" className="nam-bypass-cab" onClick={onBypassCab}>
                Bypass Cab
              </button>
            )}
          </div>
        )}

        {isCabinet ? (
          // Mic placement is edited with the same knobs as every other module;
          // the readout translates the two parameters into the terms an engineer
          // thinks in (axis position, centimetres).
          <div className="cab-inspector">
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
            <div className="cab-readout">
              <span>
                <b>{positionLabel(paramValue("cab_mic", 20))}</b>{" "}
                {paramValue("cab_mic", 20).toFixed(0)}%
              </span>
              <span>{distanceCm(paramValue("cab_dist", 40)).toFixed(1)} cm</span>
            </div>
            <p className="inspector-note">
              Position is measured from the speaker centre; distance is shown on a
              0–30 cm scale. The cabinet is modelled as a whole, so there is no
              per-speaker or second-mic processing yet.
            </p>
          </div>
        ) : (
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
        )}
      </div>
    </section>
  );
}
