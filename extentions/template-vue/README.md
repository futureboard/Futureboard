# Futureboard Extension Template

This folder is a VSCode-style extension template for Futureboard Studio.

> Note: the repository currently uses the historical folder name `extentions/`.

## Files

- `sphere-extension.json` — extension manifest, similar in spirit to VSCode `package.json` contributions.
- `index.ts` — activation entry point.
- `src/editor/App.tsx` — React editor surface for the plugin UI.
- `src/backend/lib.rs` — native Rust DSP backend template.

## Audio Plugin Contribution

Declare plugins under `contributes.audioPlugins`:

```json
{
  "id": "template.gain",
  "name": "Template Gain",
  "category": "effect",
  "backend": "./src/backend",
  "editor": "./src/editor/App.tsx",
  "params": [
    { "id": "gainDb", "name": "Gain", "defaultValue": 0, "min": -24, "max": 24, "unit": "dB" }
  ]
}
```

DAUx owns realtime DSP state. Extension code must treat the audio callback as realtime-critical:

- no heap allocation in the sample loop
- no locks
- no filesystem or network I/O
- no logging from realtime process functions

## Build

```sh
bun run typecheck
cargo build
```

Future work: package this folder into a `.fbx` extension archive and load it through a Futureboard extension host.
