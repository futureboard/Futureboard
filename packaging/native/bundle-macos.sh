#!/usr/bin/env bash
# Bundle the xtask-staged Community Edition runtime into a macOS .app.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

PACKAGE_DIR="${1:-}"
OUT="${2:-$ROOT/packaging/native/out}"

if [[ -z "$PACKAGE_DIR" ]]; then
  PACKAGE_DIR="$(find "$ROOT/out/release/community" -mindepth 1 -maxdepth 1 -type d -name 'macos-*' -print -quit 2>/dev/null || true)"
fi

APP_NAME="Futureboard Studio"
APP_DIR="$OUT/$APP_NAME.app"

# IMPORTANT:
# This must match CFBundleExecutable in Info.plist.
# If Info.plist says futureboard_native, keep this.
# If Info.plist says FutureboardNative, change this to FutureboardNative.
APP_EXECUTABLE_NAME="FutureboardNative"

ICON_SRC="$ROOT/packages/shared/app/icons/icon.icns"
PLIST_SRC="$ROOT/packaging/native/Info.plist"

CONTENTS="$APP_DIR/Contents"
MACOS="$CONTENTS/MacOS"
RESOURCES="$CONTENTS/Resources"
FRAMEWORKS="$CONTENTS/Frameworks"

if [[ -z "$PACKAGE_DIR" || ! -f "$PACKAGE_DIR/FutureboardNative" ]]; then
  echo "error: xtask macOS runtime package not found: ${PACKAGE_DIR:-<none>}" >&2
  echo "run: cargo xtask package --profile release --edition community --plugin all" >&2
  exit 1
fi

if [[ ! -f "$PACKAGE_DIR/build-info.json" ]]; then
  echo "error: missing xtask package metadata: $PACKAGE_DIR/build-info.json" >&2
  exit 1
fi

if [[ ! -f "$PLIST_SRC" ]]; then
  echo "error: Info.plist not found: $PLIST_SRC" >&2
  exit 1
fi

rm -rf "$APP_DIR"
mkdir -p "$MACOS" "$RESOURCES" "$FRAMEWORKS"

# Existing Info.plist
cp "$PLIST_SRC" "$CONTENTS/Info.plist"

# Preserve the complete validated xtask runtime layout beside the executable.
# This includes helper processes, CEF files/locales, built-in plugins, runtime
# libraries and build-info.json.
cp -a "$PACKAGE_DIR/." "$MACOS/"
chmod +x "$MACOS/$APP_EXECUTABLE_NAME"
chmod +x "$MACOS/FutureboardPluginHostX64" "$MACOS/FutureboardPluginScanner"

# Icon
if [[ -f "$ICON_SRC" ]]; then
  cp "$ICON_SRC" "$RESOURCES/icon.icns"
else
  echo "warning: missing $ICON_SRC — app will use default icon" >&2
fi

echo "Bundled macOS app: $APP_DIR"
echo
echo "Contents/MacOS:"
ls -la "$MACOS"
echo
echo "Contents/Frameworks:"
ls -la "$FRAMEWORKS"
echo
echo "Contents/Resources:"
ls -la "$RESOURCES"
