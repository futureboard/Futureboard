// Pins the TS half of the cross-language param contract. The Rust half is
// `wire::tests::model_select_wire_values_match_editor_ids` in
// `rodharerist/src/wire.rs` — both must agree on the numeric model-select
// values and on the coalescing semantics native relies on.

import { beforeEach, describe, expect, test } from "bun:test";
import {
  AMP_MODEL_INDEX,
  CAB_MODEL_INDEX,
  DRIVE_MODEL_INDEX,
  REVERB_MODEL_INDEX,
  TONE_ENGINE_INDEX,
  __flushParamEditsForTest,
  postClearClip,
  postEnabled,
  postLoadNamCapture,
  postModel,
  postParam,
  postPathOrder,
} from "./bridge";
import {
  clearActiveParamBinding,
  setActiveParamBinding,
} from "./instanceBridge";

/** Capture `futureboard.setParams` POST bodies fired by a flush. */
function captureBatches(): { id: string; value: number }[][] {
  const batches: { id: string; value: number }[][] = [];
  globalThis.fetch = ((_url: unknown, init?: { body?: unknown }) => {
    const body = JSON.parse(String(init?.body ?? "{}"));
    if (body.type === "futureboard.setParams") batches.push(body.params);
    return Promise.resolve(new Response("{}"));
  }) as typeof fetch;
  return batches;
}

beforeEach(() => {
  // Drain anything a previous test queued, then bind a fresh instance.
  clearActiveParamBinding();
  __flushParamEditsForTest();
  setActiveParamBinding({
    pluginId: "rodharerist",
    instanceId: "track-1::insert-1",
    bindingGeneration: 1,
  });
});

describe("model-select wire values", () => {
  test("amp map mirrors AmpModel::ALL order", () => {
    expect(AMP_MODEL_INDEX).toEqual({
      mandarin: 0,
      plexi: 1,
      twin: 2,
      topboost: 3,
      recto: 4,
      jcm: 5,
      slate: 6,
      bassman: 7,
    });
  });

  test("drive map mirrors DriveModel::ALL order", () => {
    expect(DRIVE_MODEL_INDEX).toEqual({
      screamer: 0,
      minotaur: 1,
      rat: 2,
      breaker: 3,
      fuzz: 4,
      centurion: 5,
      ds_one: 6,
      super_drive: 7,
      metal_core: 8,
      tight_rift: 9,
    });
  });

  test("cab map mirrors CabModel::ALL order", () => {
    expect(CAB_MODEL_INDEX).toEqual({
      vintage_cab: 0,
      american_2x12: 1,
      tweed_1x12: 2,
      modern_412: 3,
      open_back: 4,
      vintage_212: 5,
      oversized_412: 6,
      bass_cabinet: 7,
      brit_412: 8,
      uber_412: 9,
      slo_412: 10,
    });
  });

  test("reverb map mirrors ReverbModel::ALL order", () => {
    expect(REVERB_MODEL_INDEX).toEqual({
      plate: 0,
      room: 1,
      hall: 2,
      shimmer: 3,
    });
  });

  test("tone engine indices match ToneEngineKind", () => {
    expect(TONE_ENGINE_INDEX).toEqual({ classic: 0, nam_capture: 1, bypass: 2 });
  });
});

describe("param edit coalescing", () => {
  test("repeated edits to one id flush as a single last-value entry", () => {
    const batches = captureBatches();
    postParam("drive_gain", 1.0);
    postParam("drive_gain", 4.2);
    postParam("drive_gain", 9.9);
    __flushParamEditsForTest();
    expect(batches).toEqual([[{ id: "drive_gain", value: 9.9 }]]);
  });

  test("distinct ids keep insertion order in one batch", () => {
    const batches = captureBatches();
    postEnabled("amp", false);
    postModel("drive", "rat");
    postParam("delay_time", 500);
    __flushParamEditsForTest();
    expect(batches).toEqual([
      [
        { id: "amp_on", value: 0 },
        { id: "drive_model", value: 2 },
        { id: "delay_time", value: 500 },
      ],
    ]);
  });

  test("amp special engines ride tone_engine", () => {
    const batches = captureBatches();
    postModel("amp", "bypass");
    __flushParamEditsForTest();
    postModel("amp", "nam_capture");
    __flushParamEditsForTest();
    postModel("amp", "plexi");
    __flushParamEditsForTest();
    expect(batches).toEqual([
      [{ id: "tone_engine", value: 2 }],
      [{ id: "tone_engine", value: 1 }],
      [{ id: "amp_model", value: 1 }],
    ]);
  });

  test("path order publishes all ten slots with -1 for empty", () => {
    const batches = captureBatches();
    postPathOrder(["amp", "comp", "eq", "wah"]);
    __flushParamEditsForTest();
    expect(batches).toEqual([
      [
        { id: "path_slot_0", value: 2 },
        { id: "path_slot_1", value: 7 },
        { id: "path_slot_2", value: 8 },
        { id: "path_slot_3", value: 9 },
        { id: "path_slot_4", value: -1 },
        { id: "path_slot_5", value: -1 },
        { id: "path_slot_6", value: -1 },
        { id: "path_slot_7", value: -1 },
        { id: "path_slot_8", value: -1 },
        { id: "path_slot_9", value: -1 },
      ],
    ]);
  });

  test("mod and wah model selects ride their model params", () => {
    const batches = captureBatches();
    postModel("mod", "phaser");
    __flushParamEditsForTest();
    postModel("mod", "tremolo");
    __flushParamEditsForTest();
    postModel("wah", "touch_wah");
    __flushParamEditsForTest();
    expect(batches).toEqual([
      [{ id: "mod_model", value: 1 }],
      [{ id: "mod_model", value: 3 }],
      [{ id: "wah_model", value: 1 }],
    ]);
  });

  test("comp/eq enables and clear_clip ride the param wire", () => {
    const batches = captureBatches();
    postEnabled("comp", false);
    postEnabled("eq", true);
    postClearClip();
    __flushParamEditsForTest();
    expect(batches).toEqual([
      [
        { id: "comp_on", value: 0 },
        { id: "eq_on", value: 1 },
        { id: "clear_clip", value: 1 },
      ],
    ]);
  });

  test("loadNamCapture posts the bound instance and full file text", () => {
    const posts: Record<string, unknown>[] = [];
    globalThis.fetch = ((_url: unknown, init?: { body?: unknown }) => {
      posts.push(JSON.parse(String(init?.body ?? "{}")));
      return Promise.resolve(new Response("{}"));
    }) as typeof fetch;
    postLoadNamCapture('{"weights":[1]}', {
      name: "MyCapture",
      stereo: true,
      fullRig: false,
    });
    expect(posts).toEqual([
      {
        type: "futureboard.loadNamCapture",
        protocolVersion: 1,
        pluginId: "rodharerist",
        instanceId: "track-1::insert-1",
        bindingGeneration: 1,
        name: "MyCapture",
        json: '{"weights":[1]}',
        stereo: true,
        fullRig: false,
      },
    ]);
  });

  test("rebinding drops edits queued under the old instance", () => {
    const batches = captureBatches();
    postParam("drive_gain", 3.3);
    setActiveParamBinding({
      pluginId: "rodharerist",
      instanceId: "track-2::insert-7",
      bindingGeneration: 2,
    });
    __flushParamEditsForTest();
    expect(batches).toEqual([]);
  });

  test("no binding means no POST at all", () => {
    const batches = captureBatches();
    clearActiveParamBinding();
    postParam("drive_gain", 3.3);
    __flushParamEditsForTest();
    expect(batches).toEqual([]);
  });
});
