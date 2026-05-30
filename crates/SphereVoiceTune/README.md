# SphereVoiceTune

`SphereVoiceTune` is an experimental research crate built for Futureboard Studio to explore Melodyne-like vocal pitch analysis, note segmentation, and non-destructive vocal correction/editing.

> [!WARNING]
> This crate is an **isolated research experiment**. It is not connected to the live audio engine, plugin host, arrangement UI, mixer, project file serialization, or any production code path. It has zero impact on the DAW's existing playback or rendering systems.

## Core Features

1. **Audio Analysis Input**: Accepts raw monophonic float sample buffers (`&[f32]`) and sample rates. It does not perform file I/O.
2. **Pitch Detection**: Implements a robust monophonic pitch detection algorithm (simplified YIN) with parabolic interpolation for sub-sample accuracy.
3. **Note Segmentation**: Groups contiguous voiced pitch frames with stable frequencies into distinct `VoiceNote` objects, ignoring silent or unvoiced/low-energy parts.
4. **Correction Model**: Represents non-destructive correction operations (e.g. pitch snap target note, pitch drift, vibrato, stretch, formant shift, and gain).
5. **Render Plan Only**: Outputs a `VoiceTuneRenderPlan` describing the requested edits instead of modifying audio buffers destructively in real-time.

## Future Integration Plan

This crate serves as a foundational step. Future integration stages will include:
1. **DSP Engine Integration**: Implementing high-quality PSOLA or Phase Vocoder pitch/time stretching in a background service or worker.
2. **UI Integration**: Integrating with a Melodyne-like scrollable canvas component in `SphereUIComponents` (represented as a bottom panel vocal editor tab).
3. **Project Save/Load**: Adding serialization structures (`serde` support) to embed voice tune document states in project files.
