#!/bin/sh
# SketchyBar plugin (PRD 0031 RF1): set `script` of an item to this file.
# SketchyBar passes the item's name in $NAME. See docs/integrations/sketchybar.md.
. "$(dirname "$0")/common.sh"

if ! cuotai_read; then
  "${SKETCHYBAR:-sketchybar}" --set "${NAME:-cuotai}" label="offline" label.color=0xff888888
  exit 0
fi
case $cuotai_severity in
  critical | outage) color=0xffff5555 ;;
  warning) color=0xffffb86c ;;
  *) color=0xff50fa7b ;;
esac
"${SKETCHYBAR:-sketchybar}" --set "${NAME:-cuotai}" label="$(cuotai_label)" label.color="$color"
