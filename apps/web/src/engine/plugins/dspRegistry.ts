import type { InsertDevice } from "../../types/daw";
import type { InsertNodeFactory } from "./types";
import { createEquz8Node } from "./factories/equz8Node";
import { createUltraDelayNode } from "./factories/ultraDelayNode";
import { createUltraVerbNode } from "./factories/ultraVerbNode";
import { createFB2ACompNode } from "./factories/fb2aCompNode";

// Map by plugin type string (from InsertDevice.type or InsertDevice.name)
const BY_TYPE: Record<string, InsertNodeFactory> = {
  eq:                   createEquz8Node,
  reverb:               createUltraVerbNode,
  delay:                createUltraDelayNode,
  "optical-compressor": createFB2ACompNode,
};

const BY_NAME: Record<string, InsertNodeFactory> = {
  equz8:      createEquz8Node,
  ultraverb:  createUltraVerbNode,
  ultradelay: createUltraDelayNode,
  "fb-2a comp": createFB2ACompNode,
  fb2acomp:   createFB2ACompNode,
};

export function getDspFactory(device: InsertDevice): InsertNodeFactory | undefined {
  return (
    BY_TYPE[device.type] ??
    BY_NAME[device.name.toLowerCase()] ??
    undefined
  );
}
