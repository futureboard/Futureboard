#!/usr/bin/env bash
# Create a DMG from the staged .app (macOS only).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="${1:-$ROOT/packaging/native/out}"
APP_NAME="Futureboard Studio"
APP_DIR="$OUT/$APP_NAME.app"
DMG_PATH="$OUT/$APP_NAME.dmg"

if [[ ! -d "$APP_DIR" ]]; then
  echo "error: .app not found — run bundle-macos.sh first: $APP_DIR" >&2
  exit 1
fi

STAGING="$OUT/dmg-staging"
rm -rf "$STAGING" "$DMG_PATH"
mkdir -p "$STAGING"
cp -R "$APP_DIR" "$STAGING/"
ln -s /Applications "$STAGING/Applications"

hdiutil create -volname "$APP_NAME" -srcfolder "$STAGING" -ov -format UDZO "$DMG_PATH"
rm -rf "$STAGING"

echo "Created DMG: $DMG_PATH"
