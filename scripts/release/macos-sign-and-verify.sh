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

widgets_dir="apps/desktop/widgets/macos"
plugin="$app/Contents/PlugIns/CuotaiWidgets.appex"
[ -d "$plugin" ] || { echo "missing $plugin (run macos-embed-widgets.sh first)" >&2; exit 1; }
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# PRD 0014 RF2.1/RF3.1: sign inside-out. `--deep` signs every nested piece
# first, then the extension and the app are re-signed with their own
# entitlements (a `--deep` pass would give the extension none, and WidgetKit
# refuses an unsandboxed extension).
if [ "${CUOTAI_MACOS_SIGNED:-}" = "1" ]; then
  : "${APPLE_SIGNING_IDENTITY:?signing keychain is ready but identity is unavailable}"
  identity_args=(--options runtime --timestamp --sign "$APPLE_SIGNING_IDENTITY")
  sign_args=(--force --timestamp --sign "$APPLE_SIGNING_IDENTITY")
  description="Developer ID"
else
  identity_args=(--sign -)
  sign_args=(--force --sign -)
  description="ad-hoc"
fi
codesign --force --deep "${identity_args[@]}" "$app"
codesign --force "${identity_args[@]}" --entitlements "$widgets_dir/CuotaiWidgets.entitlements" "$plugin"
codesign --force "${identity_args[@]}" "$app"
codesign --verify --deep --strict --verbose=2 "$app"
codesign -d --entitlements - --xml "$plugin" 2>/dev/null | grep -q 'com.apple.security.app-sandbox' || {
  echo "the widget extension is not sandboxed" >&2
  exit 1
}

# Recreate the image only after the final app signature exists, so the payload
# users install is byte-for-byte the payload verified above.
rm -f "$dmg"
hdiutil create -volname Cuotai -srcfolder "$app" -ov -format UDZO "$dmg"
codesign "${sign_args[@]}" "$dmg"
codesign --verify --strict --verbose=2 "$dmg"
echo "applied and verified $description signatures on the app payload and DMG container"
