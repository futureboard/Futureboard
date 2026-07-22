// Native<->React instance-binding bridge protocol.
//
// Separate from `bridge.ts` (which forwards parameter/model/meter calls
// through an injected `window.rodhareist` object — a different, still-inert
// contract; see its own file header). This one binds *which* DSP insert the
// shared CEF page is currently showing, and travels over a different
// transport:
//
//   native -> React : `window.postMessage(msg, "*")`, run via
//                      `Frame::execute_java_script` (see
//                      `builtin_plugin_editor_window.rs::push_selected_instance`)
//   React -> native : `fetch("__bridge", { method: "POST", body: JSON })`,
//                      intercepted by the `mikoplugin://` scheme handler
//                      before it reaches any plugin's asset resolver (see
//                      `SphereWebView/src/scheme.rs`)
//
// Message shapes here must match the Rust structs in
// `builtin_plugin_editor_window.rs` (`SelectInstanceMsg`, `InstanceRemovedMsg`,
// `InboundMsg`) field-for-field — both sides serialize with camelCase.

/** Bump alongside any breaking change to a message shape below. */
export const BRIDGE_PROTOCOL_VERSION = 1;

/** Relative to the page's own origin (`mikoplugin://<plugin>/`). */
const BRIDGE_ENDPOINT = "__bridge";

export type InstanceDisplayMetadata = {
  trackId: string;
  trackName: string;
  insertId: string;
  insertName: string;
};

/** Native -> React: rebind the shared page to a different DSP instance. */
export type SelectInstanceMessage = {
  type: "futureboard.selectInstance";
  protocolVersion: number;
  pluginId: string;
  instanceId: string;
  bindingGeneration: number;
  display: InstanceDisplayMetadata;
  stateRevision: number;
  /**
   * TODO(phase5): rodharerist has no serialized per-insert DSP state yet
   * (native's `SelectInstanceMsg.state` is always `{}` today — see that
   * struct's doc comment). Treat this as opaque until the native side has
   * something real to put in it.
   */
  state: unknown;
};

/** Native -> React: the instance that used to be selected no longer exists. */
export type InstanceRemovedMessage = {
  type: "futureboard.instanceRemoved";
  protocolVersion: number;
  instanceId: string;
};

export type NativeMessage = SelectInstanceMessage | InstanceRemovedMessage;

function isNativeMessage(data: unknown): data is NativeMessage {
  if (!data || typeof data !== "object") return false;
  const type = (data as { type?: unknown }).type;
  return type === "futureboard.selectInstance" || type === "futureboard.instanceRemoved";
}

/**
 * Subscribe to native->React bridge messages. Returns an unsubscribe
 * function. Safe to call with no native host present (e.g. `bun run dev`) —
 * the listener just never fires.
 */
export function onNativeMessage(handler: (msg: NativeMessage) => void): () => void {
  const listener = (event: MessageEvent) => {
    if (isNativeMessage(event.data)) handler(event.data);
  };
  window.addEventListener("message", listener);
  return () => window.removeEventListener("message", listener);
}

/** Fire-and-forget POST to the native bridge endpoint. Never throws. */
function post(body: unknown): void {
  try {
    void fetch(BRIDGE_ENDPOINT, {
      method: "POST",
      body: JSON.stringify(body),
    }).catch(() => {
      // No native host (standalone browser preview) — expected, not an error.
    });
  } catch {
    /* no-op */
  }
}

/** Sent once on mount. Native replies with the currently-selected instance
 * (or nothing, if none is selected yet) once this arrives. */
export function sendBridgeReady(pluginId: string): void {
  post({
    type: "futureboard.bridgeReady",
    protocolVersion: BRIDGE_PROTOCOL_VERSION,
    pluginId,
    bridgeVersion: BRIDGE_PROTOCOL_VERSION,
  });
}

/** Acknowledges a `selectInstance` once its state has been atomically applied. */
export function sendInstanceReady(
  pluginId: string,
  instanceId: string,
  bindingGeneration: number,
  stateRevision: number,
): void {
  post({
    type: "futureboard.instanceReady",
    protocolVersion: BRIDGE_PROTOCOL_VERSION,
    pluginId,
    instanceId,
    bindingGeneration,
    stateRevision,
  });
}

/**
 * Ask native to validate and (if approved) select `instanceId`. Used when the
 * route changed without an approved `selectInstance` behind it (manual URL
 * edit, browser back/forward) — the route is never trusted on its own.
 */
export function requestSelectInstance(instanceId: string): void {
  post({
    type: "futureboard.requestSelectInstance",
    protocolVersion: BRIDGE_PROTOCOL_VERSION,
    instanceId,
  });
}
