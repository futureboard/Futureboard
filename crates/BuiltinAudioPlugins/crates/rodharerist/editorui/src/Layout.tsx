import type { CategoryId, Param, Preset } from "./data";
import { Footer } from "./Components/Footer";
import { Header } from "./Components/Header";
import { ModuleEditor } from "./Components/ModuleEditor";
import { PresetBrowser } from "./Components/PresetBrowser";
import { SignalChain } from "./Components/SignalChain";

export type LayoutProps = {
  presets: Preset[];
  currentPresetId: string;
  presetName: string;
  activeCat: CategoryId;
  activeModelId: string;
  bypassed: Partial<Record<CategoryId, boolean>>;
  params: Param[];
  testing: boolean;
  vu: {
    inL: number;
    inR: number;
    outL: number;
    outR: number;
  };
  onStepPreset: (dir: number) => void;
  onLoadPreset: (id: string) => void;
  onToggleTest: () => void;
  onSelectCategory: (cat: CategoryId) => void;
  onSelectModel: (id: string) => void;
  onToggleBypass: () => void;
  onParamChange: (id: string, value: number) => void;
};

export function Layout({
  presets,
  currentPresetId,
  presetName,
  activeCat,
  activeModelId,
  bypassed,
  params,
  testing,
  vu,
  onStepPreset,
  onLoadPreset,
  onToggleTest,
  onSelectCategory,
  onSelectModel,
  onToggleBypass,
  onParamChange,
}: LayoutProps) {
  return (
    <div className="plugin">
      <Header
        presetId={currentPresetId}
        presetName={presetName}
        testing={testing}
        onStepPreset={onStepPreset}
        onToggleTest={onToggleTest}
      />

      <div className="workspace">
        <PresetBrowser
          presets={presets}
          currentPresetId={currentPresetId}
          onLoadPreset={onLoadPreset}
        />

        <main className="dashboard">
          <SignalChain
            activeCat={activeCat}
            bypassed={bypassed}
            vu={vu}
            onSelectCategory={onSelectCategory}
          />
          <ModuleEditor
            activeCat={activeCat}
            activeModelId={activeModelId}
            bypassed={!!bypassed[activeCat]}
            params={params}
            onSelectCategory={onSelectCategory}
            onSelectModel={onSelectModel}
            onToggleBypass={onToggleBypass}
            onParamChange={onParamChange}
          />
        </main>
      </div>

      <Footer />
    </div>
  );
}
