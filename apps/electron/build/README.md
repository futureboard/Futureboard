# Electron build resources

This folder is consumed by `electron-builder` (`directories.buildResources`).

The app icon is sourced from `apps/electron/icons/app.png` (1024×1024 PNG).
electron-builder auto-generates the per-platform formats (`.ico`, `.icns`,
multi-size PNGs) at build time. Replace that file to rebrand.

Useful tools: <https://www.electron.build/icons>.

## Building on Windows

electron-builder caches a `winCodeSign` archive on first run that contains
darwin signing helpers as symlinks. Extracting symlinks on Windows requires
**Developer Mode** to be enabled (Settings → System → For developers) or an
elevated terminal. Without it `bun run dist:win` will fail at the
"updating asar integrity" / cache-extract step, even though the unpacked
app under `release/win-unpacked/` is already produced.

Either:

1. Enable Developer Mode and re-run `bun run dist:win`, or
2. Run `bun run pack` for a quick local sanity check — the unpacked app is
   sufficient to launch via `release/win-unpacked/Mochi DAW.exe`.

