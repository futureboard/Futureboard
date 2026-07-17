#!/usr/bin/env bash
# Shared submodule bootstrap for CI (no VST3 docs / full history).
set -euo pipefail

# Required for Community Edition builds / clippy / tests.
REQUIRED_SUBMODULES=(
  external/vst3sdk
  external/clap
  external/clap-helpers
  external/yoga
  packages/shared/tabler-icons
  packages/shared/lucide
)

# Optional: local path checkout when the workspace still patches cpal via
# `external/cpal`. Production CI also accepts the git patch in Cargo.toml
# (`git+https://github.com/futureboard/cpal`), so a missing clone must not fail
# the whole job when the patch does not need the path.
OPTIONAL_SUBMODULES=(
  external/cpal
)

echo "Syncing submodule URLs..."
git submodule sync -- "${REQUIRED_SUBMODULES[@]}" "${OPTIONAL_SUBMODULES[@]}" || true

echo "Initializing required submodules (shallow where safe)..."
git submodule update --init --force --depth=1 --checkout -- \
  external/clap \
  external/clap-helpers \
  external/yoga \
  packages/shared/tabler-icons \
  packages/shared/lucide

# VST3 SDK needs full checkout for nested SDK submodules.
git submodule update --init --force --checkout -- external/vst3sdk

git -C external/vst3sdk submodule update --init --force --checkout -- \
  base cmake pluginterfaces public.sdk tutorials vstgui4

for path in "${OPTIONAL_SUBMODULES[@]}"; do
  if git submodule update --init --force --depth=1 --checkout -- "$path"; then
    echo "Optional submodule ready: $path"
  else
    echo "Optional submodule skipped (non-fatal): $path"
  fi
done

echo "Submodules ready."
