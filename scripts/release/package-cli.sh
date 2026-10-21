#!/usr/bin/env bash
# Packages the built `cuotai` CLI binary into the stable release archive
# (PRD 0021 RF2): cuotai-<version>-<target>.tar.gz with the binary and the
# licenses. Usage: package-cli.sh <target> <version> <dist-dir>
set -euo pipefail

target="${1:?target triple}"
version="${2:?version}"
dist="${3:?dist directory}"
bin="target/${target}/release/cuotai"

[ -f "$bin" ] || { echo "missing built binary: $bin" >&2; exit 1; }

staging="$(mktemp -d)"
trap 'rm -rf "$staging"' EXIT
cp "$bin" "$staging/cuotai"
cp LICENSE-APACHE LICENSE-MIT "$staging/"

mkdir -p "$dist"
name="cuotai-${version}-${target}.tar.gz"
# GNU tar normalizes ordering and ownership; bsdtar on macOS gets an explicit
# file order. Checksums authenticate the exact archive produced by this run.
if tar --version 2>/dev/null | grep -q GNU; then
  tar -czf "${dist}/${name}" \
    --sort=name --owner=0 --group=0 --numeric-owner \
    -C "$staging" cuotai LICENSE-APACHE LICENSE-MIT
else
  COPYFILE_DISABLE=1 tar -czf "${dist}/${name}" \
    -C "$staging" cuotai LICENSE-APACHE LICENSE-MIT
fi
