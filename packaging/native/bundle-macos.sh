#!/usr/bin/env bash
# Bundle target/release/FutureboardNative into a macOS .app using apps/shared assets.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BIN="${1:-$ROOT/target/release/FutureboardNative}"
OUT="${2:-$ROOT/packaging/native/out}"
APP_NAME="Futureboard Studio"
APP_DIR="$OUT/$APP_NAME.app"

if [[ ! -f "$BIN" ]]; then
  echo "error: native binary not found: $BIN" >&2
  exit 1
fi

ICON_SRC="$ROOT/apps/shared/icon.icns"
PLIST_SRC="$ROOT/packaging/native/Info.plist"

rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS" "$APP_DIR/Contents/Resources"

cp "$PLIST_SRC" "$APP_DIR/Contents/Info.plist"
cp "$BIN" "$APP_DIR/Contents/MacOS/futureboard_native"
chmod +x "$APP_DIR/Contents/MacOS/futureboard_native"

if [[ -f "$ICON_SRC" ]]; then
  cp "$ICON_SRC" "$APP_DIR/Contents/Resources/icon.icns"
else
  echo "warning: missing $ICON_SRC — app will use default icon" >&2
fi

echo "Bundled macOS app: $APP_DIR"
