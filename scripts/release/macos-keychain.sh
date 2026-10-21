#!/usr/bin/env bash
# Imports the Apple Developer ID certificate into a temporary keychain when
# the signing secrets are configured (PRD 0021 RF3.1). Never prints secret
# values; on dry runs or without secrets it reports "absent" and the build
# falls back to an ad-hoc signature marked unsigned.
#
# Env: the certificate trio below and one complete notarization-presence flag
# are required together. Incomplete credentials select the explicit ad-hoc
# path. Notarization secret values are not exposed to this step.
#   DRY_RUN, APPLE_CERTIFICATE, APPLE_CERTIFICATE_PASSWORD,
#   APPLE_SIGNING_IDENTITY, HAS_APPLE_API_NOTARY, HAS_APPLE_ID_NOTARY
#
# Writes CUOTAI_MACOS_SIGNED=1 to $GITHUB_ENV only when signing and
# notarization can both complete.
set -euo pipefail

if [ "${DRY_RUN:-false}" = "true" ]; then
  echo "dry-run: Apple signing skipped; the app will be ad-hoc signed and marked unsigned"
  exit 0
fi

cert_complete=false
[ -n "${APPLE_CERTIFICATE:-}" ] \
  && [ -n "${APPLE_CERTIFICATE_PASSWORD:-}" ] \
  && [ -n "${APPLE_SIGNING_IDENTITY:-}" ] \
  && cert_complete=true
notary_complete=false
if [ "${HAS_APPLE_API_NOTARY:-false}" = "true" ] \
  || [ "${HAS_APPLE_ID_NOTARY:-false}" = "true" ]; then
  notary_complete=true
fi
if [ "$cert_complete" != "true" ] || [ "$notary_complete" != "true" ]; then
  echo "complete Apple signing and notarization credentials are unavailable: using verified ad-hoc signatures"
  exit 0
fi

: "${RUNNER_TEMP:?}"
keychain="$RUNNER_TEMP/cuotai-release.keychain-db"
keychain_password="$(openssl rand -base64 32)"
p12="$RUNNER_TEMP/cuotai-release-cert.p12"
cleanup_p12() {
  rm -f "$p12"
}
trap cleanup_p12 EXIT

echo "$APPLE_CERTIFICATE" | base64 --decode > "$p12"
security create-keychain -p "$keychain_password" "$keychain"
security set-keychain-settings -lut 3600 "$keychain"
security unlock-keychain -p "$keychain_password" "$keychain"
security import "$p12" -k "$keychain" -P "$APPLE_CERTIFICATE_PASSWORD" \
  -T /usr/bin/codesign -T /usr/bin/security
security set-key-partition-list -S apple-tool:,apple:,codesign: \
  -s -k "$keychain_password" "$keychain" >/dev/null
rm -f "$p12"
security list-keychains -d user -s "$keychain" $(security list-keychains -d user | sed 's/"//g')

if ! security find-identity -v "$keychain" | grep -qF "$APPLE_SIGNING_IDENTITY"; then
  echo "configured Apple signing identity not found in the imported certificate" >&2
  exit 1
fi

echo "CUOTAI_MACOS_SIGNED=1" >> "$GITHUB_ENV"
echo "Apple Developer ID identity available; the build will sign with it"
