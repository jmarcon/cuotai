#!/usr/bin/env bash
# Smoke test of one packaged CLI archive (PRD 0021 RF6.1): unpack exactly the
# archive users download, check its layout, then run `--version`,
# `--list-providers`, `--once` and `--json` against a sample configuration
# with no network.
#
# "No network" is enforced by the operating system wherever a runner allows
# it and proven by the result everywhere:
#   * Linux: the binary runs in a pinned minimal container started with
#     `--network none` (SMOKE_IMAGE). The image has none of the build host's
#     libraries, so a binary missing a shared library fails here.
#   * macOS: the binary runs under `sandbox-exec` with every network
#     operation denied.
#   * Windows: no unprivileged per-process network block exists on the
#     runner, so only the proxy sink and the classification proof apply.
# On every system the proxy variables point at a closed local port, and the
# `--json` report must classify every account as `needs_configuration`: the
# sample configuration names credential variables that are never defined, so
# each account is rejected before any HTTP request is built.
#
# Usage: smoke-cli.sh <archive> <expected-version> <config.toml>
# Optional environment: SMOKE_IMAGE (Linux container image, pinned by digest).
set -euo pipefail

archive="${1:?CLI archive (.tar.gz or .zip)}"
version="${2:?expected version}"
config="${3:?sample configuration}"
[ -f "$archive" ] || { echo "missing archive $archive" >&2; exit 1; }
[ -f "$config" ] || { echo "missing configuration $config" >&2; exit 1; }

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
pkg="$work/pkg"
mkdir -p "$pkg" "$work/cfg" "$work/home"
cp "$config" "$work/cfg/smoke.toml"

native_path() { # Windows tools need Windows paths under Git Bash.
  if command -v cygpath >/dev/null 2>&1; then cygpath -w "$1"; else printf '%s\n' "$1"; fi
}

case "$archive" in
  *.tar.gz) tar -xzf "$archive" -C "$pkg" ;;
  *.zip)
    if command -v unzip >/dev/null 2>&1; then
      unzip -q "$archive" -d "$pkg"
    else
      # shellcheck disable=SC2016 # PowerShell expands its own $env: variables.
      SMOKE_ZIP="$(native_path "$archive")" SMOKE_DEST="$(native_path "$pkg")" \
        pwsh -NoProfile -NonInteractive -Command \
        'Expand-Archive -LiteralPath $env:SMOKE_ZIP -DestinationPath $env:SMOKE_DEST'
    fi
    ;;
  *) echo "unknown archive type: $archive" >&2; exit 1 ;;
esac

# Layout: the binary and both licenses, nothing else, no nested directory.
exe=cuotai
case "$archive" in *.zip) exe=cuotai.exe ;; esac
layout="$(cd "$pkg" && find . -mindepth 1 | LC_ALL=C sort | tr '\n' ' ')"
expected_layout="$(printf './%s\n' LICENSE-APACHE LICENSE-MIT "$exe" | LC_ALL=C sort | tr '\n' ' ')"
if [ "$layout" != "$expected_layout" ]; then
  echo "archive layout is '$layout', expected '$expected_layout'" >&2
  exit 1
fi
bin="$pkg/$exe"
[ -f "$bin" ] && [ ! -L "$bin" ] || { echo "$exe is not a regular file" >&2; exit 1; }
case "$exe" in
  cuotai) [ -x "$bin" ] || { echo "$exe is not executable in the archive" >&2; exit 1; } ;;
esac

# A closed local port: any request that slipped past the classification
# would fail instead of leaving the runner.
sink="http://127.0.0.1:9"

if [ -n "${SMOKE_IMAGE:-}" ]; then
  isolation="container $SMOKE_IMAGE with --network none"
  docker pull --quiet "$SMOKE_IMAGE" >/dev/null
elif [ "$(uname -s)" = "Darwin" ]; then
  isolation="sandbox-exec with network denied"
else
  isolation="proxy sink and classification proof (no per-process network block on this runner)"
fi
echo "isolation: $isolation"

