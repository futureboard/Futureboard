import type { CategoryId, Param, Preset } from "./data";
import { DiscardDialog } from "./Components/DiscardDialog";
import { Footer } from "./Components/Footer";
import { Header } from "./Components/Header";
import { ModuleEditor } from "./Components/ModuleEditor";
import { PresetBrowser } from "./Components/PresetBrowser";
import { SignalChain } from "./Components/SignalChain";

export type DiscardPrompt = {
  presetName: string;
  onSave: () => void;
  onDiscard: () => void;
  onCancel: () => void;
};

export type LayoutProps = {
  presets: Preset[];
  currentPresetId: string;
  presetName: string;
  modified: boolean;
  dirtyPresetIds: ReadonlySet<string>;
  activeCat: CategoryId;
  activeModelId: string;
  stageModels: Record<CategoryId, string>;
  pathOrder: CategoryId[];
  bypassed: Partial<Record<CategoryId, boolean>>;
  params: Param[];
  testing: boolean;
  showTestDi: boolean;
  vu: {
    inL: number;
    inR: number;
    outL: number;
    outR: number;
  };
  discardPrompt: DiscardPrompt | null;
  onStepPreset: (dir: number) => void;
  onLoadPreset: (id: string) => void;
  onToggleTest: () => void;
  onSave: () => void;
  onRevert: () => void;
  onSelectCategory: (cat: CategoryId) => void;
  onToggleModule: (cat: CategoryId) => void;
  onReorderPath: (next: CategoryId[]) => void;
  onSelectModel: (id: string) => void;
  onToggleBypass: () => void;
  onParamChange: (id: string, value: number) => void;
};

export function Layout({
  presets,
  currentPresetId,
  presetName,
  modified,
  dirtyPresetIds,
  activeCat,
  activeModelId,
  stageModels,
  pathOrder,
  bypassed,
  params,
  testing,
  showTestDi,
  vu,
  discardPrompt,
  onStepPreset,
  onLoadPreset,
  onToggleTest,
  onSave,
  onRevert,
  onSelectCategory,
  onToggleModule,
  onReorderPath,
  onSelectModel,
  onToggleBypass,
  onParamChange,
}: LayoutProps) {
  return (
    <div className="plugin">
      <Header
        presetId={currentPresetId}
        presetName={presetName}
        modified={modified}
        testing={testing}
        showTestDi={showTestDi}
        onStepPreset={onStepPreset}
        onToggleTest={onToggleTest}
        onSave={onSave}
        onRevert={onRevert}
      />

      <div className="workspace">
        <PresetBrowser
          presets={presets}
          currentPresetId={currentPresetId}
          modifiedIds={dirtyPresetIds}
          onLoadPreset={onLoadPreset}
        />

        <main className="dashboard">
          <SignalChain
            pathOrder={pathOrder}
            activeCat={activeCat}
            stageModels={stageModels}
            bypassed={bypassed}
            vu={vu}
            onSelectCategory={onSelectCategory}
            onToggleModule={onToggleModule}
            onReorderPath={onReorderPath}
          />
          <ModuleEditor
            activeCat={activeCat}
            activeModelId={activeModelId}
            bypassed={!!bypassed[activeCat]}
            params={params}
            onSelectModel={onSelectModel}
            onToggleBypass={onToggleBypass}
            onParamChange={onParamChange}
          />
        </main>
      </div>

      <Footer />

      {discardPrompt && (
        <DiscardDialog
          presetName={discardPrompt.presetName}
          onSave={discardPrompt.onSave}
          onDiscard={discardPrompt.onDiscard}
          onCancel={discardPrompt.onCancel}
        />
      )}
    </div>
  );
}
