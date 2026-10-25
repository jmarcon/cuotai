#!/bin/sh
# PRD 0022 RF18.2: regenerate the real-UI screenshots from the browser demo (see README.md).
# Usage: sh apps/desktop/tools/ui-shots/capture.sh [--out DIR] [name-prefix ...]
set -e
HERE=$(command cd "$(dirname "$0")" && pwd)
exec node "$HERE/capture.mjs" "$@"
