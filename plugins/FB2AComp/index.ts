export {
  FB2A_DEFAULT_PARAMS,
  clamp,
  normalizeFB2AParams,
  peakReductionToThresholdDb,
  serializeFB2AParams,
  type FB2AMode,
  type FB2AMeterMode,
  type FB2AParams,
} from "./Core";

export { FB2ACompEditor } from "./Editor/PluginEditor";

export const FB2ACompPlugin = {
  id: "fb2acomp",
  type: "optical-compressor",
  name: "FB-2A Comp",
  shortName: "FB2A",
  category: "dynamics",
  color: "#e8a84a",
};
