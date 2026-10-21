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

# Select exactly one final regular non-symlink package by expected extension
# pattern. The `-setup.exe` suffix and `.msi` are the patterns Tauri's NSIS
# and MSI bundlers always emit, and they are how we ignore staging sidecars.
# Symlinks and anything that resolves outside the bundle directory are
# rejected: they could otherwise let an attacker copy an arbitrary file in.
select_final() { # <bundle-subdirectory> <glob-pattern-relative>
  local dir="$1" expected_pat="$2"
  shopt -s nullglob
  local candidates=()
  local entry
  for entry in "$bundle/$dir/"$expected_pat; do
    if [ -L "$entry" ] || [ ! -f "$entry" ]; then
      echo "$entry is not a regular, non-symlink file" >&2
      exit 1
    fi
    local target_dir_abs expected_prefix
    target_dir_abs="$(cd "$bundle/$dir" && pwd -P)"
    expected_prefix="${target_dir_abs}/"
    case "$(readlink -f -- "$entry")/" in
      "$expected_prefix"*) ;;
      *) echo "$entry resolves outside $bundle/$dir" >&2; exit 1 ;;
    esac
    candidates+=("$entry")
  done
  case "${#candidates[@]}" in
    0)
      echo "expected one $expected_pat package in $bundle/$dir, found 0" >&2
      exit 1
      ;;
    1) printf '%s\n' "${candidates[0]}" ;;
    *)
      printf '%s\n' "${candidates[@]}" >&2
      echo "expected one $expected_pat package in $bundle/$dir, found ${#candidates[@]}" >&2
      exit 1
      ;;
  esac
}

nsis="$(select_final nsis '*-setup.exe')"
cp "$nsis" "dist/Cuotai_${VERSION}_${arch}-setup.exe"
echo "collected dist/Cuotai_${VERSION}_${arch}-setup.exe"

if [ "$arch" = "x64" ]; then
  msi="$(select_final msi '*.msi')"
  cp "$msi" "dist/Cuotai_${VERSION}_x64_en-US.msi"
  echo "collected dist/Cuotai_${VERSION}_x64_en-US.msi"
fi
