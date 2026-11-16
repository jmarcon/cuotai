#!/bin/sh
# Waybar custom module (PRD 0031 RF1): prints the JSON Waybar expects with
# "return-type": "json". See docs/integrations/waybar.md.
. "$(dirname "$0")/common.sh"

if ! cuotai_read; then
  printf '{"text":"Cuotai offline","tooltip":"cuotai --serve is not reachable","class":"offline"}\n'
  exit 0
fi
label=$(cuotai_label)
# Waybar reads `text` and `tooltip` as Pango markup: escape the name, so an
# account called "<b>x</b>" or "A & B" shows as typed (leave "escape" off).
jq -nc --arg text "Cuotai $label" --arg class "$cuotai_severity" --arg reset "$cuotai_reset" \
  'def markup: gsub("&"; "&amp;") | gsub("<"; "&lt;") | gsub(">"; "&gt;");
   {text: ($text | markup), class: $class, tooltip: (if $reset == "" then "No reset time known" else "Resets at " + $reset end)}'
