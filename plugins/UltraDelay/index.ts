export {
  ULTRADELAY_DEFAULT_PARAMS,
  clamp,
  divisionToMs,
  normalizeUltraDelayParams,
  serializeUltraDelayParams,
  type DelayTimeDivision,
  type UltraDelayMode,
  type UltraDelayParams,
} from "./Core";

export { UltraDelayEditor } from "./Editor/PluginEditor";

export const UltraDelayPlugin = {
  id: "ultradelay",
  type: "delay",
  name: "UltraDelay",
  shortName: "Delay",
  category: "space",
  color: "#80d4b0",
};
