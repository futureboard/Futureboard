#!/usr/bin/env bash
# Bundle target/release/FutureboardNative into a portable Linux AppImage.
#
# Usage: bundle-appimage.sh [BIN] [OUT_DIR] [APP_VERSION]
#   BIN         Path to the built native binary.
#               Default: target/release/FutureboardNative
#   OUT_DIR     Directory the finished .AppImage is written to.
#               Default: target/appimage
#   APP_VERSION Version string embedded in the AppImage filename.
#               Default: read from version.json
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

BIN="${1:-$ROOT/target/release/FutureboardNative}"
OUT_DIR="${2:-$ROOT/target/appimage}"
APP_VERSION="${3:-}"

if [[ -z "$APP_VERSION" ]]; then
  # Avoid a hard `node` dependency for a one-line JSON read.
  APP_VERSION="$(grep -oP '"version"\s*:\s*"\K[^"]+' "$ROOT/version.json" 2>/dev/null || true)"
fi
if [[ -z "$APP_VERSION" ]]; then
  echo "error: could not determine app version (pass it as \$3, or ensure version.json is readable)" >&2
  exit 1
fi

DESKTOP_SLUG="futureboard-studio"
PKG_DIR="$ROOT/packaging/linux"
DESKTOP_SRC="$PKG_DIR/futureboard-studio.desktop"
APPRUN_SRC="$PKG_DIR/AppRun"
MIME_SRC="$PKG_DIR/futureboard-studio-mime.xml"
ICON_SRC="$ROOT/packages/shared/app/icons/app.png"

APPDIR="$OUT_DIR/AppDir"

if [[ ! -f "$BIN" ]]; then
  echo "error: native binary not found: $BIN (run: cargo build --release -p futureboard_native)" >&2
  exit 1
fi
for f in "$DESKTOP_SRC" "$APPRUN_SRC" "$MIME_SRC" "$ICON_SRC"; do
  test -f "$f" || { echo "error: missing packaging asset: $f" >&2; exit 1; }
done

rm -rf "$APPDIR"
mkdir -p \
  "$APPDIR/usr/bin" \
  "$APPDIR/usr/share/applications" \
  "$APPDIR/usr/share/icons/hicolor/512x512/apps" \
  "$APPDIR/usr/share/mime/packages"

install -m755 "$BIN" "$APPDIR/usr/bin/FutureboardNative"
install -m755 "$APPRUN_SRC" "$APPDIR/AppRun"

install -m644 "$DESKTOP_SRC" "$APPDIR/${DESKTOP_SLUG}.desktop"
install -m644 "$DESKTOP_SRC" "$APPDIR/usr/share/applications/${DESKTOP_SLUG}.desktop"

install -m644 "$ICON_SRC" "$APPDIR/${DESKTOP_SLUG}.png"
install -m644 "$ICON_SRC" "$APPDIR/usr/share/icons/hicolor/512x512/apps/${DESKTOP_SLUG}.png"

install -m644 "$MIME_SRC" "$APPDIR/usr/share/mime/packages/${DESKTOP_SLUG}.xml"

# appimagetool (fetched on demand, cached under target/ so CI can cache it
# across runs same as the Rust build cache).
APPIMAGETOOL="$ROOT/target/appimagetool-x86_64.AppImage"
if [[ ! -x "$APPIMAGETOOL" ]]; then
  echo "Fetching appimagetool..."
  curl -fsSL -o "$APPIMAGETOOL" \
    https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
  chmod +x "$APPIMAGETOOL"
fi

mkdir -p "$OUT_DIR"
OUTPUT="$OUT_DIR/Futureboard.Studio-${APP_VERSION}-x86_64.AppImage"
rm -f "$OUTPUT"

# `--appimage-extract-and-run` makes appimagetool run itself without FUSE,
# which most CI containers don't have `/dev/fuse` for.
ARCH=x86_64 "$APPIMAGETOOL" --appimage-extract-and-run "$APPDIR" "$OUTPUT"

echo "Built AppImage: $OUTPUT"
