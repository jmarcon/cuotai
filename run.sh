#!/bin/sh
# Development runner: the repository's cuotai.toml and a local SQLite credential store
# that keeps tokens in PLAIN TEXT. Debug builds only; release builds refuse that store.
set -eu

ROOT="$(cd "$(dirname "$0")" && pwd)"
CONFIG="${CUOTAI_CONFIG:-$ROOT/cuotai.toml}"
SQLITE="${CUOTAI_DEV_CREDENTIALS_SQLITE:-$ROOT/cuotai-dev.sqlite3}"

usage() {
    cat <<'EOF'
Usage: sh run.sh [tui|gui] [args...]

  tui   terminal dashboard (default; arguments starting with '-' are passed to tui)
  gui   desktop app (apps/desktop/src-tauri)

Environment overrides (defaults in the repository root):
  CUOTAI_CONFIG                  config file (default: cuotai.toml)
  CUOTAI_DEV_CREDENTIALS_SQLITE  SQLite store (default: cuotai-dev.sqlite3)

Warning: that store keeps tokens in plain text. Use it for development only.
EOF
}

mode=tui
case "${1:-}" in
    -h | --help)
        usage
        exit 0
        ;;
    tui | gui)
        mode="$1"
        shift
        ;;
    -*) ;;
    "") ;;
    *)
        echo "run.sh: unknown mode '$1' (expected tui or gui)" >&2
        usage >&2
        exit 2
        ;;
esac

if [ ! -f "$CONFIG" ]; then
    echo "run.sh: warning: config file not found: $CONFIG (starting without accounts)" >&2
fi

case "$mode" in
    tui)
        # CUOTAI_CONFIG rather than --config, so a --config argument still wins.
        cd "$ROOT"
        CUOTAI_CONFIG="$CONFIG" exec cargo run -p cuotai-tui -- --dev-credentials-sqlite "$SQLITE" "$@"
        ;;
    gui)
        cd "$ROOT/apps/desktop/src-tauri"
        CUOTAI_CONFIG="$CONFIG" CUOTAI_DEV_CREDENTIALS_SQLITE="$SQLITE" exec cargo run "$@"
        ;;
esac
