#!/bin/sh
# tmux status segment (PRD 0031 RF1): in tmux.conf,
#   set -g status-right '#(/path/to/tmux.sh)'
# See docs/integrations/tmux.md.
. "$(dirname "$0")/common.sh"

if ! cuotai_read; then
  printf '#[fg=colour244]cuotai offline#[default]\n'
  exit 0
fi
case $cuotai_severity in
  critical | outage) colour=red ;;
  warning) colour=yellow ;;
  *) colour=green ;;
esac
printf '#[fg=%s]%s#[default]\n' "$colour" "$(cuotai_label)"
