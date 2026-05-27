#!/usr/bin/env bash
# Shared submodule bootstrap for CI (no VST3 docs / full history).
set -euo pipefail

git submodule sync -- \
  external/vst3sdk \
  external/clap \
  external/clap-helpers \
  external/yoga \
  packages/shared/tabler-icons \
  packages/shared/lucide

git submodule update --init --force --depth=1 --checkout -- \
  external/clap \
  external/clap-helpers \
  external/yoga \
  packages/shared/tabler-icons \
  packages/shared/lucide

git submodule update --init --force --checkout -- external/vst3sdk

git -C external/vst3sdk submodule update --init --force --checkout -- \
  base cmake pluginterfaces public.sdk tutorials vstgui4

echo "Submodules ready."
