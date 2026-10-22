#!/usr/bin/env bash
# Updater-signs a collected artifact and records it in the job's signing
# fragment (PRD 0018 RF1.1/RF1.2, PRD 0021 RF3.4). Used where the bytes the
# updater ships only exist after platform signing: the Windows installers,
# whose Authenticode signature is applied after Tauri bundles them, so an
# updater signature computed at bundle time would not verify.
#
# The Tauri private key is read from the environment, never printed and never
# written anywhere else; it is used for nothing but this signature.
#
# Usage: updater-sign.sh <artifact> <fragment.json>
#   artifact    path to the artifact the manifest will publish
#   fragment    per-job signing fragment to extend with the updater proof
#
# The artifact keeps the signing record it already has (Authenticode on
# Windows) and gains the detached `.sig` as `updater_signature`, so one
# artifact carries two proofs without ever being listed twice.
#
# A dry run or a run without the key skips honestly: the fragment then reports
# the updater as unavailable with the reason, and no `.sig` is claimed.
set -euo pipefail

: "${VERSION:?}"
artifact="${1:?path of the artifact to updater-sign}"
fragment="${2:?signing fragment to extend}"
sig="${artifact}.sig"

[ -f "$artifact" ] || { echo "missing updater artifact $artifact" >&2; exit 1; }
[ -f "$fragment" ] || { echo "missing signing fragment $fragment" >&2; exit 1; }

# The signer runs with `apps/desktop` as its working directory (where the
# desktop manifest lives), so it needs an absolute path.
artifact_abs="$(cd "$(dirname -- "$artifact")" && pwd)/$(basename -- "$artifact")"
name="$(basename -- "$artifact")"

report_unavailable() { # <reason>
  jq --arg r "$1" '
    .updater = {available: false, artifacts: [], reason: $r}
  ' "$fragment" > "$fragment.tmp"
  mv "$fragment.tmp" "$fragment"
}

if [ "${DRY_RUN:-false}" = "true" ]; then
  report_unavailable "dry-run: updater artifacts are not signed"
  echo "dry-run: updater signature skipped for $name"
  exit 0
fi

if [ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ] \
  || [ -z "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}" ]; then
  report_unavailable \
    "complete Tauri updater signing credentials are not configured in the release environment"
  echo "no complete Tauri updater signing credentials: $name is not updater-signed"
  exit 0
fi

# The pinned Tauri signer writes the sibling `.sig` itself and binds the
# signature to the app version in its trusted comment. Informational stdout is
# discarded; the generated file is validated instead.
rm -f "$sig"
(cd apps/desktop && cargo tauri signer sign --app-version "$VERSION" "$artifact_abs" >/dev/null)
[ -s "$sig" ] || { echo "Tauri signer did not create $sig" >&2; exit 1; }
encoded="$(tr -d '[:space:]' < "$sig")"
case "$encoded" in
  ''|*[!A-Za-z0-9+/=]*)
    echo "Tauri signer created a malformed base64 signature" >&2
    exit 1
    ;;
esac
printf '%s\n' "$encoded" > "$sig"

# One artifact, two proofs (PRD 0018 RF1.1 / PRD 0021 RF3.4): the installer
# keeps the Authenticode (or unsigned) record it already has, and the detached
# `.sig` is attached to it as `updater_signature`. Adding a second record with
# the same payload name is what `signing::merge` rejects, so the artifact would
# never reach a release.
jq --arg a "$name" --arg s "$name.sig" '
  (.artifacts // []) as $all
  | if any($all[]; .name == $a) then
      $all | map(if .name == $a then . + {updater_signature: $s} else . end)
    else
      error("the updater artifact has no existing signing record: \($a)")
    end as $records
  | .artifacts = $records
  | .updater = {
      available: true,
      artifacts: ((.updater.artifacts // []) + [$a, $s])
    }
' "$fragment" > "$fragment.tmp"
mv "$fragment.tmp" "$fragment"

echo "updater-signed $name for version $VERSION"