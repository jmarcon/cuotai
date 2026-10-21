#!/usr/bin/env bash
# Notarizes, staples and Gatekeeper-checks both the app payload and the DMG
# when the complete protected credential set was accepted by the keychain
# step. The DMG is recreated after the app is stapled.
set -euo pipefail

if [ "${DRY_RUN:-false}" = "true" ] || [ "${CUOTAI_MACOS_SIGNED:-}" != "1" ]; then
  echo "trusted Apple signing/notarization unavailable: keeping verified ad-hoc artifacts"
  exit 0
fi
: "${APPLE_SIGNING_IDENTITY:?}" "${RUNNER_TEMP:?}"

bundle="apps/desktop/src-tauri/target/universal-apple-darwin/release/bundle"
app="$bundle/macos/Cuotai.app"
shopt -s nullglob
dmgs=("$bundle"/dmg/*.dmg)
[ -d "$app" ] || { echo "missing $app" >&2; exit 1; }
[ "${#dmgs[@]}" -eq 1 ] || {
  echo "expected exactly one DMG in $bundle/dmg, found ${#dmgs[@]}" >&2
  exit 1
}
dmg="${dmgs[0]}"

codesign --verify --deep --strict --verbose=2 "$app"
codesign --verify --strict --verbose=2 "$dmg"

p8=""
app_zip="$RUNNER_TEMP/cuotai-notarization-app.zip"
cleanup() {
  [ -n "$p8" ] && rm -f "$p8"
  rm -f "$app_zip"
}
trap cleanup EXIT

auth_args=()
api_complete=false
[ -n "${APPLE_API_KEY:-}" ] \
  && [ -n "${APPLE_API_KEY_ID:-}" ] \
  && [ -n "${APPLE_API_ISSUER:-}" ] \
  && api_complete=true
apple_id_complete=false
[ -n "${APPLE_ID:-}" ] \
  && [ -n "${APPLE_PASSWORD:-}" ] \
  && [ -n "${APPLE_TEAM_ID:-}" ] \
  && apple_id_complete=true
if [ "$api_complete" = "true" ]; then
  p8="$(mktemp -t AuthKey).p8"
  echo "$APPLE_API_KEY" | base64 --decode > "$p8"
  auth_args=(--key "$p8" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER")
elif [ "$apple_id_complete" = "true" ]; then
  auth_args=(--apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID")
else
  echo "complete notarization credentials disappeared after signing was enabled" >&2
  exit 1
fi

rm -f "$app_zip"
ditto -c -k --keepParent "$app" "$app_zip"
xcrun notarytool submit "$app_zip" "${auth_args[@]}" --wait
xcrun stapler staple "$app"
xcrun stapler validate "$app"
spctl --assess --type execute --verbose=2 "$app"

# Stapling changes the app bytes, so rebuild and re-sign the DMG before its
# independent notarization submission.
rm -f "$dmg"
hdiutil create -volname Cuotai -srcfolder "$app" -ov -format UDZO "$dmg"
codesign --force --timestamp --sign "$APPLE_SIGNING_IDENTITY" "$dmg"
codesign --verify --strict --verbose=2 "$dmg"
xcrun notarytool submit "$dmg" "${auth_args[@]}" --wait
xcrun stapler staple "$dmg"
xcrun stapler validate "$dmg"
spctl --assess --type open --context context:primary-signature --verbose=2 "$dmg"

echo "CUOTAI_MACOS_NOTARIZED=1" >> "$GITHUB_ENV"
echo "notarized, stapled and Gatekeeper-validated the app payload and DMG container"
