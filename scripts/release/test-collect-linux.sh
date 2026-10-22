#!/usr/bin/env bash
# Behavior test for scripts/release/collect-linux.sh (PRD 0021 RF2.5).
# Build a synthetic `bundle/` directory under a temporary root, exports a
# trivial `VERSION`, runs the collector, and asserts on:
#   * the green path: one final package per extension plus Tauri's
#     linuxdeploy `Cuotai.AppDir` staging directory and an unrelated
#     extension sibling collect the right files into `dist/` with the
#     exact stable names;
#   * zero matching files fail closed;
#   * two final same-extension matches fail closed and list the offenders
#     (a separate case from any staging directory);
#   * a symlink whose own name matches the expected extension is rejected
#     instead of being copied.
# No language or tool dependency beyond POSIX `bash` and the GNU coreutils
# `readlink` used by the collector itself; invoked directly as
#   bash scripts/release/test-collect-linux.sh
# The script exits 0 on success, non-zero on any failed assertion.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COLLECT="${SCRIPT_DIR}/collect-linux.sh"

fail() { echo "FAIL: $*" >&2; exit 1; }
note() { echo "==> $*"; }

setup_workdir() {
  local dest="$1"
  mkdir -p "$dest/dist"
  mkdir -p "$dest/apps/desktop/src-tauri/target/release/bundle"
}

write_pkg() {
  local path="$1" name="$2"
  mkdir -p "$(dirname "$path")"
  printf 'fixture payload for %s\n' "$name" > "$path"
}

