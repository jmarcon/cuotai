#!/bin/sh
# i3bar / sway block (PRD 0031 RF1): prints one i3bar-protocol block object
# for a wrapper that merges it into the i3status array, or for
# `status_command` scripts of your own. See docs/integrations/i3status.md.
. "$(dirname "$0")/common.sh"

if ! cuotai_read; then
  printf '{"name":"cuotai","full_text":"cuotai offline","color":"#888888"}\n'
  exit 0
fi
case $cuotai_severity in
  critical | outage) color='#ff5555' ;;
  warning) color='#ffb86c' ;;
  *) color='#50fa7b' ;;
esac
jq -nc --arg text "Cuotai $(cuotai_label)" --arg color "$color" \
  '{name: "cuotai", full_text: $text, color: $color}'
