#!/bin/sh
# Waybar custom module (PRD 0031 RF1): prints the JSON Waybar expects with
# "return-type": "json". See docs/integrations/waybar.md.
. "$(dirname "$0")/common.sh"

if ! cuotai_read; then
  printf '{"text":"Cuotai offline","tooltip":"cuotai --serve is not reachable","class":"offline"}\n'
  exit 0
fi
label=$(cuotai_label)
jq -nc --arg text "Cuotai $label" --arg class "$cuotai_severity" --arg reset "$cuotai_reset" \
  '{text: $text, class: $class, tooltip: (if $reset == "" then "No reset time known" else "Resets at " + $reset end)}'
