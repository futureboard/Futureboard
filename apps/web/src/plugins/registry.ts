import type { ComponentType } from "react";

import { Equz8Editor, EQUZ8_DEFAULT_PARAMS, serializeEquz8Params } from "../../../../plugins/Equz8";
import { UltraVerbEditor, ULTRAVERB_DEFAULT_PARAMS, serializeUltraVerbParams } from "../../../../plugins/UltraVerb";
import { UltraDelayEditor, ULTRADELAY_DEFAULT_PARAMS, serializeUltraDelayParams } from "../../../../plugins/UltraDelay";
import { FB2ACompEditor, FB2A_DEFAULT_PARAMS, serializeFB2AParams } from "../../../../plugins/FB2AComp";

export type PluginEditorProps = {
  params: Record<string, number | string | boolean>;
  enabled: boolean;
  onParamsChange: (patch: Record<string, number | string | boolean>) => void;
  onToggleEnabled: () => void;
  onReset: () => void;
  getSpectrum?: () => Float32Array | null;
};

export type BuiltInPlugin = {
  id: string;
  name: string;
  shortName: string;
  type: string;
  category: string;
  color: string;
  defaultParams: () => Record<string, number | string | boolean>;
  Editor: ComponentType<PluginEditorProps>;
};

export const BUILT_IN_PLUGINS: BuiltInPlugin[] = [
  {
    id: "equz8",
    name: "Equz8",
    shortName: "EQ8",
    type: "eq",
    category: "eq",
    color: "#22d3ee",
    defaultParams: () => serializeEquz8Params(EQUZ8_DEFAULT_PARAMS),
    Editor: Equz8Editor,
  },
  {
    id: "ultraverb",
    name: "UltraVerb",
    shortName: "Verb",
    type: "reverb",
    category: "space",
    color: "#7cc7ff",
    defaultParams: () => serializeUltraVerbParams(ULTRAVERB_DEFAULT_PARAMS),
    Editor: UltraVerbEditor,
  },
  {
    id: "ultradelay",
    name: "UltraDelay",
    shortName: "Delay",
    type: "delay",
    category: "space",
    color: "#80d4b0",
    defaultParams: () => serializeUltraDelayParams(ULTRADELAY_DEFAULT_PARAMS),
    Editor: UltraDelayEditor,
  },
  {
    id: "fb2acomp",
    name: "FB-2A Comp",
    shortName: "FB2A",
    type: "optical-compressor",
    category: "dynamics",
    color: "#e8a84a",
    defaultParams: () => serializeFB2AParams(FB2A_DEFAULT_PARAMS),
    Editor: FB2ACompEditor,
  },
];

export function findPlugin(nameOrId: string): BuiltInPlugin | undefined {
  const key = nameOrId.toLowerCase().replace(/[-\s]/g, "");
  return BUILT_IN_PLUGINS.find(
    (p) =>
      p.id === nameOrId ||
      p.name === nameOrId ||
      p.id.replace(/-/g, "") === key ||
      p.name.toLowerCase().replace(/[-\s]/g, "") === key
  );
}
