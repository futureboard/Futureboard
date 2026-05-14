This repository is Futureboard Studio / Mochi DAW.

Before making changes, read `CLAUDE.md` for the full project guide.

Important rules:
- Keep TypeScript clean.
- Do not rewrite unrelated UI.
- Do not change audio engine architecture unless asked.
- Audio clips and MIDI clips must be routed separately.
- MIDI clips must never use waveform rendering.
- Use platform adapters instead of direct Electron/Node/browser APIs.
- Current app is a React/Electron prototype and living spec.
- Future runtime will move to WASM/native DSP/SphereEngine later.
