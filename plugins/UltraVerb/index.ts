export {
  ULTRAVERB_DEFAULT_PARAMS,
  clamp,
  normalizeUltraVerbParams,
  serializeUltraVerbParams,
  type UltraVerbMode,
  type UltraVerbParams,
} from "./Core";

export { UltraVerbEditor } from "./Editor/PluginEditor";

export const UltraVerbPlugin = {
  id: "ultraverb",
  type: "reverb",
  name: "UltraVerb",
  shortName: "Verb",
  category: "space",
  color: "#7cc7ff",
};
