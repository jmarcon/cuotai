#!/bin/sh
# Renders the mockups in this directory to ../*.png at 2x (PRD 0022 "AFTER" set).
# Usage: sh render.sh [name-prefix ...]    (no argument: every screen listed in screens.tsv)
# Needs: node 22+, python3, Google Chrome. The repo root is served on 127.0.0.1 because the pages
# load theme CSS, provider glyph masks and the logo from apps/desktop/ui (masks need http, not file://).
set -e
HERE=$(command cd "$(dirname "$0")" && pwd)
ROOT=$(command cd "$HERE/../../../../../.." && pwd)
OUT=$(command cd "$HERE/.." && pwd)
CHROME="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
PORT="${MOCKUP_PORT:-8861}"
node "$HERE/build.mjs" >/dev/null
python3 -m http.server "$PORT" --bind 127.0.0.1 --directory "$ROOT" >/dev/null 2>&1 &
SERVER=$!
trap 'kill $SERVER 2>/dev/null' EXIT INT TERM
sleep 1
PROFILE=$(mktemp -d)
REL="docs/plans/assets/0022/after/src"
while IFS="$(printf '\t')" read -r name w h; do
  if [ $# -gt 0 ]; then ok=0; for p in "$@"; do case "$name" in "$p"*) ok=1;; esac; done; [ $ok = 1 ] || continue; fi
  "$CHROME" --headless=new --disable-gpu --hide-scrollbars --lang=en-US --user-data-dir="$PROFILE" \
    --force-device-scale-factor=2 --window-size="$w,$h" --virtual-time-budget=2500 \
    --screenshot="$OUT/$name.png" "http://127.0.0.1:$PORT/$REL/$name.html" >/dev/null 2>&1
  echo "$name.png ${w}x${h}"
done < "$HERE/screens.tsv"
rm -rf "$PROFILE"
