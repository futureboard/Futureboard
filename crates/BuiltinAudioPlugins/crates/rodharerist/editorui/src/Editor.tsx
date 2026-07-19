import { StrictMode, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import {
  categories,
  cloneParameters,
  models,
  presetsData,
  type CategoryId,
  type Param,
} from "./data";
import { Layout } from "./Layout";
import { postEnabled, postModel, postParam } from "./bridge";
import "./Styles/Editor.css";

type VuState = {
  inL: number;
  inR: number;
  outL: number;
  outR: number;
};

function applyCategoryTheme(cat: CategoryId) {
  const c = categories[cat];
  document.documentElement.style.setProperty("--cat-color", c.color);
  document.documentElement.style.setProperty("--cat-rgb", c.rgb);
}

function RodhareistEditor() {
  const [currentPresetId, setCurrentPresetId] = useState("06D");
  const [activeCat, setActiveCat] = useState<CategoryId>("amp");
  const [activeModelId, setActiveModelId] = useState("mandarin");
  const [bypassed, setBypassed] = useState<Partial<Record<CategoryId, boolean>>>(
    {},
  );
  const [parameters, setParameters] = useState<Record<string, Param[]>>(
    () => cloneParameters(),
  );
  const [testing, setTesting] = useState(false);
  const [vu, setVu] = useState<VuState>({
    inL: 0,
    inR: 0,
    outL: 0,
    outR: 0,
  });

  const audioRef = useRef<{
    ctx: AudioContext;
    gain: GainNode;
    timer: number | null;
  } | null>(null);
  const levelsRef = useRef({ inLvl: 0, outLvl: 0 });

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

  const selectCategory = useCallback((cat: CategoryId) => {
    setActiveCat(cat);
    const first = models[cat]?.[0];
    if (first) {
      setActiveModelId(first.id);
      postModel(categories[cat].node, first.id);
    }
  }, []);

  const loadPreset = useCallback((id: string) => {
    const p = presetsData.find((x) => x.id === id);
    if (!p) return;
    setCurrentPresetId(id);
    setActiveCat(p.category);
    setActiveModelId(p.model);
    setParameters((prev) => {
      const next = { ...prev };
      const modelParams = next[p.model];
      if (modelParams && p.values) {
        next[p.model] = modelParams.map((param) =>
          p.values[param.id] !== undefined
            ? { ...param, val: p.values[param.id]! }
            : param,
        );
      }
      return next;
    });
  }, []);

  const stepPreset = useCallback(
    (dir: number) => {
      const idx = presetsData.findIndex((p) => p.id === currentPresetId);
      const next =
        (idx + dir + presetsData.length) % presetsData.length;
      loadPreset(presetsData[next]!.id);
    },
    [currentPresetId, loadPreset],
  );

  const selectModel = useCallback(
    (id: string) => {
      setActiveModelId(id);
      postModel(categories[activeCat].node, id);
    },
    [activeCat],
  );

  const toggleBypass = useCallback(() => {
    setBypassed((prev) => {
      const enabled = !prev[activeCat]; // next bypassed state
      // Bridge sends "enabled" (the inverse of bypassed).
      postEnabled(categories[activeCat].node, !enabled);
      return { ...prev, [activeCat]: enabled };
    });
  }, [activeCat]);

  const onParamChange = useCallback(
    (id: string, value: number) => {
      postParam(id, value);
      setParameters((prev) => {
        const modelParams = prev[activeModelId];
        if (!modelParams) return prev;
        return {
          ...prev,
          [activeModelId]: modelParams.map((p) =>
            p.id === id ? { ...p, val: value } : p,
          ),
        };
      });
    },
    [activeModelId],
  );

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

  return (
    <Layout
      presets={presetsData}
      currentPresetId={currentPresetId}
      presetName={currentPreset.name}
      activeCat={activeCat}
      activeModelId={activeModelId}
      bypassed={bypassed}
      params={params}
      testing={testing}
      vu={vu}
      onStepPreset={stepPreset}
      onLoadPreset={loadPreset}
      onToggleTest={() => void toggleTest()}
      onSelectCategory={selectCategory}
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
