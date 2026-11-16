# Shared by the Cuotai status-bar recipes (PRD 0031 RF1). POSIX sh, needs
# curl and jq. Source it; it reads GET /v1/limits of `cuotai --serve` (or the
# desktop app's local server) and sets:
#
#   cuotai_pct       percent LEFT of the tightest account's binding metric,
#                    as every Cuotai surface shows it
#   cuotai_name      that account's label: its name, or the provider's when
#                    it has none or an e-mail-like one (tabs and line breaks
#                    become spaces, so they cannot shift the fields below)
#   cuotai_severity  ok | warning | critical | outage, from the menu bar's
#                    levels: warning under 70 % left, critical under 30 %
#                    or exhausted, outage for a provider outage
#   cuotai_reset     its reset time (RFC 3339) or empty
#
# Needs a server recent enough to send summary.tightest.level; the minimum
# version is in docs/integrations/README.md.
#
# Environment: CUOTAI_URL (default http://127.0.0.1:6738; only a loopback
# http://127.0.0.1:PORT, http://[::1]:PORT or http://localhost:PORT, read as
# 127.0.0.1), CUOTAI_TOKEN_FILE (the file `cuotai --serve` keeps in its data
# directory) or CUOTAI_TOKEN.
# Returns 1 when the server cannot be read, so a bar can show "offline".
#
# The token never reaches a command line: `printf` is a shell builtin and
# hands the header to curl as a config on stdin (-K -), so `ps` cannot show
# it. curl ignores ~/.curlrc (-q) and every proxy (--noproxy '*'), speaks
# only http (--proto '=http'), follows no redirect and stops after 3 seconds or
# 4 MiB. Prefer `cuotai --status-line --from-server` for a one-line bar.

cuotai_get() {
  curl -q -g -K - --noproxy '*' --proto '=http' --max-redirs 0 --max-time 3 \
    --max-filesize 4194304 -fsS "$cuotai_url/v1/limits"
}

cuotai_read() {
  cuotai_url=${CUOTAI_URL:-http://127.0.0.1:6738}
  cuotai_url=${cuotai_url%/}
  case $cuotai_url in
    http://127.0.0.1:*) cuotai_port=${cuotai_url#http://127.0.0.1:} cuotai_host=127.0.0.1 ;;
    http://localhost:*) cuotai_port=${cuotai_url#http://localhost:} cuotai_host=127.0.0.1 ;;
    'http://[::1]:'*) cuotai_port=${cuotai_url#'http://[::1]:'} cuotai_host='[::1]' ;;
    *) return 1 ;;
  esac
  case $cuotai_port in
    '' | *[!0-9]*) return 1 ;;
  esac
  cuotai_url=http://$cuotai_host:$cuotai_port
  cuotai_token=${CUOTAI_TOKEN:-}
  if [ -z "$cuotai_token" ] && [ -n "${CUOTAI_TOKEN_FILE:-}" ]; then
    cuotai_token=$(cat "$CUOTAI_TOKEN_FILE") || return 1
  fi
  if [ -n "$cuotai_token" ]; then
    # RFC 6750 characters only, so the token cannot break out of the config.
    case $cuotai_token in
      *[!A-Za-z0-9._~+/=-]*) return 1 ;;
    esac
    cuotai_body=$(printf 'header = "Authorization: Bearer %s"\n' "$cuotai_token" | cuotai_get) || return 1
  else
    cuotai_body=$(cuotai_get </dev/null) || return 1
  fi
  cuotai_fields=$(printf '%s' "$cuotai_body" | jq -r '
    (.summary.tightest // {}) as $t
    | ($t.remaining_ratio // null) as $left
    | [ (if $left == null then "" else ($left * 100 | round | tostring) end),
        (($t.label // $t.name // "") | gsub("[\\t\\n\\r]+"; " ")),
        (if .summary.worst_class == "exhausted" then "critical"
         elif (.summary.worst_incident // "") | test("^(partial_outage|major_outage)$") then "outage"
         elif $t.level == "bad" then "critical"
         elif $t.level == "warn" or ((.summary.worst_incident // "") == "degraded") then "warning"
         else "ok" end),
        ($t.resets_at // "") ] | join("\t")') || return 1
  cuotai_pct=$(printf '%s' "$cuotai_fields" | cut -f1)
  cuotai_name=$(printf '%s' "$cuotai_fields" | cut -f2)
  cuotai_severity=$(printf '%s' "$cuotai_fields" | cut -f3)
  cuotai_reset=$(printf '%s' "$cuotai_fields" | cut -f4)
}

# "45% left · Personal", or "no data" while nothing has a reading yet.
cuotai_label() {
  if [ -n "$cuotai_pct" ]; then
    printf '%s%% left · %s' "$cuotai_pct" "$cuotai_name"
  else
    printf 'no data'
  fi
}
