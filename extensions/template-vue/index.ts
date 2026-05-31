export type SphereExtensionContext = {
  subscriptions: Array<{ dispose(): void }>;
};

export type SphereExtensionApi = {
  audioPlugins: {
    registerAudioPlugin(descriptor: AudioPluginContribution): { dispose(): void };
  };
};

export type AudioPluginContribution = {
  id: string;
  name: string;
  vendor: string;
  category: "effect" | "instrument" | "analyzer" | "utility";
  params: Array<{
    id: string;
    name: string;
    defaultValue: number;
    min: number;
    max: number;
    unit: string;
  }>;
};

export function activate(context: SphereExtensionContext, api: SphereExtensionApi) {
  const disposable = api.audioPlugins.registerAudioPlugin({
    id: "template.gain",
    name: "Template Gain",
    vendor: "Futureboard Template",
    category: "effect",
    params: [
      { id: "power", name: "Power", defaultValue: 1, min: 0, max: 1, unit: "bool" },
      { id: "gainDb", name: "Gain", defaultValue: 0, min: -24, max: 24, unit: "dB" },
      { id: "mix", name: "Mix", defaultValue: 100, min: 0, max: 100, unit: "%" },
    ],
  });

  context.subscriptions.push(disposable);
}

export function deactivate() {
  // Clean up extension-level resources here. Per-plugin DSP state is owned by DAUx.
}
