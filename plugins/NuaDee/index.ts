export {
  NUADEE_DEFAULT_PARAMS,
  clamp,
  normalizeNuaDeeParams,
  satDrive,
  serializeNuaDeeParams,
  type NuaDeeParams,
} from "./Core";

export { NuaDeeEditor } from "./Editor/PluginEditor";

export const NuaDeePlugin = {
  id: "nuadee",
  type: "saturation",
  name: "NuaDee",
  shortName: "SAT",
  category: "distortion",
};
