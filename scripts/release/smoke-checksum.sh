#!/usr/bin/env bash
# Proves that the files a smoke job is about to exercise are the exact bytes
# the assembly listed in SHA256SUMS (PRD 0021 RF6): every named file must have
# exactly one `<sha256>  <name>` line and its digest must match. A file the
# assembly does not list fails, so a smoke never passes on a stray build
# output that would not be published.
# Usage: smoke-checksum.sh <SHA256SUMS> <file>...
set -euo pipefail

sums="${1:?SHA256SUMS path}"
shift
[ "$#" -gt 0 ] || { echo "no file to verify" >&2; exit 1; }
[ -f "$sums" ] || { echo "missing $sums" >&2; exit 1; }

digest() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -b "$1" | cut -d' ' -f1
  else
    shasum -a 256 -b "$1" | cut -d' ' -f1
  fi
}

for file in "$@"; do
  [ -f "$file" ] && [ ! -L "$file" ] || {
    echo "smoke input is not a regular file: $file" >&2
    exit 1
  }
  name="$(basename "$file")"
  # Exact two-space format written by `xtask release-manifest`; awk compares
  # the name as a string, never as a pattern.
  expected="$(awk -v n="$name" 'length($0) > 66 && substr($0, 65, 2) == "  " && substr($0, 67) == n { print substr($0, 1, 64) }' "$sums")"
  count="$(printf '%s' "$expected" | grep -c . || true)"
  if [ "$count" -ne 1 ]; then
    echo "$name: expected one SHA256SUMS entry, found $count" >&2
    exit 1
  fi
  actual="$(digest "$file" | tr -d '\r')"
  if [ "$actual" != "$expected" ]; then
    echo "$name: sha256 $actual differs from SHA256SUMS ($expected)" >&2
    exit 1
  fi
  echo "checksum ok: $name"
done
