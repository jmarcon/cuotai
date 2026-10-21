#!/usr/bin/env bash
# Collects the Linux Tauri bundle outputs into dist/ under their stable
# names (PRD 0021 RF2) and writes the signing fragment. Linux artifacts are
# unsigned files whose trust comes from the signed SHA256SUMS (RF3.1); the
# merge resolves their final state. Usage: collect-linux.sh <x64|arm64>
set -euo pipefail

: "${VERSION:?}"
arch="${1:?x64 or arm64}"

case "$arch" in
  x64)   target_dir=""; stable_arch="amd64" ;;
  arm64) target_dir="aarch64-unknown-linux-gnu/"; stable_arch="aarch64" ;;
  *) echo "unknown arch '$arch'" >&2; exit 1 ;;
esac

bundle="apps/desktop/src-tauri/target/${target_dir}release/bundle"
mkdir -p dist fragments

collect() { # <bundle-subdirectory> <stable-destination-name>
  local dir="$1" dest="$2"
  shopt -s nullglob
  local matches=("$bundle/$dir/"*)
  [ "${#matches[@]}" -eq 1 ] || {
    echo "expected exactly one file in $bundle/$dir, found ${#matches[@]}" >&2
    exit 1
  }
  cp "${matches[0]}" "dist/$dest"
  echo "collected dist/$dest"
}

collect appimage "Cuotai_${VERSION}_${stable_arch}.AppImage"
collect deb "Cuotai_${VERSION}_${stable_arch}.deb"
collect rpm "Cuotai_${VERSION}_${stable_arch}.rpm"

jq -n \
  --arg a "Cuotai_${VERSION}_${stable_arch}.AppImage" \
  --arg d "Cuotai_${VERSION}_${stable_arch}.deb" \
  --arg r "Cuotai_${VERSION}_${stable_arch}.rpm" \
  '
  {
    artifacts: [$a, $d, $r] | map({
      name: .,
      signed: false,
      method: "checksum-signature",
      status: "unsigned",
      note: "unsigned payload; integrity is authenticated through SHA256SUMS when its signature is available"
    })
  }
  ' > "fragments/linux-${arch}.json"

echo "wrote fragments/linux-${arch}.json"
