#!/usr/bin/env bash
# Bundle the Community Edition FutureboardNative into a portable Linux AppImage.
#
# Usage: bundle-appimage.sh [PACKAGE_DIR] [OUT_DIR] [APP_VERSION]
#   PACKAGE_DIR Complete runtime tree produced by `xtask package`.
#               Default: out/release/community/linux-x64
#   OUT_DIR     Directory the finished .AppImage is written to.
#               Default: target/appimage
#   APP_VERSION Version string embedded in the AppImage filename.
#               Default: read from version.json
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

PACKAGE_DIR="${1:-$ROOT/out/release/community/linux-x64}"
OUT_DIR="${2:-$ROOT/target/appimage}"
APP_VERSION="${3:-}"
BIN="$PACKAGE_DIR/FutureboardNative"

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
  echo "error: xtask runtime package not found: $BIN" >&2
  echo "run: cargo xtask package --profile release --edition community --plugin all" >&2
  exit 1
fi
if [[ ! -f "$PACKAGE_DIR/build-info.json" ]]; then
  echo "error: missing xtask package metadata: $PACKAGE_DIR/build-info.json" >&2
  exit 1
fi
for f in "$DESKTOP_SRC" "$APPRUN_SRC" "$MIME_SRC" "$ICON_SRC"; do
  test -f "$f" || { echo "error: missing packaging asset: $f" >&2; exit 1; }
done

rm -rf "$APPDIR"
mkdir -p \
  "$APPDIR/usr/bin" \
  "$APPDIR/usr/lib" \
  "$APPDIR/usr/share/applications" \
  "$APPDIR/usr/share/icons/hicolor/512x512/apps" \
  "$APPDIR/usr/share/mime/packages"

# Preserve the validated xtask layout as one unit. CEF resources/locales,
# helper processes, built-in plugins, runtime libraries and build-info.json
# must remain beside the main executable exactly as xtask staged them.
cp -a "$PACKAGE_DIR/." "$APPDIR/usr/bin/"
chmod +x \
  "$APPDIR/usr/bin/FutureboardNative" \
  "$APPDIR/usr/bin/FutureboardPluginHostX64" \
  "$APPDIR/usr/bin/FutureboardPluginScanner"

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
echo
echo "AppImage runtime binaries:"
ls -la "$APPDIR/usr/bin"
echo
echo "AppImage runtime libraries:"
ls -la "$APPDIR/usr/lib"
