// Centralized binding to "which DSP instance is this shared page showing
// right now" — the piece the multiplexed built-in editor needs that a
// single-instance page never did. See module doc in `../instanceBridge.ts`
// for the wire protocol this drives.
//
// Native remains authoritative (spec: "the native host decides which
// instance is active and validates all mutations"). This provider never
// activates a route by itself — it either reflects a `selectInstance` native
// already approved, or asks native to approve one (`requestSelectInstance`)
// and waits.

import {
  createContext,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { useNavigate, useParams } from "react-router-dom";
import {
  onNativeMessage,
  requestSelectInstance,
  sendBridgeReady,
  sendInstanceReady,
  type InstanceDisplayMetadata,
} from "../instanceBridge";

export type ConnectionStatus =
  | "disconnected"
  | "waiting"
  | "switching"
  | "active"
  | "error";

export type BoundInstanceState = {
  pluginId: string | null;
  instanceId: string | null;
  bindingGeneration: number;
  display: InstanceDisplayMetadata | null;
  connectionStatus: ConnectionStatus;
};

const initialState: BoundInstanceState = {
  pluginId: null,
  instanceId: null,
  bindingGeneration: 0,
  display: null,
  connectionStatus: "waiting",
};

const BoundInstanceContext = createContext<BoundInstanceState>(initialState);

export function useBoundInstance(): BoundInstanceState {
  return useContext(BoundInstanceContext);
}

/** Must match `UI_ORIGIN` / the catalog id native routes this editor under. */
const PLUGIN_ID = "rodharerist";

export function BoundInstanceProvider({ children }: { children: ReactNode }) {
  const navigate = useNavigate();
  const params = useParams<{ instanceId: string }>();
  const [state, setState] = useState<BoundInstanceState>(initialState);

  // The instance id the last *approved* `selectInstance` set. Lets the route
  // effect below tell "native just navigated us here" apart from "the route
  // changed some other way (typed URL, back/forward)" without a render race.
  const approvedInstanceRef = useRef<string | null>(null);

  useEffect(() => {
    sendBridgeReady(PLUGIN_ID);
    const off = onNativeMessage((msg) => {
      if (msg.type === "futureboard.selectInstance") {
        approvedInstanceRef.current = msg.instanceId;
        setState({
          pluginId: msg.pluginId,
          instanceId: msg.instanceId,
          bindingGeneration: msg.bindingGeneration,
          display: msg.display,
          connectionStatus: "active",
        });
        navigate(`/instance/${msg.instanceId}`, { replace: true });
        // Acknowledge only after the state above is committed — React 19
        // batches this synchronously within the handler, so by the time this
        // runs the bound state this instance will render with is already set.
        sendInstanceReady(
          msg.pluginId,
          msg.instanceId,
          msg.bindingGeneration,
          msg.stateRevision,
        );
      } else if (msg.type === "futureboard.instanceRemoved") {
        if (approvedInstanceRef.current === msg.instanceId) {
          approvedInstanceRef.current = null;
          setState((prev) => ({ ...prev, connectionStatus: "waiting" }));
        }
      }
    });
    return off;
    // Runs once: `navigate` is stable per React Router, and re-sending
    // bridgeReady on every render would re-trigger native's snapshot push.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // The route says an instance native hasn't approved — ask, don't assume.
  useEffect(() => {
    const routeInstanceId = params.instanceId;
    if (!routeInstanceId) return;
    if (routeInstanceId === approvedInstanceRef.current) return;
    requestSelectInstance(routeInstanceId);
  }, [params.instanceId]);

  return (
    <BoundInstanceContext.Provider value={state}>
      {children}
    </BoundInstanceContext.Provider>
  );
}
