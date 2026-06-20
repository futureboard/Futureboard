Futureboard includes vendored and patched third-party code.

GPUI is vendored under `crates/gpui` from the Zed project and is licensed
under Apache-2.0. Futureboard carries local patches on top of that upstream
library for native DAW integration.

Current Futureboard GPUI patches include:

- Futureboard custom cursor variants in `gpui::CursorStyle`.
- Windows custom cursor loading from bundled PNG cursor assets in
  `packages/shared/cursors`.
- Windows default Arrow cursor replacement using Futureboard's custom Arrow.
- Platform fallbacks for those cursor styles on macOS and Linux.

See `crates/gpui/PATCHED.md` for the GPUI-specific patch record.
