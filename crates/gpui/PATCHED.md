# Patched GPUI

This vendored GPUI library has been modified for Futureboard Studio.

## Custom Cursor Patch

Futureboard added app-specific cursor styles to `gpui::CursorStyle`:

- `FutureboardArrow`
- `FutureboardSelect`
- `FutureboardMarquee`
- `FutureboardMove`
- `FutureboardFadeIn`
- `FutureboardFadeOut`
- `FutureboardResizeHorizon`
- `FutureboardResizeLeft`
- `FutureboardResizeRight`

On Windows, these styles are rendered as native `HCURSOR` handles decoded from
bundled PNG assets in `packages/shared/cursors`. Runtime cursor selection uses
the `@0.5x` assets as the default size so the cursors match DAW chrome density.
The standard `CursorStyle::Arrow` is also mapped to the Futureboard custom Arrow
cursor on Windows.

On macOS and Linux, the same styles currently fall back to the closest native
system cursor so the API remains cross-platform.

## Maintenance Notes

When updating GPUI from upstream, preserve these Futureboard patches or port
them forward deliberately. Do not remove the custom cursor variants unless
Futureboard has a replacement cursor pipeline.
