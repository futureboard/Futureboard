# Audio Clip Stretch Dev Note

Date: 2026-06-13

## Manual QA Hook

Enable:

```powershell
$env:FUTUREBOARD_CLIP_DSP_DEBUG = "1"
```

The engine logs selected clip DSP state when the graph snapshot builds and when offline export starts. The log includes clip id/name, mode, algorithm, effective time ratio, pitch ratio, speed ratio, preserve-pitch, reverse, duration samples, source window, and resolved processor.

## QA Checklist

- Import a short WAV or RAUF clip.
- Set stretch to 200%; the clip should become 2x longer and playback should slow down.
- Export the arrangement; exported audio length should match the stretched timeline.
- Enable reverse; playback and export should read backwards, and the waveform should flip.
- Toggle Preserve Pitch; the path should resolve to `PhaseVocoderBasic`.

## Current Result

`PhaseVocoderBasic` is now a basic streaming OLA/granular stretcher instead of the previous resample fallback. It is intentionally rough and allocation-free in the render callback. Independent pitch shifting on top of preserve-pitch mode is still pending and is debug-labeled as `pitch_shift=pending` when non-zero.

Existing `ClipState::gain` and track pan remain canonical. `stretch.gain_db`, `stretch.pan`, and `stretch.normalize_gain` stay stored but inert so gain is not applied twice.
