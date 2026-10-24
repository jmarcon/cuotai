#!/bin/sh
# Desktop development build: same as `sh run.sh gui` (see run.sh for the plaintext-token warning).
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
exec sh "$ROOT/run.sh" gui "$@"
