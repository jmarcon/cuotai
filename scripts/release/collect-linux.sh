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

# Select exactly one final regular non-symlink package by expected extension
# in `$bundle/$dir`. Tauri's linuxdeploy AppImage tooling leaves a sibling
# `Cuotai.AppDir/` (and a `*.swp` or `.tmp-*` work area in some deb flows)
# beside the final package, so globbing every entry picks the staging tree
# too and the script reports "found N != 1" and exits mid-collection.
select_final_package() { # <bundle-subdirectory> <expected-extension>
  local dir="$1" expected_ext="$2"
  shopt -s nullglob
  local candidates=()
  local entry
  for entry in "$bundle/$dir/"*; do
    case "$entry" in
      *."$expected_ext")
        if [ -L "$entry" ] || [ ! -f "$entry" ]; then
          echo "$entry is not a regular, non-symlink file" >&2
          exit 1
        fi
        # Reject paths that escape `$bundle/$dir` (defence against symlink
        # racing) and absolute paths: every candidate must resolve there.
        # Both sides go through `readlink -f` because on macOS `/tmp` is a
        # symlink and `pwd` returns the un-realpathed form, so the simple
        # text compare would otherwise flag legitimate files.
        local target_dir_abs expected_prefix
        target_dir_abs="$(cd "$bundle/$dir" && pwd -P)"
        expected_prefix="${target_dir_abs}/"
        case "$(readlink -f -- "$entry")/" in
          "$expected_prefix"*) ;;
          *) echo "$entry resolves outside $bundle/$dir" >&2; exit 1 ;;
        esac
        candidates+=("$entry")
        ;;
    esac
  done
  case "${#candidates[@]}" in
    0)
      echo "expected one $expected_ext package in $bundle/$dir, found 0" >&2
      exit 1
      ;;
    1) printf '%s\n' "${candidates[0]}" ;;
    *)
      printf '%s\n' "${candidates[@]}" >&2
      echo "expected one $expected_ext package in $bundle/$dir, found ${#candidates[@]}" >&2
      exit 1
      ;;
  esac
}

collect() { # <bundle-subdirectory> <expected-extension> <stable-destination-name>
  local dir="$1" expected_ext="$2" dest="$3"
  local src
  src="$(select_final_package "$dir" "$expected_ext")"
  cp "$src" "dist/$dest"
  echo "collected dist/$dest"
  # Updater signature (PRD 0018 RF1.1): with
  # `--config src-tauri/tauri.updater.conf.json` the Tauri bundler signs every
  # Linux package in place while bundling, and nothing rewrites those bytes
  # afterwards, so the signature is final here. Without the overlay there is
  # no `.sig` beside the package, which is the honest unsigned state.
  #
  # The package is ONE artifact with TWO proofs: the checksum record every
  # Linux package has (RF3.1) and, when the bundler signed it, the detached
  # updater signature. Recording the payload a second time as a
  # `tauri-updater-key` entry is what `signing::merge` rejects, so it is only
  # ever attached to the record the payload already has.
  if [ -f "${src}.sig" ]; then
    cp "${src}.sig" "dist/${dest}.sig"
    echo "collected dist/${dest}.sig"
    updater_entries="$(jq -c --arg a "$dest" --arg s "${dest}.sig" '
      . + [$a, $s]
    ' <<<"$updater_entries")"
  fi
}

appimage="Cuotai_${VERSION}_${stable_arch}.AppImage"
deb="Cuotai_${VERSION}_${stable_arch}.deb"
rpm="Cuotai_${VERSION}_${stable_arch}.rpm"
updater_entries='[]'
collect appimage "AppImage" "$appimage"
collect deb     "deb"     "$deb"
collect rpm     "rpm"     "$rpm"

# The updater state follows the signatures: available with exactly the signed
# payloads and their detached signatures, or unavailable with the reason the
# key was not configured. Every package has exactly one record, and the ones
# the bundler signed carry the detached signature as their updater proof.
jq -n \
  --arg a "$appimage" \
  --arg d "$deb" \
  --arg r "$rpm" \
  --arg reason "complete Tauri updater signing credentials are not configured in the release environment" \
  --argjson updater "$updater_entries" \
  '
  {
    artifacts: ([$a, $d, $r] | map(. as $name | {
      name: $name,
      signed: false,
      method: "checksum-signature",
      status: "unsigned",
      note: "unsigned payload; integrity is authenticated through SHA256SUMS when its signature is available"
    } + if ($updater | index($name)) then {updater_signature: "\($name).sig"} else {} end))
  }
  | .updater = (if ($updater | length) == 0
      then {available: false, artifacts: [], reason: $reason}
      else {available: true, artifacts: $updater}
      end)
  ' > "fragments/linux-${arch}.json"

echo "wrote fragments/linux-${arch}.json"
