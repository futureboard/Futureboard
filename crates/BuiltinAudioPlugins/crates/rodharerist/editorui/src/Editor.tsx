import {
  StrictMode,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { createRoot } from "react-dom/client";
import {
  categories,
  chainOrder,
  defaultPath,
  models,
  parametersForPreset,
  presetsData,
  rackFromPath,
  type CategoryId,
  type Param,
} from "./data";
import { Layout } from "./Layout";
import {
  hasNativeBridge,
  postEnabled,
  postModel,
  postParam,
  postPathOrder,
} from "./bridge";
import "./Styles/Editor.css";

type VuState = {
  inL: number;
  inR: number;
  outL: number;
  outR: number;
};

type RigSnapshot = {
  activeCat: CategoryId;
  activeModelId: string;
  stageModels: Record<CategoryId, string>;
  pathOrder: CategoryId[];
  bypassed: Partial<Record<CategoryId, boolean>>;
  parameters: Record<string, Param[]>;
};

function applyCategoryTheme(cat: CategoryId) {
  const c = categories[cat];
  document.documentElement.style.setProperty("--cat-color", c.color);
  document.documentElement.style.setProperty("--cat-rgb", c.rgb);
}

function cloneParameters(
  src: Record<string, Param[]>,
): Record<string, Param[]> {
  const out: Record<string, Param[]> = {};
  for (const [id, params] of Object.entries(src)) {
    out[id] = params.map((p) => ({ ...p }));
  }
  return out;
}

function defaultStageModels(focus: CategoryId, modelId: string) {
  const stageModels = {} as Record<CategoryId, string>;
  for (const cat of chainOrder) {
    stageModels[cat] = models[cat][0]?.id ?? "";
  }
  stageModels[focus] = modelId;
  return stageModels;
}

function makeSnapshot(
  activeCat: CategoryId,
  activeModelId: string,
  stageModels: Record<CategoryId, string>,
  pathOrder: CategoryId[],
  bypassed: Partial<Record<CategoryId, boolean>>,
  parameters: Record<string, Param[]>,
): RigSnapshot {
  return {
    activeCat,
    activeModelId,
    stageModels: { ...stageModels },
    pathOrder: [...pathOrder],
    bypassed: { ...bypassed },
    parameters: cloneParameters(parameters),
  };
}

function applySnapshotToDsp(snap: RigSnapshot) {
  postPathOrder(snap.pathOrder);
  for (const cat of chainOrder) {
    const modelId = snap.stageModels[cat];
    if (modelId) postModel(categories[cat].node, modelId);
    postEnabled(categories[cat].node, !snap.bypassed[cat]);
  }
  for (const cat of chainOrder) {
    const modelId = snap.stageModels[cat];
    for (const param of snap.parameters[modelId] ?? []) {
      postParam(param.id, param.val);
    }
  }
}

function factorySnapshot(id: string): RigSnapshot | null {
  const p = presetsData.find((x) => x.id === id);
  if (!p) return null;
  const bypassed: Partial<Record<CategoryId, boolean>> = {};
  for (const cat of p.bypassed ?? []) bypassed[cat] = true;
  return {
    activeCat: p.category,
    activeModelId: p.model,
    stageModels: defaultStageModels(p.category, p.model),
    pathOrder: p.path ? [...p.path] : defaultPath(),
    bypassed,
    parameters: parametersForPreset(p),
  };
}

function RodhareistEditor() {
  const initial = presetsData[4]!;
  const [currentPresetId, setCurrentPresetId] = useState(initial.id);
  const [activeCat, setActiveCat] = useState<CategoryId>(initial.category);
  const [activeModelId, setActiveModelId] = useState(initial.model);
  const [stageModels, setStageModels] = useState<Record<CategoryId, string>>(
    () => defaultStageModels(initial.category, initial.model),
  );
  const [pathOrder, setPathOrder] = useState<CategoryId[]>(() => defaultPath());
  const [bypassed, setBypassed] = useState<Partial<Record<CategoryId, boolean>>>(
    {},
  );
  const [parameters, setParameters] = useState<Record<string, Param[]>>(
    () => parametersForPreset(initial),
  );
  const [modified, setModified] = useState(false);
  const [savedRigs, setSavedRigs] = useState<Record<string, RigSnapshot>>({});
  const [drafts, setDrafts] = useState<Record<string, RigSnapshot>>({});
  const [dirtyPresetIds, setDirtyPresetIds] = useState<Set<string>>(
    () => new Set(),
  );
  const [testing, setTesting] = useState(false);
  const [vu, setVu] = useState<VuState>({
    inL: 0,
    inR: 0,
    outL: 0,
    outR: 0,
  });
  const [showTestDi] = useState(() => !hasNativeBridge());

  const [pendingSwitchId, setPendingSwitchId] = useState<string | null>(null);

  const audioRef = useRef<{
    ctx: AudioContext;
    gain: GainNode;
    timer: number | null;
  } | null>(null);
  const levelsRef = useRef({ inLvl: 0, outLvl: 0 });

  // Keep a ref mirror so loadPreset can stash without stale closures.
  const liveRef = useRef({
    currentPresetId,
    activeCat,
    activeModelId,
    stageModels,
    pathOrder,
    bypassed,
    parameters,
    modified,
    drafts,
    savedRigs,
    pendingSwitchId,
  });
  liveRef.current = {
    currentPresetId,
    activeCat,
    activeModelId,
    stageModels,
    pathOrder,
    bypassed,
    parameters,
    modified,
    drafts,
    savedRigs,
    pendingSwitchId,
  };

  const currentPreset = useMemo(
    () =>
      presetsData.find((p) => p.id === currentPresetId) ?? presetsData[0]!,
    [currentPresetId],
  );

  const params = parameters[activeModelId] ?? [];

  useEffect(() => {
    applyCategoryTheme(activeCat);
  }, [activeCat]);

  useEffect(() => {
    const stages = defaultStageModels(initial.category, initial.model);
    postPathOrder(defaultPath());
    for (const cat of chainOrder) {
      postModel(categories[cat].node, stages[cat]!);
      postEnabled(categories[cat].node, true);
      for (const param of parameters[stages[cat]!] ?? []) {
        postParam(param.id, param.val);
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    let raf = 0;
    let alive = true;
    const tick = () => {
      if (!alive) return;
      raf = window.setTimeout(tick, 30);
      const lv = levelsRef.current;
      setVu((prev) => {
        const inL =
          lv.inLvl > prev.inL ? lv.inLvl : Math.max(0, prev.inL - 0.015);
        const inR =
          lv.inLvl * 0.95 > prev.inR
            ? lv.inLvl * 0.95
            : Math.max(0, prev.inR - 0.015);
        const outL =
          lv.outLvl > prev.outL ? lv.outLvl : Math.max(0, prev.outL - 0.015);
        const outR =
          lv.outLvl * 0.9 > prev.outR
            ? lv.outLvl * 0.9
            : Math.max(0, prev.outR - 0.015);
        lv.inLvl = Math.max(0, lv.inLvl - 0.05);
        lv.outLvl = Math.max(0, lv.outLvl - 0.055);
        return { inL, inR, outL, outR };
      });
    };
    tick();
    return () => {
      alive = false;
      window.clearTimeout(raf);
    };
  }, []);

  const markDirty = useCallback(() => {
    setModified(true);
    setDirtyPresetIds((prev) => {
      const id = liveRef.current.currentPresetId;
      if (prev.has(id)) return prev;
      const next = new Set(prev);
      next.add(id);
      return next;
    });
  }, []);

  const applyLocalSnapshot = useCallback(
    (snap: RigSnapshot, presetId: string, isDirty: boolean) => {
      setCurrentPresetId(presetId);
      setActiveCat(snap.activeCat);
      setActiveModelId(snap.activeModelId);
      setStageModels(snap.stageModels);
      setPathOrder(snap.pathOrder);
      setBypassed(snap.bypassed);
      setParameters(cloneParameters(snap.parameters));
      setModified(isDirty);
      applySnapshotToDsp(snap);
    },
    [],
  );

  const commitLoadPreset = useCallback(
    (id: string, opts?: { discardCurrent?: boolean }) => {
      const live = liveRef.current;
      let nextDrafts = live.drafts;

      if (opts?.discardCurrent) {
        nextDrafts = { ...live.drafts };
        delete nextDrafts[live.currentPresetId];
        setDrafts(nextDrafts);
        setDirtyPresetIds((prev) => {
          if (!prev.has(live.currentPresetId)) return prev;
          const next = new Set(prev);
          next.delete(live.currentPresetId);
          return next;
        });
      }

      const snap =
        nextDrafts[id] ?? live.savedRigs[id] ?? factorySnapshot(id);
      if (!snap) return;

      applyLocalSnapshot(snap, id, !!nextDrafts[id]);
      setPendingSwitchId(null);
    },
    [applyLocalSnapshot],
  );

  const loadPreset = useCallback(
    (id: string) => {
      const live = liveRef.current;
      if (id === live.currentPresetId) return;

      // Leaving a dirty unsaved rig → ask before switching.
      if (live.modified) {
        setPendingSwitchId(id);
        return;
      }

      commitLoadPreset(id);
    },
    [commitLoadPreset],
  );

  const stepPreset = useCallback(
    (dir: number) => {
      const idx = presetsData.findIndex(
        (p) => p.id === liveRef.current.currentPresetId,
      );
      const next = (idx + dir + presetsData.length) % presetsData.length;
      loadPreset(presetsData[next]!.id);
    },
    [loadPreset],
  );

  const selectCategory = useCallback(
    (cat: CategoryId) => {
      setActiveCat(cat);
      const modelId =
        liveRef.current.stageModels[cat] || models[cat]?.[0]?.id;
      if (!modelId) return;
      setActiveModelId(modelId);
      postModel(categories[cat].node, modelId);
    },
    [],
  );

  const selectModel = useCallback(
    (id: string) => {
      const cat = liveRef.current.activeCat;
      setActiveModelId(id);
      setStageModels((prev) => ({ ...prev, [cat]: id }));
      postModel(categories[cat].node, id);
      for (const param of liveRef.current.parameters[id] ?? []) {
        postParam(param.id, param.val);
      }
      markDirty();
    },
    [markDirty],
  );

  const toggleBypassFor = useCallback(
    (cat: CategoryId) => {
      setBypassed((prev) => {
        const nextBypass = !prev[cat];
        postEnabled(categories[cat].node, !nextBypass);
        return { ...prev, [cat]: nextBypass };
      });
      markDirty();
    },
    [markDirty],
  );

  const toggleBypass = useCallback(
    () => toggleBypassFor(liveRef.current.activeCat),
    [toggleBypassFor],
  );

  const reorderPath = useCallback(
    (next: CategoryId[]) => {
      setPathOrder(next);
      postPathOrder(next);
      markDirty();
      const live = liveRef.current;
      if (!next.includes(live.activeCat)) {
        const fallback = next[0] ?? rackFromPath(next)[0];
        if (fallback) {
          setActiveCat(fallback);
          const modelId =
            live.stageModels[fallback] || models[fallback]?.[0]?.id;
          if (modelId) {
            setActiveModelId(modelId);
            postModel(categories[fallback].node, modelId);
          }
        }
      }
    },
    [markDirty],
  );

  const onParamChange = useCallback(
    (id: string, value: number) => {
      postParam(id, value);
      markDirty();
      const modelId = liveRef.current.activeModelId;
      setParameters((prev) => {
        const modelParams = prev[modelId];
        if (!modelParams) return prev;
        return {
          ...prev,
          [modelId]: modelParams.map((p) =>
            p.id === id ? { ...p, val: value } : p,
          ),
        };
      });
    },
    [markDirty],
  );

  const saveRig = useCallback(() => {
    const live = liveRef.current;
    const snap = makeSnapshot(
      live.activeCat,
      live.activeModelId,
      live.stageModels,
      live.pathOrder,
      live.bypassed,
      live.parameters,
    );
    setSavedRigs((prev) => ({ ...prev, [live.currentPresetId]: snap }));
    setDrafts((prev) => {
      if (!(live.currentPresetId in prev)) return prev;
      const next = { ...prev };
      delete next[live.currentPresetId];
      return next;
    });
    setModified(false);
    setDirtyPresetIds((prev) => {
      if (!prev.has(live.currentPresetId)) return prev;
      const next = new Set(prev);
      next.delete(live.currentPresetId);
      return next;
    });
  }, []);

  const confirmSaveAndSwitch = useCallback(() => {
    const target = liveRef.current.pendingSwitchId;
    saveRig();
    if (target) commitLoadPreset(target);
  }, [commitLoadPreset, saveRig]);

  const confirmDiscardAndSwitch = useCallback(() => {
    const target = liveRef.current.pendingSwitchId;
    if (!target) {
      setPendingSwitchId(null);
      return;
    }
    commitLoadPreset(target, { discardCurrent: true });
  }, [commitLoadPreset]);

  const cancelSwitch = useCallback(() => setPendingSwitchId(null), []);

  const revertRig = useCallback(() => {
    const live = liveRef.current;
    const snap =
      live.savedRigs[live.currentPresetId] ??
      factorySnapshot(live.currentPresetId);
    if (!snap) return;
    setDrafts((prev) => {
      if (!(live.currentPresetId in prev)) return prev;
      const next = { ...prev };
      delete next[live.currentPresetId];
      return next;
    });
    setDirtyPresetIds((prev) => {
      if (!prev.has(live.currentPresetId)) return prev;
      const next = new Set(prev);
      next.delete(live.currentPresetId);
      return next;
    });
    applyLocalSnapshot(snap, live.currentPresetId, false);
  }, [applyLocalSnapshot]);

  const toggleTest = useCallback(async () => {
    if (!audioRef.current) {
      const Ctx =
        window.AudioContext ||
        (window as unknown as { webkitAudioContext: typeof AudioContext })
          .webkitAudioContext;
      const ctx = new Ctx();
      const gain = ctx.createGain();
      gain.connect(ctx.destination);
      audioRef.current = { ctx, gain, timer: null };
    }

    const audio = audioRef.current;
    const next = !testing;
    setTesting(next);

    if (audio.timer !== null) {
      window.clearInterval(audio.timer);
      audio.timer = null;
    }

    if (!next) return;

    audio.timer = window.setInterval(() => {
      if (audio.ctx.state === "suspended") void audio.ctx.resume();
      [82.41, 110.0, 146.83, 196.0].forEach((f, i) => {
        const osc = audio.ctx.createOscillator();
        const g = audio.ctx.createGain();
        osc.type = "triangle";
        osc.frequency.setValueAtTime(f, audio.ctx.currentTime + i * 0.04);
        g.gain.setValueAtTime(0, audio.ctx.currentTime);
        g.gain.linearRampToValueAtTime(
          0.16,
          audio.ctx.currentTime + 0.01 + i * 0.04,
        );
        g.gain.exponentialRampToValueAtTime(
          0.001,
          audio.ctx.currentTime + 0.75 + i * 0.04,
        );
        osc.connect(g);
        g.connect(audio.gain);
        osc.start();
        osc.stop(audio.ctx.currentTime + 0.85);
      });
      levelsRef.current.inLvl = 0.65 + Math.random() * 0.22;
      levelsRef.current.outLvl = 0.58 + Math.random() * 0.26;
    }, 850);
  }, [testing]);

  useEffect(() => {
    return () => {
      const audio = audioRef.current;
      if (!audio) return;
      if (audio.timer !== null) window.clearInterval(audio.timer);
      void audio.ctx.close();
    };
  }, []);

  useEffect(() => {
    const isTypingTarget = (el: EventTarget | null) => {
      if (!(el instanceof HTMLElement)) return false;
      const tag = el.tagName;
      if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return true;
      return el.isContentEditable;
    };

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.altKey) return;
      if (isTypingTarget(e.target)) return;

      if (liveRef.current.pendingSwitchId) {
        if (e.key === "Escape") {
          e.preventDefault();
          cancelSwitch();
        }
        return;
      }

      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
        e.preventDefault();
        if (liveRef.current.modified) saveRig();
        return;
      }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "z" && !e.shiftKey) {
        e.preventDefault();
        if (liveRef.current.modified) revertRig();
        return;
      }
      if (e.ctrlKey || e.metaKey) return;

      if (e.key === "ArrowLeft") {
        e.preventDefault();
        stepPreset(-1);
        return;
      }
      if (e.key === "ArrowRight") {
        e.preventDefault();
        stepPreset(1);
        return;
      }
      if (e.key === " " || e.code === "Space") {
        e.preventDefault();
        toggleBypass();
        return;
      }
      if (e.key >= "1" && e.key <= "7") {
        const idx = Number(e.key) - 1;
        const path = liveRef.current.pathOrder;
        const cat = path[idx] ?? chainOrder[idx];
        if (cat) {
          e.preventDefault();
          selectCategory(cat);
        }
      }
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [
    cancelSwitch,
    revertRig,
    saveRig,
    selectCategory,
    stepPreset,
    toggleBypass,
  ]);

  const pendingName =
    presetsData.find((p) => p.id === currentPresetId)?.name ?? "This rig";

  return (
    <Layout
      presets={presetsData}
      currentPresetId={currentPresetId}
      presetName={currentPreset.name}
      modified={modified}
      dirtyPresetIds={dirtyPresetIds}
      activeCat={activeCat}
      activeModelId={activeModelId}
      stageModels={stageModels}
      pathOrder={pathOrder}
      bypassed={bypassed}
      params={params}
      testing={testing}
      showTestDi={showTestDi}
      vu={vu}
      discardPrompt={
        pendingSwitchId
          ? {
              presetName: pendingName,
              onSave: confirmSaveAndSwitch,
              onDiscard: confirmDiscardAndSwitch,
              onCancel: cancelSwitch,
            }
          : null
      }
      onStepPreset={stepPreset}
      onLoadPreset={loadPreset}
      onToggleTest={() => void toggleTest()}
      onSave={saveRig}
      onRevert={revertRig}
      onSelectCategory={selectCategory}
      onToggleModule={toggleBypassFor}
      onReorderPath={reorderPath}
      onSelectModel={selectModel}
      onToggleBypass={toggleBypass}
      onParamChange={onParamChange}
    />
  );
}

const rootEl = document.getElementById("root");
if (rootEl) {
  createRoot(rootEl).render(
    <StrictMode>
      <RodhareistEditor />
    </StrictMode>,
  );
}
