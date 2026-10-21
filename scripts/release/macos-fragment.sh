#!/usr/bin/env bash
# Writes the macOS signing fragment (PRD 0021 RF3.4) from the markers the
# earlier steps set (CUOTAI_MACOS_SIGNED / CUOTAI_MACOS_NOTARIZED) and the
# files present in dist/. Never invents a signed state.
set -euo pipefail

: "${VERSION:?}"

dmg="Cuotai_${VERSION}_universal.dmg"
mkdir -p fragments

signed=false
method="ad-hoc"
status="unsigned"
note="app payload and DMG container are ad-hoc signed but have no trusted identity; Gatekeeper requires a manual override"
if [ "${CUOTAI_MACOS_SIGNED:-}" = "1" ]; then
  signed=true
  method="developer-id"
  status="signed"
  note="Developer ID signature"
fi
if [ "${CUOTAI_MACOS_NOTARIZED:-}" = "1" ]; then
  status="signed+notarized"
  note="Developer ID signatures; app payload and DMG notarized, stapled and Gatekeeper-validated"
fi

has_updater=false
if [ -f "dist/Cuotai.app.tar.gz" ] && [ -f "dist/Cuotai.app.tar.gz.sig" ]; then
  has_updater=true
fi

jq -n \
  --arg dmg "$dmg" \
  --argjson signed "$signed" \
  --arg method "$method" \
  --arg status "$status" \
  --arg note "$note" \
  --argjson has_updater "$has_updater" \
  '
  {
    artifacts: [
      {name: $dmg, signed: $signed, method: $method, status: $status, note: $note}
    ] + (if $has_updater then [
      {name: "Cuotai.app.tar.gz", signed: true, method: "tauri-updater-key", status: "signed"},
      {name: "Cuotai.app.tar.gz.sig", signed: true, method: "tauri-updater-key", status: "signed"}
    ] else [] end),
    updater: (if $has_updater then
      {available: true, artifacts: ["Cuotai.app.tar.gz", "Cuotai.app.tar.gz.sig"]}
    else
      {available: false, artifacts: [],
       reason: "complete Tauri updater signing credentials not configured or dry-run"}
    end)
  }
  ' > "fragments/macos.json"

echo "wrote fragments/macos.json"
