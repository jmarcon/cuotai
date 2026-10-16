#!/bin/sh
# Desenvolvimento: usa o cuotai.toml da raiz e o SQLite local (texto puro) no lugar do Keychain,
# o mesmo banco de run.sh. Nunca use em release; builds release recusam esse armazenamento.
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT/apps/desktop/src-tauri" || exit 1
CUOTAI_CONFIG="${CUOTAI_CONFIG:-$ROOT/cuotai.toml}" \
CUOTAI_DEV_CREDENTIALS_SQLITE="${CUOTAI_DEV_CREDENTIALS_SQLITE:-$ROOT/cuotai-dev.sqlite3}" \
exec cargo run "$@"
