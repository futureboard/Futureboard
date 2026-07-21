import type { CategoryId, Param, Preset } from "./data";
import type { NamCaptureLoadOptions } from "./bridge";
import type { AbSlot } from "./state/history";
import { DiscardDialog } from "./Components/DiscardDialog";
import { Footer } from "./Components/Footer";
import { Header } from "./Components/Header";
import { IoStrip } from "./Components/IoStrip";
import { ModuleEditor } from "./Components/ModuleEditor";
import { PresetBrowser } from "./Components/PresetBrowser";
import { SignalChain } from "./Components/SignalChain";

// Supports weights 100-700
import '@fontsource-variable/ibm-plex-sans/wght.css';

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
  inputTrim: number;
  outputTrim: number;
  globalBypass: boolean;
  canUndo: boolean;
  canRedo: boolean;
  abSlot: AbSlot;
  clipboardCat: CategoryId | null;
  discardPrompt: DiscardPrompt | null;
  onUndo: () => void;
  onRedo: () => void;
  onSelectAb: (slot: AbSlot) => void;
  onCopyAb: () => void;
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
  onToggleGlobalBypass: () => void;
  onParamChange: (id: string, value: number) => void;
  onGlobalParamChange: (id: string, value: number) => void;
  onCopySettings: (cat: CategoryId) => void;
  onPasteSettings: (cat: CategoryId) => void;
  onResetModule: (cat: CategoryId) => void;
  onLoadNamCapture: (json: string, opts: NamCaptureLoadOptions) => void;
  onBypassCab: () => void;
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
  inputTrim,
  outputTrim,
  globalBypass,
  canUndo,
  canRedo,
  abSlot,
  clipboardCat,
  discardPrompt,
  onUndo,
  onRedo,
  onSelectAb,
  onCopyAb,
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
  onToggleGlobalBypass,
  onParamChange,
  onGlobalParamChange,
  onCopySettings,
  onPasteSettings,
  onResetModule,
  onLoadNamCapture,
  onBypassCab,
}: LayoutProps) {
  return (
    <div className="plugin">
      <Header
        presetId={currentPresetId}
        presetName={presetName}
        modified={modified}
        testing={testing}
        showTestDi={showTestDi}
        canUndo={canUndo}
        canRedo={canRedo}
        abSlot={abSlot}
        onUndo={onUndo}
        onRedo={onRedo}
        onSelectAb={onSelectAb}
        onCopyAb={onCopyAb}
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
          {/* Gain staging brackets the chain so input and output levels are
              always on screen, in the order the signal actually travels. */}
          <div className="chain-region">
            <IoStrip side="in" trim={inputTrim} onTrimChange={onGlobalParamChange} />
            <SignalChain
              pathOrder={pathOrder}
              activeCat={activeCat}
              stageModels={stageModels}
              bypassed={bypassed}
              clipboardCat={clipboardCat}
              onSelectCategory={onSelectCategory}
              onToggleModule={onToggleModule}
              onReorderPath={onReorderPath}
              onCopySettings={onCopySettings}
              onPasteSettings={onPasteSettings}
              onResetModule={onResetModule}
            />
            <IoStrip
              side="out"
              trim={outputTrim}
              onTrimChange={onGlobalParamChange}
              globalBypass={globalBypass}
              onToggleGlobalBypass={onToggleGlobalBypass}
            />
          </div>

          <ModuleEditor
            activeCat={activeCat}
            activeModelId={activeModelId}
            bypassed={!!bypassed[activeCat]}
            params={params}
            onSelectModel={onSelectModel}
            onToggleBypass={onToggleBypass}
            onParamChange={onParamChange}
            onLoadNamCapture={onLoadNamCapture}
            onBypassCab={onBypassCab}
          />
        </main>
      </div>

      <Footer globalBypass={globalBypass} />

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