green_scenario() {
  local root="$1"
  setup_workdir "$root"
  # Final artefacts (one per extension) plus Tauri's linuxdeploy AppDir
  # staging directory (`Cuotai.AppDir` is the canonical AppImage staging
  # shape) and an unrelated sibling extension to make sure the collector
  # does not blindly take "the first entry".
  local v=0.67.0
  write_pkg "$root/apps/desktop/src-tauri/target/release/bundle/appimage/Cuotai_${v}_amd64.AppImage" "amd64 AppImage"
  mkdir -p "$root/apps/desktop/src-tauri/target/release/bundle/appimage/Cuotai.AppDir/usr/bin"
  echo "linuxdeploy staging payload" > "$root/apps/desktop/src-tauri/target/release/bundle/appimage/Cuotai.AppDir/usr/bin/cuotai"
  write_pkg "$root/apps/desktop/src-tauri/target/release/bundle/appimage/Cuotai_${v}_amd64.ignore" "unrelated"

  write_pkg "$root/apps/desktop/src-tauri/target/release/bundle/deb/Cuotai_${v}_amd64.deb" "amd64 deb"
  mkdir -p "$root/apps/desktop/src-tauri/target/release/bundle/deb/Cuotai.AppDir/DEBIAN"
  write_pkg "$root/apps/desktop/src-tauri/target/release/bundle/deb/Cuotai_${v}_amd64.deb.swp" "vim swap"

  write_pkg "$root/apps/desktop/src-tauri/target/release/bundle/rpm/Cuotai_${v}_amd64.rpm" "amd64 rpm"
  write_pkg "$root/apps/desktop/src-tauri/target/release/bundle/rpm/Cuotai_${v}_amd64.txt" "notes"

  (
    cd "$root"
    export VERSION="$v"
    "$COLLECT" x64
  )

  local expected_pkg base
  for base in "Cuotai_${v}_amd64.AppImage" "Cuotai_${v}_amd64.deb" "Cuotai_${v}_amd64.rpm"; do
    expected_pkg="$root/dist/$base"
    [ -s "$expected_pkg" ] || fail "expected $expected_pkg to be collected"
    cmp -s "$expected_pkg" "$(find "$root" -path "*/bundle/*/$base" -not -type l)" \
      || fail "$expected_pkg contents differ from the bundle source"
  done

  local frag="$root/fragments/linux-x64.json"
  [ -s "$frag" ] || fail "missing fragments/linux-x64.json"
  grep -q "\"name\": *\"Cuotai_${v}_amd64.AppImage\"" "$frag" || fail "AppImage not in fragment"
  grep -q "\"name\": *\"Cuotai_${v}_amd64.deb\"" "$frag" || fail "deb not in fragment"
  grep -q "\"name\": *\"Cuotai_${v}_amd64.rpm\"" "$frag" || fail "rpm not in fragment"
}

zero_scenario() {
  local root="$1"
  setup_workdir "$root"
  # Bundle directory exists but only contains a linuxdeploy AppDir staging
  # tree (which the extension glob correctly ignores because the name has
  # no `.AppImage` suffix) plus non-matching siblings.
  mkdir -p "$root/apps/desktop/src-tauri/target/release/bundle/appimage/Cuotai.AppDir/usr/bin"
  write_pkg "$root/apps/desktop/src-tauri/target/release/bundle/appimage/Cuotai.AppDir/usr/bin/cuotai" "linuxdeploy staging payload"
  write_pkg "$root/apps/desktop/src-tauri/target/release/bundle/deb/note.txt" "unrelated"
  write_pkg "$root/apps/desktop/src-tauri/target/release/bundle/rpm/note.txt" "unrelated"

  if (
      cd "$root"
      export VERSION=0.67.0
      "$COLLECT" x64
    ) >/dev/null 2>"$root/.err"; then
    fail "collector accepted zero matching AppImage files"
  fi
  grep -q "expected one AppImage package" "$root/.err" || fail "zero-match error did not name the bundle dir"
}

duplicate_scenario() {
  local root="$1"
  setup_workdir "$root"
  # Two AppImage final artefacts. This is a separate fail-closed case from a
  # staging directory: every entry matches the expected extension and the
  # collector cannot choose one.
  write_pkg "$root/apps/desktop/src-tauri/target/release/bundle/appimage/Cuotai_0.67.0_amd64.AppImage" "first"
  write_pkg "$root/apps/desktop/src-tauri/target/release/bundle/appimage/Cuotai_0.67.0_amd64-amd64.AppImage" "second"
  write_pkg "$root/apps/desktop/src-tauri/target/release/bundle/deb/Cuotai_0.67.0_amd64.deb" "deb"
  write_pkg "$root/apps/desktop/src-tauri/target/release/bundle/rpm/Cuotai_0.67.0_amd64.rpm" "rpm"

  if (
      cd "$root"
      export VERSION=0.67.0
      "$COLLECT" x64
    ) >/dev/null 2>"$root/.err"; then
    fail "collector accepted multiple AppImage files"
  fi
  grep -q "found 2" "$root/.err" || fail "duplicate-match error did not report the count"
}

symlink_scenario() {
  local root="$1"
  setup_workdir "$root"
  # Another `.AppImage` whose own name ends in the same extension, but it
  # is a symlink. The resolver inside the collector must reject the symlink
  # explicitly so it cannot be a vector for copying arbitrary files. The
  # other bundles produce their real packages so the failure is scoped to
  # the AppImage step.
  write_pkg "$root/apps/desktop/src-tauri/target/release/bundle/appimage/Cuotai_0.67.0_amd64.AppImage" "real"
  printf 'tainted payload' > "$root/apps/desktop/src-tauri/target/release/bundle/appimage/aliased.bin"
  # Name has the literal `.AppImage` suffix so the `*.AppImage` glob sees
  # it; without the symlink guard the collector would treat it as a
  # duplicate and reject with `found 2`.
  ln -s aliased.bin "$root/apps/desktop/src-tauri/target/release/bundle/appimage/aliased.AppImage"
  write_pkg "$root/apps/desktop/src-tauri/target/release/bundle/deb/Cuotai_0.67.0_amd64.deb" "deb"
  write_pkg "$root/apps/desktop/src-tauri/target/release/bundle/rpm/Cuotai_0.67.0_amd64.rpm" "rpm"

  if (
      cd "$root"
      export VERSION=0.67.0
      "$COLLECT" x64
    ) >/dev/null 2>"$root/.err"; then
    fail "collector accepted a symlinked AppImage"
  fi
  grep -q "not a regular, non-symlink file" "$root/.err" \
    || fail "symlink error did not name the kind of failure"
}

updater_signed_scenario() {
  local root="$1"
  setup_workdir "$root"
  local v=0.67.0
  local bundle="$root/apps/desktop/src-tauri/target/release/bundle"
  write_pkg "$bundle/appimage/Cuotai_${v}_amd64.AppImage" "amd64 AppImage"
  write_pkg "$bundle/appimage/Cuotai_${v}_amd64.AppImage.sig" "base64 updater signature"
  write_pkg "$bundle/deb/Cuotai_${v}_amd64.deb" "amd64 deb"
  write_pkg "$bundle/deb/Cuotai_${v}_amd64.deb.sig" "base64 updater signature"
  write_pkg "$bundle/rpm/Cuotai_${v}_amd64.rpm" "amd64 rpm"

  (
    cd "$root"
    export VERSION="$v"
    "$COLLECT" x64
  ) >/dev/null

  # Every signature the bundler produced is collected next to its payload,
  # and the fragment reports the updater as available with exactly those
  # files (PRD 0018 RF1.1).
  for base in "Cuotai_${v}_amd64.AppImage" "Cuotai_${v}_amd64.deb"; do
    [ -s "$root/dist/${base}.sig" ] || fail "expected dist/${base}.sig to be collected"
    cmp -s "$root/dist/${base}.sig" "$bundle/$(dirname "${base#Cuotai_${v}_}" | sed 's#^#x/#')${base}" 2>/dev/null || true
  done
  cmp -s "$root/dist/Cuotai_${v}_amd64.AppImage.sig" \
    "$bundle/appimage/Cuotai_${v}_amd64.AppImage.sig" \
    || fail "collected AppImage signature differs from the bundle source"
  cmp -s "$root/dist/Cuotai_${v}_amd64.deb.sig" \
    "$bundle/deb/Cuotai_${v}_amd64.deb.sig" \
    || fail "collected deb signature differs from the bundle source"
  [ -e "$root/dist/Cuotai_${v}_amd64.rpm.sig" ] \
    && fail "an unsigned rpm must not produce a signature in dist/"

  local frag="$root/fragments/linux-x64.json"
  grep -q "\"available\": *true" "$frag" || fail "fragment does not report the updater as available"
  local want
  want="$(printf '%s\n' \
      "Cuotai_${v}_amd64.AppImage" "Cuotai_${v}_amd64.AppImage.sig" \
      "Cuotai_${v}_amd64.deb" "Cuotai_${v}_amd64.deb.sig" | sort | jq -R -s -c 'split("\n")[:-1]')"
  jq -e --argjson want "$want" '.updater.artifacts | sort == $want' "$frag" >/dev/null \
    || fail "updater artifacts in the fragment are not the signed pairs"
  jq -e '[.artifacts[] | select(.method == "tauri-updater-key")] | length == 4' "$frag" >/dev/null \
    || fail "fragment does not carry the updater-key signing entries"
}

updater_unsigned_scenario() {
  local root="$1"
  setup_workdir "$root"
  local v=0.67.0
  local bundle="$root/apps/desktop/src-tauri/target/release/bundle"
  write_pkg "$bundle/appimage/Cuotai_${v}_amd64.AppImage" "amd64 AppImage"
  write_pkg "$bundle/deb/Cuotai_${v}_amd64.deb" "amd64 deb"
  write_pkg "$bundle/rpm/Cuotai_${v}_amd64.rpm" "amd64 rpm"

  (
    cd "$root"
    export VERSION="$v"
    "$COLLECT" x64
  ) >/dev/null

  # No key, no signature: the packages keep their checksum-signature trust
  # and the fragment states why the updater produced nothing.
  local frag="$root/fragments/linux-x64.json"
  grep -q "\"available\": *false" "$frag" || fail "fragment does not report the updater as unavailable"
  jq -e '.updater.reason | length > 0' "$frag" >/dev/null \
    || fail "an unavailable updater must give a reason"
  jq -e '[.artifacts[] | select(.method == "tauri-updater-key")] | length == 0' "$frag" >/dev/null \
    || fail "an unsigned run must not report updater-key signing entries"
}

main() {
  bash -n "$COLLECT"

  local green_dir zero_dir dup_dir sym_dir signed_dir unsigned_dir
  green_dir="$(mktemp -d)"
  zero_dir="$(mktemp -d)"
  dup_dir="$(mktemp -d)"
  sym_dir="$(mktemp -d)"
  signed_dir="$(mktemp -d)"
  unsigned_dir="$(mktemp -d)"
  trap 'rm -rf "${green_dir:-}" "${zero_dir:-}" "${dup_dir:-}" "${sym_dir:-}" "${signed_dir:-}" "${unsigned_dir:-}"' EXIT

  note "green: linuxdeploy AppDir staging tree + unrelated sibling collect the right packages"
  green_scenario "$green_dir"
  note "red: only a linuxdeploy AppDir staging tree (no final .AppImage) must fail closed"
  zero_scenario "$zero_dir"
  note "red: two final same-extension matches must fail closed (separate case from any staging tree)"
  duplicate_scenario "$dup_dir"
  note "red: a symlink whose own name matches the expected extension must be rejected"
  symlink_scenario "$sym_dir"
  note "green: bundler-produced updater signatures are collected and reported"
  updater_signed_scenario "$signed_dir"
  note "green: a run without the updater key stays honestly unsigned"
  updater_unsigned_scenario "$unsigned_dir"

  echo "PASS: collect-linux.sh green/red fixtures behave correctly"
}

main "$@"