status=0
cli() { # <args...>; stdout -> $work/out, stderr -> $work/err, exit code -> $status
  status=0
  if [ -n "${SMOKE_IMAGE:-}" ]; then
    docker run --rm --network none --user "$(id -u):$(id -g)" \
      -v "$pkg:/smoke/bin:ro" -v "$work/cfg:/smoke/cfg:ro" \
      -e HOME=/tmp -e XDG_CONFIG_HOME=/tmp/config -e XDG_DATA_HOME=/tmp/data \
      -e XDG_CACHE_HOME=/tmp/cache -e XDG_STATE_HOME=/tmp/state \
      -e HTTPS_PROXY="$sink" -e HTTP_PROXY="$sink" -e ALL_PROXY="$sink" \
      "$SMOKE_IMAGE" "/smoke/bin/$exe" "$@" >"$work/out" 2>"$work/err" || status=$?
    return 0
  fi
  local run=("$bin")
  if [ "$(uname -s)" = "Darwin" ]; then
    run=(sandbox-exec -p '(version 1)(allow default)(deny network*)' "$bin")
  fi
  env -u CUOTAI_CONFIG \
    HOME="$work/home" USERPROFILE="$(native_path "$work/home")" \
    APPDATA="$(native_path "$work/home")" LOCALAPPDATA="$(native_path "$work/home")" \
    XDG_CONFIG_HOME="$work/home/config" XDG_DATA_HOME="$work/home/data" \
    XDG_CACHE_HOME="$work/home/cache" XDG_STATE_HOME="$work/home/state" \
    HTTPS_PROXY="$sink" HTTP_PROXY="$sink" ALL_PROXY="$sink" \
    https_proxy="$sink" http_proxy="$sink" all_proxy="$sink" NO_PROXY="" no_proxy="" \
    "${run[@]}" "$@" >"$work/out" 2>"$work/err" || status=$?
}

if [ -n "${SMOKE_IMAGE:-}" ]; then
  cfg_arg=/smoke/cfg/smoke.toml
else
  cfg_arg="$(native_path "$work/cfg/smoke.toml")"
fi

fail() {
  echo "FAIL: $*" >&2
  echo "--- stdout" >&2; cat "$work/out" >&2 || true
  echo "--- stderr" >&2; cat "$work/err" >&2 || true
  exit 1
}

# 1. --version: one line whose last word is the release version.
cli --version
[ "$status" -eq 0 ] || fail "--version exited $status"
reported="$(tr -d '\r' < "$work/out")"
[ "$(printf '%s\n' "$reported" | grep -c .)" -eq 1 ] || fail "--version printed more than one line"
[ "${reported##* }" = "$version" ] || fail "--version reported '$reported', expected version $version"
echo "ok: --version -> $reported"

# 2. --list-providers: exit 0 and lists the providers the sample uses.
cli --list-providers
[ "$status" -eq 0 ] || fail "--list-providers exited $status"
while read -r provider; do
  tr -d '\r' < "$work/out" | grep -q "^$provider " || fail "--list-providers does not list $provider"
done < <(sed -n 's/^provider *= *"\([a-z0-9_]*\)".*/\1/p' "$config" | sort -u)
echo "ok: --list-providers ($(grep -c . "$work/out") providers)"

# 3. --once: every account fails configuration, so the exit code is 2.
cli --config "$cfg_arg" --once --no-status
[ "$status" -eq 2 ] || fail "--once exited $status, expected 2 (account failures)"
[ -s "$work/out" ] || fail "--once printed nothing"
echo "ok: --once -> exit 2"

# 4. --json: the same refresh as data; every account is classified before
# any HTTP request, which is the no-network proof on every system.
accounts="$(grep -c '^\[\[accounts\]\]' "$config")"
cli --config "$cfg_arg" --json --no-status
[ "$status" -eq 2 ] || fail "--json exited $status, expected 2"
if command -v jq >/dev/null 2>&1; then
  classified="$(tr -d '\r' < "$work/out" | jq -e --argjson n "$accounts" '
    (.reports | length) == $n
    and all(.reports[]; .status == "needs_configuration" and (.metrics | length) == 0)
  ' || true)"
else
  # Runner images without jq (Windows ARM64) parse the report with PowerShell.
  # shellcheck disable=SC2016 # PowerShell expands its own variables.
  classified="$(SMOKE_JSON="$(native_path "$work/out")" SMOKE_ACCOUNTS="$accounts" \
    pwsh -NoProfile -NonInteractive -Command '
      $r = (Get-Content -Raw -LiteralPath $env:SMOKE_JSON | ConvertFrom-Json).reports
      $ok = @($r).Count -eq [int]$env:SMOKE_ACCOUNTS -and
        @($r | Where-Object { $_.status -ne "needs_configuration" -or @($_.metrics).Count -ne 0 }).Count -eq 0
      if ($ok) { "true" } else { "false" }' | tr -d '\r' || true)"
fi
[ "$classified" = "true" ] || fail "--json did not classify all $accounts accounts as needs_configuration"
echo "ok: --json -> $accounts accounts need configuration, no provider was queried"

echo "CLI smoke passed for $(basename "$archive") ($isolation)"
