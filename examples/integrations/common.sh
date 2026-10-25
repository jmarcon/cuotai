# Shared by the Cuotai status-bar recipes (PRD 0031 RF1). POSIX sh, needs
# curl and jq. Source it; it reads GET /v1/limits of `cuotai --serve` (or the
# desktop app's local server) and sets:
#
#   cuotai_pct       percent consumed of the tightest account's binding metric
#   cuotai_name      that account's name
#   cuotai_severity  ok | warning | critical | outage
#   cuotai_reset     its reset time (RFC 3339) or empty
#
# Environment: CUOTAI_URL (default http://127.0.0.1:6738), CUOTAI_TOKEN or
# CUOTAI_TOKEN_FILE (the file `cuotai --serve` keeps in its data directory).
# Returns 1 when the server cannot be read, so a bar can show "offline".

cuotai_read() {
  cuotai_url=${CUOTAI_URL:-http://127.0.0.1:6738}
  cuotai_token=${CUOTAI_TOKEN:-}
  if [ -z "$cuotai_token" ] && [ -n "${CUOTAI_TOKEN_FILE:-}" ]; then
    cuotai_token=$(cat "$CUOTAI_TOKEN_FILE") || return 1
  fi
  if [ -n "$cuotai_token" ]; then
    cuotai_body=$(curl -fsS --max-time 3 -H "Authorization: Bearer $cuotai_token" "$cuotai_url/v1/limits") || return 1
  else
    cuotai_body=$(curl -fsS --max-time 3 "$cuotai_url/v1/limits") || return 1
  fi
  cuotai_fields=$(printf '%s' "$cuotai_body" | jq -r '
    (.summary.tightest // {}) as $t
    | [ (if $t.used_ratio then ($t.used_ratio * 100 | round | tostring) else "" end),
        ($t.name // ""),
        (if .summary.worst_class == "exhausted" then "critical"
         elif (.summary.worst_incident // "") | test("^(partial_outage|major_outage)$") then "outage"
         elif ($t.used_ratio // 0) >= 0.9 then "critical"
         elif ($t.used_ratio // 0) >= 0.75 or ((.summary.worst_incident // "") == "degraded") then "warning"
         else "ok" end),
        ($t.resets_at // "") ] | join("\t")') || return 1
  cuotai_pct=$(printf '%s' "$cuotai_fields" | cut -f1)
  cuotai_name=$(printf '%s' "$cuotai_fields" | cut -f2)
  cuotai_severity=$(printf '%s' "$cuotai_fields" | cut -f3)
  cuotai_reset=$(printf '%s' "$cuotai_fields" | cut -f4)
}

# "78% Personal", or "no data" while nothing has a reading yet.
cuotai_label() {
  if [ -n "$cuotai_pct" ]; then
    printf '%s%% %s' "$cuotai_pct" "$cuotai_name"
  else
    printf 'no data'
  fi
}
