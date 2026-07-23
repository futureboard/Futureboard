#!/usr/bin/env bash
set -euo pipefail

cef_version="150.0.11"

case "${RUNNER_OS:?RUNNER_OS is required}-${RUNNER_ARCH:?RUNNER_ARCH is required}" in
  Windows-X64)
    cef_platform="cef_windows_x86_64"
    ;;
  Linux-X64)
    cef_platform="cef_linux_x86_64"
    ;;
  macOS-X64)
    cef_platform="cef_macos_x86_64"
    ;;
  macOS-ARM64)
    cef_platform="cef_macos_aarch64"
    ;;
  *)
    echo "Unsupported CEF runner: ${RUNNER_OS}-${RUNNER_ARCH}" >&2
    exit 1
    ;;
esac

cef_path="${GITHUB_WORKSPACE:?GITHUB_WORKSPACE is required}/build/cef/${cef_version}/${cef_platform}"
echo "CEF_PATH=${cef_path}" >> "${GITHUB_ENV:?GITHUB_ENV is required}"
echo "CEF_PATH=${cef_path}"
