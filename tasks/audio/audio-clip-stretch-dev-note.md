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

Preserve-pitch clips now route through the **Signalsmith** backend by default
(time-stretch + independent pitch transpose). Its ~120 ms algorithmic latency is
compensated per-clip via `output_seek` pre-roll priming in
`render_signalsmith_clip_segment`, so the next `process` output aligns to the
playback position on every (re)start and stretched clips no longer drift behind
the mix. Set `FUTUREBOARD_STRETCH_SIGNALSMITH=0` to force the old crude OLA
fallback (`PhaseVocoderBasic`) for A/B comparison; it is zero-latency but warbly
and now only used when Signalsmith is unavailable or explicitly disabled.

### Preserve-pitch / pitch-shift QA

- Manual mode, ratio 100%, set pitch to +7 st: pitch rises, length unchanged, no
  warble; clip stays in sync with a metronome/other tracks (no ~120 ms drag).
- Slow to 50% / speed to 200% with Preserve Pitch on: tempo changes, pitch holds.
- Seek into the middle of a stretched clip and loop it: re-priming keeps the
  output aligned at each (re)start with no growing offset.

Existing `ClipState::gain` and track pan remain canonical. `stretch.gain_db`,
`stretch.pan`, and `stretch.normalize_gain` stay stored but inert so gain is not
applied twice.
