#!/usr/bin/env bash
# PRD 0014 RF3.1: builds the WidgetKit extension and embeds it in the app
# Tauri just bundled, at Cuotai.app/Contents/PlugIns/CuotaiWidgets.appex.
# Runs before macos-sign-and-verify.sh, which signs it inside-out with its
# own entitlements, so notarization sees one signed tree. Needs no secret:
# a full rehearsal embeds the same extension and signs it ad-hoc.
set -euo pipefail

: "${VERSION:?}"
app="${1:-apps/desktop/src-tauri/target/universal-apple-darwin/release/bundle/macos/Cuotai.app}"
[ -d "$app" ] || { echo "missing $app" >&2; exit 1; }

mkdir -p "$app/Contents/PlugIns"
apps/desktop/widgets/macos/build-appex.sh "$app/Contents/PlugIns" "$VERSION"
plugin="$app/Contents/PlugIns/CuotaiWidgets.appex"
point="$(/usr/libexec/PlistBuddy -c 'Print :NSExtension:NSExtensionPointIdentifier' "$plugin/Contents/Info.plist")"
[ "$point" = "com.apple.widgetkit-extension" ] || {
  echo "unexpected extension point $point" >&2
  exit 1
}
echo "embedded $plugin"
