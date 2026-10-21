#!/usr/bin/env bash
# Collects the Windows Tauri bundle outputs into dist/ under their stable
# names (PRD 0021 RF2). Usage: collect-windows.sh <x64|arm64>
# Signing happens afterwards (scripts/release/windows-sign.ps1).
set -euo pipefail

: "${VERSION:?}"
arch="${1:?x64 or arm64}"

case "$arch" in
  x64)   target_dir="" ;;
  arm64) target_dir="aarch64-pc-windows-msvc/" ;;
  *) echo "unknown arch '$arch'" >&2; exit 1 ;;
esac

bundle="apps/desktop/src-tauri/target/${target_dir}release/bundle"
mkdir -p dist

shopt -s nullglob
nsis=("$bundle"/nsis/*-setup.exe)
[ "${#nsis[@]}" -eq 1 ] || {
  echo "expected exactly one NSIS installer in $bundle/nsis, found ${#nsis[@]}" >&2
  exit 1
}
cp "${nsis[0]}" "dist/Cuotai_${VERSION}_${arch}-setup.exe"
echo "collected Cuotai_${VERSION}_${arch}-setup.exe"

if [ "$arch" = "x64" ]; then
  msis=("$bundle"/msi/*.msi)
  [ "${#msis[@]}" -eq 1 ] || {
    echo "expected exactly one MSI in $bundle/msi, found ${#msis[@]}" >&2
    exit 1
  }
  cp "${msis[0]}" "dist/Cuotai_${VERSION}_x64_en-US.msi"
  echo "collected Cuotai_${VERSION}_x64_en-US.msi"
fi
