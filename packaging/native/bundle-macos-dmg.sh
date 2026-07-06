#!/usr/bin/env bash
# Bundle Futureboard Studio.app from macOS release output.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SRC_DIR="${1:-$ROOT/target/release}"
OUT="${2:-$ROOT/packaging/native/out}"

APP_NAME="Futureboard Studio"
APP_EXE="FutureboardNative"
APP_DIR="$OUT/$APP_NAME.app"

PLIST_SRC="$ROOT/packaging/native/Info.plist"
ICON_SRC="$ROOT/packages/shared/app/icons/icon.icns"

CONTENTS="$APP_DIR/Contents"
MACOS="$CONTENTS/MacOS"
FRAMEWORKS="$CONTENTS/Frameworks"
RESOURCES="$CONTENTS/Resources"

rm -rf "$APP_DIR"
mkdir -p "$MACOS" "$FRAMEWORKS" "$RESOURCES"

if [[ ! -f "$SRC_DIR/$APP_EXE" ]]; then
  echo "error: main executable not found: $SRC_DIR/$APP_EXE" >&2
  exit 1
fi

if [[ ! -f "$PLIST_SRC" ]]; then
  echo "error: Info.plist not found: $PLIST_SRC" >&2
  exit 1
fi

# Existing Info.plist
cp "$PLIST_SRC" "$CONTENTS/Info.plist"

# Main executable
cp "$SRC_DIR/$APP_EXE" "$MACOS/"
chmod +x "$MACOS/$APP_EXE"

# Other Mach-O helper binaries in release root
while IFS= read -r bin; do
  name="$(basename "$bin")"

  if [[ "$name" == "$APP_EXE" ]]; then
    continue
  fi

  cp "$bin" "$MACOS/"
  chmod +x "$MACOS/$name"
done < <(
  find "$SRC_DIR" -maxdepth 1 -type f -perm -111 \
    ! -name "*.dylib" \
    ! -name "*.a" \
    ! -name "*.rlib" \
    ! -name "*.dSYM" \
    -print
)

# Dynamic libraries
find "$SRC_DIR" -maxdepth 1 -type f -name "*.dylib" -print0 | while IFS= read -r -d '' dylib; do
  cp "$dylib" "$FRAMEWORKS/"
done

# Optional icon
if [[ -f "$ICON_SRC" ]]; then
  cp "$ICON_SRC" "$RESOURCES/icon.icns"
fi

echo "Created app bundle: $APP_DIR"
echo
echo "MacOS:"
ls -la "$MACOS"
echo
echo "Frameworks:"
ls -la "$FRAMEWORKS"
echo
echo "Resources:"
ls -la "$RESOURCES"
