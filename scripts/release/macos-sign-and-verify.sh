#!/usr/bin/env bash
# Signs the complete app before recreating its DMG, then signs and verifies the
# container. Uses Developer ID on configured tag runs and explicit ad-hoc
# signatures otherwise.
set -euo pipefail

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

if [ "${CUOTAI_MACOS_SIGNED:-}" = "1" ]; then
  : "${APPLE_SIGNING_IDENTITY:?signing keychain is ready but identity is unavailable}"
  codesign --force --deep --options runtime --timestamp \
    --sign "$APPLE_SIGNING_IDENTITY" "$app"
  sign_args=(--force --timestamp --sign "$APPLE_SIGNING_IDENTITY")
  description="Developer ID"
else
  codesign --force --deep --sign - "$app"
  sign_args=(--force --sign -)
  description="ad-hoc"
fi
codesign --verify --deep --strict --verbose=2 "$app"

# Recreate the image only after the final app signature exists, so the payload
# users install is byte-for-byte the payload verified above.
rm -f "$dmg"
hdiutil create -volname Cuotai -srcfolder "$app" -ov -format UDZO "$dmg"
codesign "${sign_args[@]}" "$dmg"
codesign --verify --strict --verbose=2 "$dmg"
echo "applied and verified $description signatures on the app payload and DMG container"
