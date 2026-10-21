#!/usr/bin/env bash
# Collects the macOS Tauri bundle outputs into dist/ under their stable
# names (PRD 0021 RF2). Also creates the updater archive Cuotai.app.tar.gz
# and signs it with the Tauri updater key when that secret is configured
# (the key is never used for anything else). Dry runs skip the updater
# archive honestly.
set -euo pipefail

: "${VERSION:?}"

bundle="apps/desktop/src-tauri/target/universal-apple-darwin/release/bundle"
dist="dist"
mkdir -p "$dist"
dist_abs="$(cd "$dist" && pwd)"

shopt -s nullglob
dmgs=("$bundle"/dmg/*.dmg)
[ "${#dmgs[@]}" -eq 1 ] || {
  echo "expected exactly one DMG in $bundle/dmg, found ${#dmgs[@]}" >&2
  exit 1
}
cp "${dmgs[0]}" "${dist_abs}/Cuotai_${VERSION}_universal.dmg"
echo "collected Cuotai_${VERSION}_universal.dmg"

app="$bundle/macos/Cuotai.app"
[ -d "$app" ] || { echo "missing $app" >&2; exit 1; }

if [ "${DRY_RUN:-false}" = "true" ]; then
  echo "dry-run: updater archive skipped"
  exit 0
fi
if [ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ] \
  || [ -z "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}" ]; then
  echo "complete Tauri updater signing credentials are unavailable: no updater archive"
  exit 0
fi

# Updater archive: the .app for PRD 0018. macOS ships bsdtar, so no GNU
# --sort/--owner flags here.
(cd "$bundle/macos" && COPYFILE_DISABLE=1 tar -czf "${dist_abs}/Cuotai.app.tar.gz" Cuotai.app)

# The pinned Tauri signer writes the sibling `.sig` itself. Bind the signature
# to the application version, discard informational stdout, then validate the
# generated file rather than scraping terminal text.
archive="${dist_abs}/Cuotai.app.tar.gz"
signature="${archive}.sig"
(cd apps/desktop && cargo tauri signer sign --app-version "$VERSION" "$archive" >/dev/null)
[ -s "$signature" ] || { echo "Tauri signer did not create $signature" >&2; exit 1; }
sig="$(tr -d '[:space:]' < "$signature")"
case "$sig" in
  ''|*[!A-Za-z0-9+/=]*)
    echo "Tauri signer created a malformed base64 signature" >&2
    exit 1
    ;;
esac
printf '%s\n' "$sig" > "$signature"
echo "updater archive Cuotai.app.tar.gz signed for version $VERSION"
