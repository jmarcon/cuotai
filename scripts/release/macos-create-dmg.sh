#!/usr/bin/env bash
# Creates the compressed (UDZO) DMG for an app bundle, with volume name Cuotai.
# Usage: macos-create-dmg.sh <path/to/Cuotai.app> <path/to/output.dmg>
#
# `hdiutil create -srcfolder` sizes its temporary image on its own, and that
# estimate is sometimes too small for the universal app ("No space left on
# device" while copying the payload). The size is therefore computed from the
# payload with generous headroom, and one failed attempt is retried with twice
# that size after a short delay.
set -euo pipefail

[ "$#" -eq 2 ] || { echo "usage: $0 <app> <dmg>" >&2; exit 2; }
app="$1"
dmg="$2"
[ -d "$app" ] || { echo "missing $app" >&2; exit 1; }

payload_kb="$(du -sk "$app" | awk '{print $1}')"
# +50% for filesystem overhead, and never less than 64 MiB of headroom.
size_kb=$(( payload_kb * 3 / 2 ))
min_kb=$(( payload_kb + 65536 ))
[ "$size_kb" -ge "$min_kb" ] || size_kb="$min_kb"

create() {
  rm -f "$dmg"
  hdiutil create -volname Cuotai -srcfolder "$app" -size "${1}k" -ov -format UDZO "$dmg"
}

echo "creating $dmg from a ${payload_kb} KiB payload with a ${size_kb} KiB image"
if ! create "$size_kb"; then
  size_kb=$(( size_kb * 2 ))
  echo "hdiutil create failed; retrying in 10 s with a ${size_kb} KiB image" >&2
  sleep 10
  create "$size_kb"
fi
hdiutil verify "$dmg" -quiet
