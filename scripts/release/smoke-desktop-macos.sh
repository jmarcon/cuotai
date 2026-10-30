#!/usr/bin/env bash
# Smoke test of the macOS desktop DMG users download (PRD 0021 RF6.2):
#   * the DMG container and the app inside it pass `codesign --verify`;
#   * the signature matches what signing-state.json claims (Developer ID when
#     the state says so, ad-hoc otherwise) — never more, never less;
#   * the main executable is universal (`lipo` reports arm64 and x86_64);
#   * Info.plist carries the release version;
#   * the app starts and is still running after a grace period, with a
#     throw-away HOME so no runner or user state is read or written.
# Gatekeeper (`spctl`) is only meaningful with the real notarized identity and
# stays in the manual checklist of docs/releasing.md.
# Usage: smoke-desktop-macos.sh <dmg> <expected-version> <signing-state.json>
set -euo pipefail

dmg="${1:?DMG path}"
version="${2:?expected version}"
state="${3:?signing-state.json}"
grace="${SMOKE_GRACE_SECONDS:-15}"
[ -f "$dmg" ] || { echo "missing $dmg" >&2; exit 1; }

work="$(mktemp -d)"
mount="$work/mount"
pid=""
cleanup() {
  [ -n "$pid" ] && kill "$pid" 2>/dev/null || true
  hdiutil detach "$mount" -quiet 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT

method="$(jq -r --arg n "$(basename "$dmg")" '.artifacts[] | select(.name == $n) | .method' "$state")"
[ -n "$method" ] || { echo "$(basename "$dmg") has no signing-state entry" >&2; exit 1; }

codesign --verify --strict --verbose=2 "$dmg"
mkdir -p "$mount"
hdiutil attach "$dmg" -nobrowse -readonly -noautoopen -mountpoint "$mount" -quiet
apps=("$mount"/*.app)
[ "${#apps[@]}" -eq 1 ] && [ -d "${apps[0]}" ] || {
  echo "expected exactly one app in the DMG, found ${#apps[@]}" >&2
  exit 1
}
# Run a copy: the read-only image must not be the working directory of a
# running process when it is detached.
ditto "${apps[0]}" "$work/Cuotai.app"
hdiutil detach "$mount" -quiet
app="$work/Cuotai.app"

codesign --verify --deep --strict --verbose=2 "$app"
# The signing-state entry describes both signatures: the DMG container and
# the app inside it must each carry the recorded kind.
assert_signature_kind() { # <path> <label>
  local details
  details="$(codesign -dvv "$1" 2>&1)"
  case "$method" in
    developer-id)
      grep -q '^Authority=Developer ID Application' <<<"$details" || {
        echo "signing state claims Developer ID but the $2 is not signed by one" >&2
        exit 1
      }
      ;;
    ad-hoc)
      grep -q '^Signature=adhoc' <<<"$details" || {
        echo "signing state claims an ad-hoc signature but the $2 carries another one" >&2
        exit 1
      }
      ;;
    *) echo "unexpected macOS signing method '$method'" >&2; exit 1 ;;
  esac
}
assert_signature_kind "$dmg" "DMG"
assert_signature_kind "$app" "app"
echo "ok: codesign verified ($method)"

# PRD 0014 RF3.1: the WidgetKit extension ships inside the app, signed the
# same way, sandboxed, and (Developer ID only) in the shared app group.
plugin="$app/Contents/PlugIns/CuotaiWidgets.appex"
[ -d "$plugin" ] || { echo "the app does not embed CuotaiWidgets.appex" >&2; exit 1; }
codesign --verify --strict --verbose=2 "$plugin"
assert_signature_kind "$plugin" "widget extension"
plugin_entitlements="$(codesign -d --entitlements - --xml "$plugin" 2>/dev/null)"
grep -q 'com.apple.security.app-sandbox' <<<"$plugin_entitlements" || {
  echo "the widget extension is not sandboxed" >&2
  exit 1
}
if [ "$method" = "developer-id" ]; then
  app_entitlements="$(codesign -d --entitlements - --xml "$app" 2>/dev/null)"
  for entitlements in "$plugin_entitlements" "$app_entitlements"; do
    grep -q 'WL9XSC6H36.dev.cuotai.desktop' <<<"$entitlements" || {
      echo "the app or its widget extension lacks the app group entitlement" >&2
      exit 1
    }
  done
fi
# Linked with `@main` as its entry point the extension registers but traps at
# launch, so the widget gallery stays empty while every check above passes.
plugin_binary="$plugin/Contents/MacOS/CuotaiWidgets"
for arch in arm64 x86_64; do
  nm -arch "$arch" -u "$plugin_binary" | grep -q '^_NSExtensionMain$' || {
    echo "the widget extension ($arch) is not linked with the _NSExtensionMain entry point" >&2
    exit 1
  }
done
echo "ok: widget extension embedded and signed ($method)"

executable="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$app/Contents/Info.plist")"
binary="$app/Contents/MacOS/$executable"
archs="$(lipo -archs "$binary")"
lipo -info "$binary"
for arch in arm64 x86_64; do
  grep -qw "$arch" <<<"$archs" || { echo "$executable lacks the $arch slice ($archs)" >&2; exit 1; }
done
echo "ok: universal binary ($archs)"

bundle_version="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist")"
[ "$bundle_version" = "$version" ] || {
  echo "CFBundleShortVersionString is $bundle_version, expected $version" >&2
  exit 1
}
echo "ok: bundle version $bundle_version"

mkdir -p "$work/home"
HOME="$work/home" "$binary" >"$work/app.log" 2>&1 &
pid=$!
sleep "$grace"
if ! kill -0 "$pid" 2>/dev/null; then
  code=0
  wait "$pid" || code=$?
  pid=""
  echo "the app exited with $code within ${grace}s of starting" >&2
  cat "$work/app.log" >&2
  exit 1
fi
echo "ok: the app is running after ${grace}s"
kill "$pid"
wait "$pid" 2>/dev/null || true
pid=""
echo "macOS desktop smoke passed for $(basename "$dmg")"
