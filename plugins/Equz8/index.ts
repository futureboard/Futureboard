export {
  EQUZ8_DB_RANGE,
  EQUZ8_DEFAULT_BANDS,
  EQUZ8_DEFAULT_PARAMS,
  EQUZ8_FREQ_MAX,
  EQUZ8_FREQ_MIN,
  bandContributionDb,
  clamp,
  normalizeEquz8Params,
  serializeEquz8Params,
  totalEqGainDb,
  type Equz8Band,
  type Equz8BandType,
  type Equz8Params,
} from "./Core";

export { Equz8Editor } from "./Editor/PluginEditor";

export const Equz8Plugin = {
  id: "equz8",
  type: "eq",
  name: "Equz8",
  shortName: "EQ8",
  category: "eq",
};
