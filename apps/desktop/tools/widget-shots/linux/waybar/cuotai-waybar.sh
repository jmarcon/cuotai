#!/bin/sh
# Example Waybar module for Cuotai's widget snapshot (PRD 0014 RF5.1).
#
# Reads `widget.json` and prints one line of Waybar's JSON protocol
# (`text`, `tooltip`, `class`, `percentage`). It only reads the file the
# desktop app already wrote: no provider query, no credential. Needs `jq`.
#
#   "custom/cuotai": {
#       "exec": "~/.config/waybar/cuotai-waybar.sh",
#       "return-type": "json",
#       "interval": 30
#   }
#
# Usage: cuotai-waybar.sh [path/to/widget.json]
set -eu

file=${1:-"${XDG_DATA_HOME:-$HOME/.local/share}/cuotai/widget.json"}

if [ ! -r "$file" ]; then
    # The feature is off (or the app never ran): say so, never print zeros.
    printf '%s\n' '{"text":"Cuotai: off","tooltip":"Turn on Settings → Widgets in Cuotai","class":"off"}'
    exit 0
fi

exec jq -c '
  def pct: (. * 100 | round | tostring) + "%";
  # The core writes RFC 3339 with sub-second precision; jq parses whole seconds.
  def epoch: sub("\\.[0-9]+"; "") | fromdateiso8601;
  def age:
    if . == null then "no reading yet"
    else (now - epoch) as $s
      | if $s < 3600 then "\($s / 60 | floor) min ago"
        elif $s < 172800 then "\($s / 3600 | floor) h ago"
        else "\($s / 86400 | floor) d ago" end
    end;
  def short:
    if .state == "unavailable" or .binding == null then "\(.name) unavailable"
    else "\(.name) \(.binding.used_ratio | pct)" + (if .state == "stale" then " (stale)" else "" end)
    end;
  def line:
    if .state == "unavailable" or .binding == null then "\(.name) (\(.provider)): unavailable"
    else "\(.name) (\(.provider)): \(.binding.used_ratio | pct) \(.binding.label), \(.checked_at | age)"
      + (if .state == "stale" then ", stale" else "" end)
    end;
  (.accounts[0].binding.used_ratio // null) as $top
  | ([.accounts[:3][] | short] | join(" · ")) as $text
  | {
      text: (if $text == "" then "Cuotai: no accounts" else $text end),
      tooltip: (([.accounts[] | line] | join("\n"))
        + (if .truncated then "\n(more accounts not published)" else "" end)),
      class: (if $top == null then "unavailable"
              elif $top >= 1 then "exhausted"
              elif $top >= 0.9 then "critical"
              elif $top >= 0.75 then "warning"
              else "normal" end),
      percentage: (if $top == null then 0 else ($top * 100 | round) end)
    }
' "$file"
