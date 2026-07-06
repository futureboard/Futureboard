#!/usr/bin/env bash
# Bundle target/release/FutureboardNative into a macOS .app using shared app assets.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

BIN="${1:-$ROOT/target/release/FutureboardNative}"
OUT="${2:-$ROOT/packaging/native/out}"

APP_NAME="Futureboard Studio"
APP_DIR="$OUT/$APP_NAME.app"

SRC_DIR="$(cd "$(dirname "$BIN")" && pwd)"
MAIN_BIN_NAME="$(basename "$BIN")"

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

if [[ ! -f "$BIN" ]]; then
  echo "error: native binary not found: $BIN" >&2
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

# Main executable
cp "$BIN" "$MACOS/$APP_EXECUTABLE_NAME"
chmod +x "$MACOS/$APP_EXECUTABLE_NAME"

# Other Mach-O helper binaries in target/release root.
# macOS binaries often have no extension, so copy executable files.
while IFS= read -r helper; do
  helper_name="$(basename "$helper")"

  if [[ "$helper_name" == "$MAIN_BIN_NAME" ]]; then
    continue
  fi

  # Skip obvious non-runtime build files
  case "$helper_name" in
    *.dylib|*.a|*.rlib|*.dSYM|*.rmeta|*.o|*.d)
      continue
      ;;
  esac

  cp "$helper" "$MACOS/$helper_name"
  chmod +x "$MACOS/$helper_name"
done < <(
  find "$SRC_DIR" -maxdepth 1 -type f -perm -111 -print
)

# Dynamic libraries
find "$SRC_DIR" -maxdepth 1 -type f -name "*.dylib" -print0 | while IFS= read -r -d '' dylib; do
  cp "$dylib" "$FRAMEWORKS/"
done

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
