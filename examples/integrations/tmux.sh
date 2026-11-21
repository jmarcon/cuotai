#!/bin/sh
# tmux status segment (PRD 0031 RF1): in tmux.conf,
#   set -g status-right '#(/path/to/tmux.sh)'
# See docs/integrations/tmux.md.
. "$(dirname "$0")/common.sh"

if ! cuotai_read; then
  printf '#[fg=colour244]Cuotai offline#[default]\n'
  exit 0
fi
case $cuotai_severity in
  critical | outage) colour=red ;;
  warning) colour=yellow ;;
  *) colour=green ;;
esac
# tmux reads `#` in the status line as a format (`#[fg=…]`, `#(command)`):
# double it, so a label from an aggregator connection cannot restyle the bar.
label=$(cuotai_label | sed 's/#/##/g')
printf '#[fg=%s]%s#[default]\n' "$colour" "$label"
